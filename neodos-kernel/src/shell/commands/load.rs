// src/shell/commands/load.rs

use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::arch::x64::paging::{USER_LIMIT, alloc_user_slot};
use crate::module_abi::{NdModuleHeader, NDM_ABI_VERSION};

const MAX_BIN_SIZE: usize = 64 * 1024;

impl DosShell {
    pub fn cmd_load(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: LOAD <filename>");
            println!("  Loads a driver module (.ndm) and executes it in Ring 3.");
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
                    static mut BIN_BUF: [u8; MAX_BIN_SIZE] = [0u8; MAX_BIN_SIZE];
                    unsafe {
                        let buf_ptr: *mut [u8; MAX_BIN_SIZE] = core::ptr::addr_of_mut!(BIN_BUF);
                        core::ptr::write_bytes(buf_ptr as *mut u8, 0, MAX_BIN_SIZE);
                        match vfs.read(drive_idx, node.inode, 0, core::slice::from_raw_parts_mut(buf_ptr as *mut u8, MAX_BIN_SIZE)) {
                            Ok(n) if n > 0 => {
                                let data = core::slice::from_raw_parts((*buf_ptr).as_ptr(), n);
                                if let Some(parsed) = NdModuleHeader::from_bytes(data) {
                                    serial_println!("[SHELL] NDM v{} {} '{}' ({}B code + {}B data)",
                                        NDM_ABI_VERSION, parsed.module_type.to_str(),
                                        parsed.name, parsed.code_slice.len(), parsed.data_slice.len());

                                    let slot_code = slot.code_base as *mut u8;
                                    let slot_data = slot.code_base.wrapping_add(parsed.code_slice.len() as u64) as *mut u8;

                                    core::ptr::copy_nonoverlapping(parsed.code_slice.as_ptr(), slot_code, parsed.code_slice.len());
                                    if !parsed.data_slice.is_empty() {
                                        core::ptr::copy_nonoverlapping(parsed.data_slice.as_ptr(), slot_data, parsed.data_slice.len());
                                    }

                                    let code_file_off = parsed.code_file_offset;
                                    let entry_file_off = parsed.entry_point_offset;
                                    entry = slot.code_base + (entry_file_off - code_file_off) as u64;
                                    true
                                } else {
                                    serial_println!("[SHELL] No NDM header, loading raw {} bytes", n);
                                    let dst = slot.code_base as *mut u8;
                                    core::ptr::copy_nonoverlapping((*buf_ptr).as_ptr(), dst, n);
                                    entry = slot.code_base;
                                    true
                                }
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
            entry, slot.stack_top, slot.slot_idx, cwd_drive, &self.current_dir);

        println!("Loaded module '{}' (PID {}) in Ring 3", filename, pid);
    }
}