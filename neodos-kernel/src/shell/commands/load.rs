// src/shell/commands/load.rs
//
// LOAD <filename>
//
// Loads a driver module (.ndm or .bin) and executes it as a process
// that registers itself as a device handler.

use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;

const MAX_DRIVER_SIZE: usize = 64 * 1024;

impl<'a> DosShell<'a> {
    pub fn cmd_load(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: LOAD <filename>");
            println!("  Loads a driver module and registers it as device handler.");
            println!("  Example: LOAD DRIVER.BIN");
            return;
        }

        let filename = args[0];

        // Find and read the driver binary
        let inode = match self.resolve_file_inode(filename) {
            Ok(i) => i,
            Err(_) => {
                println!("File not found: {}", filename);
                return;
            }
        };

        static mut DRV_BUF: [u8; MAX_DRIVER_SIZE] = [0u8; MAX_DRIVER_SIZE];

        let drv_size = unsafe {
            let buf_ptr: *mut [u8; MAX_DRIVER_SIZE] = core::ptr::addr_of_mut!(DRV_BUF);
            (*buf_ptr).fill(0);
            match self.fs.read_file_to_buf(inode, &mut *buf_ptr, self.cache, self.ata) {
                Ok(n) => n,
                Err(e) => {
                    println!("Error reading '{}': {:?}", filename, e);
                    return;
                }
            }
        };

        if drv_size == 0 {
            println!("Error: '{}' is empty.", filename);
            return;
        }

        serial_println!("[LOAD] '{}' -> {} bytes", filename, drv_size);

        // Allocate a user slot for the driver
        let slot = match crate::arch::x64::paging::alloc_user_slot() {
            Some(s) => s,
            None => {
                println!("Error: No free user memory slots.");
                return;
            }
        };

        if slot.stack_top > crate::arch::x64::paging::USER_LIMIT {
            println!("Error: User slot exceeds memory window.");
            crate::arch::x64::paging::free_user_slot(slot.slot_idx);
            return;
        }

        // Copy driver to slot
        unsafe {
            let dst = slot.code_base as *mut u8;
            let src = DRV_BUF.as_ptr();
            core::ptr::copy_nonoverlapping(src, dst, drv_size);
        }

        // Spawn as user process
        let pid = crate::usermode::spawn_usermode(slot.code_base, slot.stack_top, slot.slot_idx);

        serial_println!("[LOAD] Spawned driver PID {}", pid);
        println!("Loading '{}' (PID {}) as driver...", filename, pid);

        // Wait for driver to register itself
        crate::usermode::wait_for_process(pid);
        crate::usermode::clear_wait_pid();
        
        println!("Driver '{}' loaded (PID {})", filename, pid);
    }
}