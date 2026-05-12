// src/input.rs
//
// Lock-free single-producer (IRQ1) / single-consumer (shell) input buffer.
// The producer runs with interrupts disabled (hardware), so the consumer
// masks interrupts around its critical section to prevent the producer
// from observing an inconsistent state.

const INPUT_BUFFER_SIZE: usize = 1024;

pub struct InputBuffer {
    buffer: [u8; INPUT_BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl InputBuffer {
    pub const fn new() -> Self {
        InputBuffer {
            buffer: [0; INPUT_BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }

    pub fn push(&mut self, byte: u8) -> Result<(), ()> {
        let next = (self.tail + 1) % INPUT_BUFFER_SIZE;
        if next == self.head {
            return Err(());
        }
        self.buffer[self.tail] = byte;
        self.tail = next;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            return None;
        }
        let byte = self.buffer[self.head];
        self.head = (self.head + 1) % INPUT_BUFFER_SIZE;
        Some(byte)
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }
}

static mut INPUT_BUFFER: InputBuffer = InputBuffer::new();

/// Called from IRQ1 context (keyboard handler).
/// Interrupts are already disabled by the CPU, so it is safe to mutate
/// INPUT_BUFFER without additional synchronisation.
pub fn push_byte(byte: u8) {
    unsafe {
        let _ = INPUT_BUFFER.push(byte);
    }
}

/// Called from the main shell loop.  Disables interrupts so the keyboard
/// IRQ cannot run concurrently and see a partially-updated head pointer.
pub fn pop_byte() -> Option<u8> {
    let mut if_set = false;
    unsafe {
        let mut flags: u64;
        core::arch::asm!("pushfq; pop {}", lateout(reg) flags);
        if_set = (flags & 0x200) != 0;
        core::arch::asm!("cli");
        
        if let Some(byte) = INPUT_BUFFER.pop() {
            if if_set { core::arch::asm!("sti"); }
            return Some(byte);
        }
        
        let lsr: u8;
        core::arch::asm!(
            "in al, dx",
            out("al") lsr,
            in("dx") 0x3F8 + 5,
            options(nomem, nostack, preserves_flags)
        );
        if lsr & 1 != 0 {
            let byte: u8;
            core::arch::asm!(
                "in al, dx",
                out("al") byte,
                in("dx") 0x3F8,
                options(nomem, nostack, preserves_flags)
            );
            if if_set { core::arch::asm!("sti"); }
            crate::serial_println!("[INPUT] serial read: 0x{:02X}", byte);
            return Some(byte);
        }
        
        if if_set { core::arch::asm!("sti"); }
        None
    }
}
