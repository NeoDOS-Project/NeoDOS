use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::arch::x64::paging::{USER_LIMIT, alloc_user_slot};

const MAX_BIN_SIZE: usize = 64 * 1024;

impl DosShell {
    pub fn cmd_load(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: LOAD <filename>");
            println!("  Loads a flat binary and executes it in Ring 3.");
            return;
        }

        let filename = args[0];
        let full_path = self.resolve_absolute_path(filename);

        let slot = match alloc_user_slot() {
            Some(s) => s,
            None => { println!("Error: No free user memory slots."); return; }
        };

        if slot.stack_top > USER_LIMIT {
            println!("Error: User slot exceeds memory window.");
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return;
        }

        let mut entry = slot.code_base;

        let ok = crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    static mut NXE_BUF: [u8; MAX_BIN_SIZE] = [0u8; MAX_BIN_SIZE];
                    unsafe {
                        let buf_ptr: *mut [u8; MAX_BIN_SIZE] = core::ptr::addr_of_mut!(NXE_BUF);
                        core::ptr::write_bytes(buf_ptr as *mut u8, 0, MAX_BIN_SIZE);
                        match vfs.read(drive_idx, node.inode, 0, core::slice::from_raw_parts_mut(buf_ptr as *mut u8, MAX_BIN_SIZE)) {
                            Ok(n) if n > 0 => {
                                let data = core::slice::from_raw_parts((*buf_ptr).as_ptr(), n);
                                serial_println!("[SHELL] Loading raw {} bytes", n);
                                let dst = slot.code_base as *mut u8;
                                core::ptr::copy_nonoverlapping(data.as_ptr(), dst, n);
                                entry = slot.code_base;
                                true
                            }
                            Ok(_) => false,
                            Err(e) => { println!("Error reading '{}': {:?}", filename, e); false }
                        }
                    }
                }
                Err(_) => { println!("File not found: {}", filename); false }
            }
        });

        if !ok {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            println!("Error: Failed to load '{}'", filename);
            return;
        }

        let cwd_drive = self.current_drive as u8 - b'A';
        let pid = crate::usermode::spawn_usermode(
            entry, slot.stack_top, slot.slot_idx, cwd_drive, &self.current_dir, 0);

        println!("Loaded module '{}' (PID {}) in Ring 3", filename, pid);
    }
}
