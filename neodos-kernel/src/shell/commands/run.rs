// src/shell/commands/run.rs
//
// RUN <filename>
//
// Loads a flat binary from NeoDOS FS into a per-process user slot,
// spawns it as a scheduler-managed Ring-3 process, and waits
// for it to complete.

use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::arch::x64::paging::{USER_LIMIT, alloc_user_slot};

const MAX_BIN_SIZE: usize = 64 * 1024;

impl<'a> DosShell<'a> {
    pub fn cmd_run(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: RUN <filename>");
            println!("  Loads a flat binary from NeoDOS FS and executes it in Ring 3.");
            return;
        }

        let filename = args[0];

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
        let inode = match self.resolve_file_inode(filename) {
            Ok(i) => i,
            Err(_) => {
                println!("File not found: {}", filename);
                crate::arch::x64::paging::free_user_slot(slot.slot_idx);
                return;
            }
        };

        static mut BIN_BUF: [u8; MAX_BIN_SIZE] = [0u8; MAX_BIN_SIZE];

        let bin_size = unsafe {
            let buf_ptr: *mut [u8; MAX_BIN_SIZE] = core::ptr::addr_of_mut!(BIN_BUF);
            (*buf_ptr).fill(0);
            match self.fs.read_file_to_buf(inode, &mut *buf_ptr, self.cache, self.ata) {
                Ok(n) => n,
                Err(e) => {
                    println!("Error reading '{}': {:?}", filename, e);
                    crate::arch::x64::paging::free_user_slot(slot.slot_idx);
                    return;
                }
            }
        };

        if bin_size == 0 {
            println!("Error: '{}' is empty.", filename);
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return;
        }

        serial_println!("[RUN] '{}' -> {} bytes, slot 0x{:x}..0x{:x}",
            filename, bin_size, slot.code_base, slot.stack_top);

        // ── 3. Copy binary to the slot's code area ──
        unsafe {
            let dst = slot.code_base as *mut u8;
            let src = core::ptr::addr_of!(BIN_BUF) as *const u8;
            core::ptr::copy_nonoverlapping(src, dst, bin_size);
        }

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
