// src/shell/commands/run.rs
//
// RUN <filename>
//
// Loads a flat binary from NeoDOS FS into the user-accessible memory window
// (USER_BASE) and transfers control to it via IRETQ (Ring 3).
//
// Binary format: raw flat binary loaded at 0x400000.
// The binary can issue INT 0x80 syscalls:
//   RAX=0              sys_exit(code)
//   RAX=1, RBX=ptr, RCX=len   sys_write(buf, len)
//   RAX=2              sys_yield
//   RAX=3              sys_getpid → RAX
//
// Stack: 64 KB allocated at USER_BASE + 64 KB, growing down.
//
// Limitations (v0.7):
//   - Max binary size: 64 KB (fits inside the 4 MB user window with room for stack)
//   - No ELF loading — plain flat binary only (nasm -f bin, or similar)
//   - Single-threaded foreground execution (the shell blocks until sys_exit)

use crate::println;
use crate::serial_println;
use crate::shell::shell::DosShell;
use crate::arch::x64::paging::{USER_BASE, USER_LIMIT};

/// Maximum size (bytes) of a user binary.
/// Must leave room for stack inside the user window.
const MAX_BIN_SIZE: usize = 64 * 1024; // 64 KB

/// Stack size allocated for the user process.
const USER_STACK_SIZE: u64 = 64 * 1024; // 64 KB

/// Entry point of the user binary (loaded at USER_BASE).
const USER_ENTRY: u64 = USER_BASE;

/// Top of the user stack: just above the binary, page-aligned down.
/// Layout inside the user window:
///   USER_BASE ─── binary code/data (up to MAX_BIN_SIZE)
///   USER_BASE + MAX_BIN_SIZE ─── stack grows downward (USER_STACK_SIZE)
const USER_STACK_TOP: u64 = USER_BASE + MAX_BIN_SIZE as u64 + USER_STACK_SIZE;

impl<'a> DosShell<'a> {
    pub fn cmd_run(&mut self, args: &[&str]) {
        if args.is_empty() {
            println!("Usage: RUN <filename>");
            println!("  Loads a flat binary from NeoDOS FS and executes it in Ring 3.");
            return;
        }

        let filename = args[0];

        // ── 1. Sanity-check the user window is inside our 4 GB identity map ──
        if USER_STACK_TOP > USER_LIMIT {
            println!("Error: User window too small for binary + stack.");
            println!("  USER_BASE=0x{:x}  USER_LIMIT=0x{:x}  stack_top=0x{:x}",
                USER_BASE, USER_LIMIT, USER_STACK_TOP);
            return;
        }

        // ── 2. Find and read the binary ──
        let inode = match self.resolve_file_inode(filename) {
            Ok(i) => i,
            Err(_) => {
                println!("File not found: {}", filename);
                return;
            }
        };

        // Temporary heap-sized buffer on the kernel stack would overflow; use
        // a static buffer instead (single-task system, no re-entrancy issue).
        static mut BIN_BUF: [u8; MAX_BIN_SIZE] = [0u8; MAX_BIN_SIZE];

        let bin_size = unsafe {
            // Use raw pointer to avoid the mutable-ref-to-mutable-static lint.
            let buf_ptr: *mut [u8; MAX_BIN_SIZE] = core::ptr::addr_of_mut!(BIN_BUF);
            (*buf_ptr).fill(0);
            match self.fs.read_file_to_buf(inode, &mut *buf_ptr, self.cache, self.ata) {
                Ok(n) => n,
                Err(e) => {
                    println!("Error reading '{}': {:?}", filename, e);
                    return;
                }
            }
        };

        if bin_size == 0 {
            println!("Error: '{}' is empty.", filename);
            return;
        }

        serial_println!("[RUN] '{}' -> {} bytes, loading to 0x{:x}", filename, bin_size, USER_ENTRY);

        // ── 3. Copy binary to user-accessible memory ──
        // USER_BASE..USER_LIMIT is marked USER_ACCESSIBLE during paging init.
        unsafe {
            let dst = USER_ENTRY as *mut u8;
            let src = core::ptr::addr_of!(BIN_BUF) as *const u8;
            core::ptr::copy_nonoverlapping(src, dst, bin_size);
        }

        serial_println!("[RUN] Binary copied. Entering Ring 3 @ 0x{:x}, RSP=0x{:x}",
            USER_ENTRY, USER_STACK_TOP);

        println!("Launching '{}' ({} bytes) in Ring 3...", filename, bin_size);

        // ── 4. Enter Ring 3 via IRETQ ──
        // execute_usermode does not return; the only exit path is sys_exit (INT 0x80
        // with RAX=0), which marks the scheduler slot as Terminated. Because we
        // are currently running in the shell (kernel task, not a scheduler slot),
        // the simplest approach for v0.7 is to spin-wait until the process exits.
        //
        // A proper implementation would add the process to the scheduler and block
        // the shell until a "child exited" event. That is left for v0.8.
        crate::usermode::execute_usermode(USER_ENTRY, USER_STACK_TOP);

        // execute_usermode is `options(noreturn)` — if we reach here something went
        // very wrong; the panic handler will halt the system.
        #[allow(unreachable_code)]
        {
            println!("Returned from Ring 3 (unexpected).");
        }
    }
}
