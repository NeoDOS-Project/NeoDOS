// src/shell/commands/run.rs

use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::arch::x64::paging::{USER_LIMIT, alloc_user_slot};

const MAX_BIN_SIZE: usize = 64 * 1024;

impl DosShell {
    pub fn cmd_run(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: RUN <filename>");
            println!("  Loads a flat binary and executes it in Ring 3.");
            return;
        }

        let filename = args[0];
        let full_path = self.resolve_absolute_path(filename);

        // ── 1. Allocate a per-process user slot ──
        let slot = match alloc_user_slot() {
            Some(s) => s,
            None => {
                println!("Error: No free user memory slots.");
                return;
            }
        };

        if slot.stack_top > USER_LIMIT {
            println!("Error: User slot exceeds memory window.");
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return;
        }

        // ── 2. Find and read the binary ──
        let mut bin_size = 0;
        
        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    static mut BIN_BUF: [u8; MAX_BIN_SIZE] = [0u8; MAX_BIN_SIZE];
                    unsafe {
                        let buf_ptr: *mut [u8; MAX_BIN_SIZE] = core::ptr::addr_of_mut!(BIN_BUF);
                        (*buf_ptr).fill(0);
                        match vfs.read(drive_idx, node.inode, 0, &mut *buf_ptr) {
                            Ok(n) => {
                                bin_size = n;
                                if bin_size > 0 {
                                    // ── 3. Copy binary to the slot's code area ──
                                    let dst = slot.code_base as *mut u8;
                                    let src = core::ptr::addr_of!(BIN_BUF) as *const u8;
                                    core::ptr::copy_nonoverlapping(src, dst, bin_size);
                                }
                            }
                            Err(e) => {
                                println!("Error reading '{}': {:?}", filename, e);
                            }
                        }
                    }
                }
                Err(_) => {
                    println!("File not found: {}", filename);
                }
            }
        });

        if bin_size == 0 {
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return;
        }

        serial_println!("[RUN] '{}' -> {} bytes, slot 0x{:x}..0x{:x}",
            filename, bin_size, slot.code_base, slot.stack_top);

        // ── 4. Spawn as scheduler process ──
        let pid = crate::usermode::spawn_usermode(slot.code_base, slot.stack_top, slot.slot_idx);

        serial_println!("[RUN] Spawned PID {}, slot_idx={}", pid, slot.slot_idx);
        println!("Launching '{}' ({} bytes) in Ring 3 (PID {})...", filename, bin_size, pid);

        // ── 5. Wait for the process to complete ──
        crate::usermode::wait_for_process(pid);

        crate::usermode::clear_wait_pid();
        println!("Process '{}' (PID {}) exited.", filename, pid);
    }
}
