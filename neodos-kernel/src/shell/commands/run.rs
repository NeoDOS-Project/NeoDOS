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
            println!("  Loads a binary (ELF64 or flat) and executes it in Ring 3.");
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
        let mut entry = slot.code_base;

        crate::globals::with_vfs(|vfs| {
            match vfs.resolve_path(&full_path) {
                Ok((drive_idx, node)) => {
                    static mut NXE_BUF: [u8; MAX_BIN_SIZE] = [0u8; MAX_BIN_SIZE];
                    unsafe {
                        let buf_ptr: *mut [u8; MAX_BIN_SIZE] = core::ptr::addr_of_mut!(NXE_BUF);
                        (*buf_ptr).fill(0);
                        match vfs.read(drive_idx, node.inode, 0, &mut *buf_ptr) {
                            Ok(n) => {
                                bin_size = n;
                                if bin_size > 0 {
                                    // ── 3. Detect format: ELF or flat binary ──
                                    let is_elf = bin_size >= 4
                                        && (*buf_ptr)[0] == 0x7f
                                        && (*buf_ptr)[1] == b'E'
                                        && (*buf_ptr)[2] == b'L'
                                        && (*buf_ptr)[3] == b'F';
                                    if is_elf {
                                        // ELF64 binary
                                        let data = core::slice::from_raw_parts(
                                            core::ptr::addr_of!(*buf_ptr) as *const u8,
                                            bin_size,
                                        );
                                        match crate::elf::load_elf(data) {
                                            Some(result) => {
                                                entry = result.entry;
                                                serial_println!("[SHELL] ELF64: entry=0x{:x}", entry);
                                            }
                                            None => {
                                                println!("Error: Invalid or unsupported ELF binary.");
                                                bin_size = 0;
                                            }
                                        }
                                    } else {
                                        // Flat binary: copy raw to slot
                                        let dst = slot.code_base as *mut u8;
                                        let src = core::ptr::addr_of!(NXE_BUF) as *const u8;
                                        core::ptr::copy_nonoverlapping(src, dst, bin_size);
                                    }
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

        serial_println!("[SHELL] '{}' -> {} bytes, entry=0x{:x}",
            filename, bin_size, entry);

        // ── 4. Spawn as scheduler process (inherit shell's cwd) ──
        let cwd_drive = self.current_drive as u8 - b'A';
        let pid = crate::usermode::spawn_usermode(entry, slot.stack_top, slot.slot_idx, cwd_drive, &self.current_dir);

        serial_println!("[SHELL] Spawned PID {}, slot_idx={}", pid, slot.slot_idx);
        println!("Launching '{}' ({} bytes) in Ring 3 (PID {})...", filename, bin_size, pid);

        // ── 5. Wait for the process to complete ──
        crate::usermode::wait_for_process(pid);

        crate::usermode::clear_wait_pid();

        // ── 6. Recycle slot, free kernel stack, clean up remaining resources ──
        crate::scheduler::cleanup_terminated_process(pid);

        println!("Process '{}' (PID {}) exited.", filename, pid);
    }
}
