use core::fmt;

pub struct SerialPort {
    port: u16,
}

impl SerialPort {
    pub const fn new(port: u16) -> Self {
        SerialPort { port }
    }

    pub unsafe fn init(&self) {
        self.write_reg(1, 0x00);    // Disable interrupts
        self.write_reg(3, 0x80);    // Enable DLAB (set baud rate divisor)
        self.write_reg(0, 0x03);    // Set divisor to 3 (38400 baud)
        self.write_reg(1, 0x00);    // (hi byte)
        self.write_reg(3, 0x03);    // 8 bits, no parity, one stop bit
        self.write_reg(2, 0x06);    // Disable FIFO, clear TX+RX (no buffering)
        self.write_reg(4, 0x0B);    // IRQs enabled, RTS/DSR set
    }

    unsafe fn write_reg(&self, reg: u16, value: u8) {
        crate::hal::outb(self.port + reg, value);
    }

    unsafe fn read_reg(&self, reg: u16) -> u8 {
        crate::hal::inb(self.port + reg)
    }

    fn line_status(&self) -> u8 {
        unsafe { self.read_reg(5) }
    }

    pub fn send(&self, data: u8) {
        while (self.line_status() & 0x20) == 0 {}
        crate::hal::outb(self.port, data);
    }

    pub fn flush(&self) {
        // Wait until transmitter is completely empty
        while (self.line_status() & 0x40) == 0 {}
    }
    
}

impl fmt::Write for SerialPort {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        // Ensure all bytes are transmitted before releasing the lock
        self.flush();
        Ok(())
    }
}

use spin::Mutex;
use lazy_static::lazy_static;

lazy_static! {
    pub static ref SERIAL1: Mutex<SerialPort> = Mutex::new(SerialPort::new(0x3F8));
}

pub fn init() {
    unsafe {
        SERIAL1.lock().init();
    }
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let _ = SERIAL1.lock().write_fmt(args);
}

/// Lock-free raw serial writer for fault/panic paths.
///
/// `SERIAL1` is a non-reentrant `spin::Mutex`: if a CPU faults while it still
/// holds the port (for example inside the exception logger itself), the fault
/// handler would spin forever on the same lock and hide the original fault.
/// This writer never touches `SERIAL1`; it writes straight to the UART, so a
/// fault can always be reported.
struct RawSerial;

impl fmt::Write for RawSerial {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let port = SerialPort::new(0x3F8);
        for byte in s.bytes() {
            port.send(byte);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _raw_print(args: fmt::Arguments) {
    use core::fmt::Write;
    use core::sync::atomic::{AtomicBool, Ordering};
    // Best-effort serialization: keeps concurrent fault reports from tearing
    // each other, but never blocks forever if a CPU dies holding the flag.
    static RAW_BUSY: AtomicBool = AtomicBool::new(false);
    let mut spins = 0u32;
    while RAW_BUSY
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        spins += 1;
        if spins > 2_000_000 {
            break;
        }
        core::hint::spin_loop();
    }
    let _ = RawSerial.write_fmt(args);
    RAW_BUSY.store(false, Ordering::Release);
}

#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => ($crate::arch::x64::serial::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)));
}

/// Lock-free serial print. Safe to call from exception/panic handlers even if
/// `SERIAL1` is already held by this or another CPU.
#[macro_export]
macro_rules! raw_serial_print {
    ($($arg:tt)*) => ($crate::arch::x64::serial::_raw_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! raw_serial_println {
    () => ($crate::raw_serial_print!("\n"));
    ($($arg:tt)*) => ($crate::raw_serial_print!("{}\n", format_args!($($arg)*)));
}
