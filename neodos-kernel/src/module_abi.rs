use crate::drivers::block::BlockDevice;
use core::mem;

// ── ABI layout validation (compile-time assertions) ────────────────

/// Assert that a `#[repr(C)]` type has a specific size at compile time.
macro_rules! assert_layout_size {
    ($ty:ty, $expected:expr) => {
        const _: [(); $expected] = [(); mem::size_of::<$ty>()];
    };
}

assert_layout_size!(NdModuleHeader, 64);
assert_layout_size!(KernelServiceTableV1, 168); // 8 (magic+version) + 12*8 (ptrs) + 8*8 (reserved)

// ── NDM header (v1) ──────────────────────────────────────────────────

pub const NDM_MAGIC: u32 = 0x004D444E;
pub const NDM_ABI_VERSION: u32 = 1;
pub const NDM_HEADER_SIZE: u16 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleType {
    Driver = 0,
    FileSystem = 1,
    ShellExtension = 2,
}

impl ModuleType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(ModuleType::Driver),
            1 => Some(ModuleType::FileSystem),
            2 => Some(ModuleType::ShellExtension),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            ModuleType::Driver => "driver",
            ModuleType::FileSystem => "filesystem",
            ModuleType::ShellExtension => "shell extension",
        }
    }
}

/// NDM file header (64 bytes).
/// Every field is naturally aligned in C layout so #[repr(C)] suffices
/// (no padding is added and no `packed` is needed).
#[repr(C)]
#[derive(Clone, Copy)]
pub struct NdModuleHeader {
    magic: u32,
    version: u32,
    module_type: u8,
    reserved1: u8,
    header_size: u16,
    entry_offset: u32,
    code_offset: u32,
    code_size: u32,
    data_offset: u32,
    data_size: u32,
    api_version: u32,
    _reserved2: u32,
    name: [u8; 16],
    compat_flags: u8,
    _padding: [u8; 7],
}

unsafe impl Send for NdModuleHeader {}

/// Result of a successful NDM header parse.
pub struct ParsedModule<'a> {
    pub module_type: ModuleType,
    pub code_slice: &'a [u8],
    pub data_slice: &'a [u8],
    pub entry_point_offset: u32,
    pub code_file_offset: u32,
    pub name: &'a str,
}

impl NdModuleHeader {
    pub fn from_bytes(data: &[u8]) -> Option<ParsedModule<'_>> {
        if data.len() < NDM_HEADER_SIZE as usize {
            return None;
        }

        let header: &NdModuleHeader = unsafe { &*(data.as_ptr() as *const NdModuleHeader) };

        if header.magic != NDM_MAGIC
            || header.version != NDM_ABI_VERSION
            || header.header_size != NDM_HEADER_SIZE
            || header.api_version != NDM_ABI_VERSION
        {
            return None;
        }
        if (header.header_size as usize) > data.len() {
            return None;
        }

        let module_type = ModuleType::from_u8(header.module_type)?;

        let code_end = (header.code_offset as usize).saturating_add(header.code_size as usize);
        let data_end = (header.data_offset as usize).saturating_add(header.data_size as usize);

        if code_end > data.len() || data_end > data.len() {
            return None;
        }

        if header.code_size > 0 && header.data_size > 0 {
            let c_start = header.code_offset as usize;
            let d_start = header.data_offset as usize;
            if c_start < data_end && d_start < code_end {
                return None;
            }
        }

        let effective_entry = if header.entry_offset == 0 {
            header.code_offset
        } else {
            header.entry_offset
        };
        if effective_entry < header.code_offset
            || effective_entry >= header.code_offset.saturating_add(header.code_size)
        {
            return None;
        }

        let name_len = header.name.iter().position(|&b| b == 0).unwrap_or(16);
        let name = core::str::from_utf8(&header.name[..name_len]).ok()?;
        if name.is_empty() {
            return None;
        }

        let code_slice = &data[header.code_offset as usize..code_end];
        let data_slice = &data[header.data_offset as usize..data_end];

        Some(ParsedModule {
            module_type,
            code_slice,
            data_slice,
            entry_point_offset: effective_entry,
            code_file_offset: header.code_offset,
            name,
        })
    }
}

// ── Kernel Service Table (Ring-0 TSR modules) ───────────────────────

pub const KERNEL_SERVICE_TABLE_ADDR: u64 = 0x4FFFF00;
pub const KERNEL_SERVICE_MAGIC: u32 = 0x4B535456; // "KSTV"

pub type FnPrintStr = extern "C" fn(ptr: *const u8);
pub type FnAllocFrame = extern "C" fn() -> u64;
pub type FnFreeFrame = extern "C" fn(phys: u64);
pub type FnInb = extern "C" fn(port: u16) -> u8;
pub type FnOutb = extern "C" fn(port: u16, val: u8);
pub type FnInw = extern "C" fn(port: u16) -> u16;
pub type FnOutw = extern "C" fn(port: u16, val: u16);
pub type FnInl = extern "C" fn(port: u16) -> u32;
pub type FnOutl = extern "C" fn(port: u16, val: u32);
pub type FnReadBlocks = extern "C" fn(lba: u64, count: u8, buf: *mut u8) -> i32;
pub type FnWriteBlocks = extern "C" fn(lba: u64, count: u8, buf: *const u8) -> i32;

#[repr(C)]
pub struct KernelServiceTableV1 {
    pub magic: u32,
    pub version: u32,
    pub print_str: Option<FnPrintStr>,
    pub serial_print: Option<FnPrintStr>,
    pub alloc_frame: Option<FnAllocFrame>,
    pub free_frame: Option<FnFreeFrame>,
    pub inb: Option<FnInb>,
    pub outb: Option<FnOutb>,
    pub inw: Option<FnInw>,
    pub outw: Option<FnOutw>,
    pub inl: Option<FnInl>,
    pub outl: Option<FnOutl>,
    pub read_blocks: Option<FnReadBlocks>,
    pub write_blocks: Option<FnWriteBlocks>,
    _reserved: [u64; 8],
}

pub fn init_kernel_service_table() {
    let ptr = KERNEL_SERVICE_TABLE_ADDR as *mut KernelServiceTableV1;
    unsafe {
        ptr.write(KernelServiceTableV1 {
            magic: KERNEL_SERVICE_MAGIC,
            version: 1,
            print_str: Some(tsr_console_print),
            serial_print: Some(tsr_serial_print),
            alloc_frame: Some(tsr_alloc_frame),
            free_frame: Some(tsr_free_frame),
            inb: Some(tsr_inb),
            outb: Some(tsr_outb),
            inw: Some(tsr_inw),
            outw: Some(tsr_outw),
            inl: Some(tsr_inl),
            outl: Some(tsr_outl),
            read_blocks: Some(tsr_read_blocks),
            write_blocks: Some(tsr_write_blocks),
            _reserved: [0u64; 8],
        });
    }
}

// ── TSR service helpers ─────────────────────────────────────────────

extern "C" fn tsr_console_print(ptr: *const u8) {
    let len = unsafe {
        let mut i = 0;
        while *ptr.add(i) != 0 {
            i += 1;
        }
        i
    };
    let s = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len)) };
    crate::console::print_str(s);
}

extern "C" fn tsr_serial_print(ptr: *const u8) {
    let port = crate::arch::x64::serial::SERIAL1.lock();
    unsafe {
        let mut i = 0;
        loop {
            let c = *ptr.add(i);
            if c == 0 {
                break;
            }
            port.send(c);
            i += 1;
        }
    }
}

extern "C" fn tsr_alloc_frame() -> u64 {
    crate::memory::allocate_frame().unwrap_or(0)
}

extern "C" fn tsr_free_frame(phys: u64) {
    crate::memory::free_frame(phys);
}

extern "C" fn tsr_inb(port: u16) -> u8 {
    let val: u8;
    unsafe { core::arch::asm!("in al, dx", out("al") val, in("dx") port) }
    val
}

extern "C" fn tsr_outb(port: u16, val: u8) {
    unsafe { core::arch::asm!("out dx, al", in("dx") port, in("al") val) }
}

extern "C" fn tsr_inw(port: u16) -> u16 {
    let val: u16;
    unsafe { core::arch::asm!("in ax, dx", out("ax") val, in("dx") port) }
    val
}

extern "C" fn tsr_outw(port: u16, val: u16) {
    unsafe { core::arch::asm!("out dx, ax", in("dx") port, in("ax") val) }
}

extern "C" fn tsr_inl(port: u16) -> u32 {
    let val: u32;
    unsafe { core::arch::asm!("in eax, dx", out("eax") val, in("dx") port) }
    val
}

extern "C" fn tsr_outl(port: u16, val: u32) {
    unsafe { core::arch::asm!("out dx, eax", in("dx") port, in("eax") val) }
}

extern "C" fn tsr_read_blocks(lba: u64, count: u8, buf: *mut u8) -> i32 {
    let slice = unsafe { core::slice::from_raw_parts_mut(buf, (count as usize) * 512) };
    let mut lock = crate::globals::ATA_DRIVER.try_lock();
    match lock.as_mut().and_then(|o| o.as_mut()) {
        Some(dev) => match dev.read_blocks(lba, count, slice) {
            Ok(()) => 0,
            Err(_) => -1,
        },
        None => -1,
    }
}

extern "C" fn tsr_write_blocks(lba: u64, count: u8, buf: *const u8) -> i32 {
    let slice = unsafe { core::slice::from_raw_parts(buf, (count as usize) * 512) };
    let mut lock = crate::globals::ATA_DRIVER.try_lock();
    match lock.as_mut().and_then(|o| o.as_mut()) {
        Some(dev) => match dev.write_blocks(lba, count, slice) {
            Ok(()) => 0,
            Err(_) => -1,
        },
        None => -1,
    }
}
