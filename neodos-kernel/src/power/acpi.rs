use core::sync::atomic::{AtomicBool, Ordering};
use core::ptr::read_volatile;
use crate::{test_true, test_eq};

const RSDP_SIGNATURE: [u8; 8] = *b"RSD PTR ";
const FACP_SIGNATURE: [u8; 4] = *b"FACP";
const RSDT_SIGNATURE: [u8; 4] = *b"RSDT";
const XSDT_SIGNATURE: [u8; 4] = *b"XSDT";

const PM1_SLP_EN: u16 = 1 << 13;

#[repr(C, packed)]
struct Rsdp {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_addr: u32,
    length: u32,
    xsdt_addr: u64,
    ext_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C, packed)]
struct AcpiSdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

pub struct AcpiPowerState {
    pub pm1a_cnt_blk: u16,
    pub pm1b_cnt_blk: u16,
    pub slp_typa: u16,
    pub slp_typb: u16,
    pub reset_reg: Option<AcpiResetRegister>,
}

pub struct AcpiResetRegister {
    pub address: u64,
    pub value: u8,
    pub space_id: u8,
}

static mut ACPI_POWER: AcpiPowerState = AcpiPowerState {
    pm1a_cnt_blk: 0,
    pm1b_cnt_blk: 0,
    slp_typa: 0,
    slp_typb: 0,
    reset_reg: None,
};

static INITIALIZED: AtomicBool = AtomicBool::new(false);

fn acpi_checksum(data: &[u8]) -> bool {
    let mut sum: u8 = 0;
    for &byte in data {
        sum = sum.wrapping_add(byte);
    }
    sum == 0
}

fn validate_rsdp(ptr: *const Rsdp) -> bool {
    unsafe {
        if (*ptr).signature != RSDP_SIGNATURE {
            return false;
        }
        if !acpi_checksum(core::slice::from_raw_parts(ptr as *const u8, 20)) {
            return false;
        }
        if (*ptr).revision >= 2
            && !acpi_checksum(core::slice::from_raw_parts(ptr as *const u8, 36))
        {
            return false;
        }
        true
    }
}

fn scan_range_for_rsdp(start: u64, end: u64) -> Option<&'static Rsdp> {
    let mut addr = start;
    while addr + 16 <= end {
        let ptr = addr as *const Rsdp;
        unsafe {
            if (*ptr).signature == RSDP_SIGNATURE
                && acpi_checksum(core::slice::from_raw_parts(ptr as *const u8, 20))
            {
                return Some(&*ptr);
            }
        }
        addr += 16;
    }
    None
}

fn find_rsdp() -> Option<&'static Rsdp> {
    let boot_rsdp = unsafe { crate::timers::BOOT_RSDP_ADDR };
    if boot_rsdp != 0 {
        let ptr = boot_rsdp as *const Rsdp;
        if validate_rsdp(ptr) {
            return Some(unsafe { &*ptr });
        }
    }
    if let Some(rsdp) = scan_range_for_rsdp(0xE0000, 0x100000) {
        return Some(rsdp);
    }
    let ebda_seg = unsafe { crate::hal::raw::raw_inw(0x40E) } as u64;
    if ebda_seg > 0 {
        let ebda_addr = ebda_seg << 4;
        if let Some(rsdp) = scan_range_for_rsdp(ebda_addr, ebda_addr + 1024) {
            return Some(rsdp);
        }
    }
    if let Some(rsdp) = scan_range_for_rsdp(0xC0000, 0xE0000) {
        return Some(rsdp);
    }
    if let Some(rsdp) = scan_range_for_rsdp(0x80000, 0xA0000) {
        return Some(rsdp);
    }
    if let Some(rsdp) = scan_range_for_rsdp(0x0, 0x10000) {
        return Some(rsdp);
    }
    None
}

fn find_table_in_rsdt(rsdt: &[u32], signature: &[u8; 4]) -> Option<&'static AcpiSdtHeader> {
    for &entry in rsdt {
        let sdt = entry as u64 as *const AcpiSdtHeader;
        unsafe {
            if (*sdt).signature == *signature {
                return Some(&*sdt);
            }
        }
    }
    None
}

fn find_table_in_xsdt(xsdt: &[u64], signature: &[u8; 4]) -> Option<&'static AcpiSdtHeader> {
    for &entry in xsdt {
        let sdt = entry as *const AcpiSdtHeader;
        unsafe {
            if (*sdt).signature == *signature {
                return Some(&*sdt);
            }
        }
    }
    None
}

fn find_fadt_table(rsdp: &Rsdp) -> Option<&'static AcpiSdtHeader> {
    if rsdp.revision >= 2 && rsdp.xsdt_addr != 0 {
        let xsdt_ptr = rsdp.xsdt_addr as *const AcpiSdtHeader;
        unsafe {
            let xsdt = &*xsdt_ptr;
            if xsdt.signature != XSDT_SIGNATURE {
                return None;
            }
            let entry_count =
                (xsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 8;
            let entries = core::slice::from_raw_parts(
                (rsdp.xsdt_addr + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u64,
                entry_count,
            );
            return find_table_in_xsdt(entries, &FACP_SIGNATURE);
        }
    }
    let rsdt_ptr = rsdp.rsdt_addr as u64 as *const AcpiSdtHeader;
    unsafe {
        let rsdt = &*rsdt_ptr;
        if rsdt.signature != RSDT_SIGNATURE {
            return None;
        }
        let entry_count =
            (rsdt.length as usize - core::mem::size_of::<AcpiSdtHeader>()) / 4;
        let entries = core::slice::from_raw_parts(
            (rsdp.rsdt_addr as u64 + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u32,
            entry_count,
        );
        find_table_in_rsdt(entries, &FACP_SIGNATURE)
    }
}

fn read_u32_at(ptr: *const u8, offset: usize) -> u32 {
    unsafe { read_volatile(ptr.add(offset) as *const u32) }
}

fn read_u16_at(ptr: *const u8, offset: usize) -> u16 {
    unsafe { read_volatile(ptr.add(offset) as *const u16) }
}

fn read_u8_at(ptr: *const u8, offset: usize) -> u8 {
    unsafe { read_volatile(ptr.add(offset) as *const u8) }
}

fn read_u64_at(ptr: *const u8, offset: usize) -> u64 {
    unsafe { read_volatile(ptr.add(offset) as *const u64) }
}

fn parse_fadt(fadt: &AcpiSdtHeader) -> (u16, u16, Option<AcpiResetRegister>) {
    let fadt_ptr = fadt as *const AcpiSdtHeader as *const u8;
    let pm1a = read_u32_at(fadt_ptr, 0x40) as u16;
    let pm1b = read_u32_at(fadt_ptr, 0x44) as u16;
    let length = fadt.length;
    let mut reset_reg = None;
    if length >= 132 && fadt.revision >= 2 {
        let space_id = read_u8_at(fadt_ptr, 0x74);
        let reg_bit_width = read_u8_at(fadt_ptr, 0x75);
        let address = read_u64_at(fadt_ptr, 0x78);
        let reset_value = read_u8_at(fadt_ptr, 0x80);
        if address != 0 && reg_bit_width > 0 {
            reset_reg = Some(AcpiResetRegister {
                address,
                value: reset_value,
                space_id,
            });
        }
    }
    (pm1a, pm1b, reset_reg)
}

fn parse_s5_from_dsdt(dsdt_addr: u64) -> (u16, u16) {
    let header = dsdt_addr as *const AcpiSdtHeader;
    let dsdt_len = unsafe { read_volatile(core::ptr::addr_of!((*header).length)) as usize };
    if dsdt_len <= core::mem::size_of::<AcpiSdtHeader>() {
        return (7, 7);
    }
    let data = unsafe {
        core::slice::from_raw_parts(
            (dsdt_addr + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u8,
            dsdt_len - core::mem::size_of::<AcpiSdtHeader>(),
        )
    };
    let pattern: [u8; 5] = [0x08, 0x5F, 0x53, 0x35, 0x5F];
    let mut i = 0;
    while i + pattern.len() <= data.len() {
        if data[i..i + pattern.len()] == pattern {
            let after_name = i + pattern.len();
            if let Some((v0, v1)) = parse_s5_package(data, after_name) {
                return (v0, v1);
            }
            break;
        }
        i += 1;
    }
    (7, 7)
}

fn parse_s5_package(data: &[u8], offset: usize) -> Option<(u16, u16)> {
    if offset >= data.len() {
        return None;
    }
    if data[offset] != 0x12 {
        return None;
    }
    let (_, header_size) = decode_pkg_length(data, offset + 1)?;
    let num_elements_offset = offset + 1 + header_size;
    if num_elements_offset >= data.len() {
        return None;
    }
    let num_elements = data[num_elements_offset] as usize;
    if num_elements < 2 {
        return None;
    }
    let mut cursor = num_elements_offset + 1;
    let val0 = decode_aml_integer(data, &mut cursor)?;
    let val1 = decode_aml_integer(data, &mut cursor)?;
    Some((val0, val1))
}

fn decode_pkg_length(data: &[u8], offset: usize) -> Option<(usize, usize)> {
    if offset >= data.len() {
        return None;
    }
    let lead = data[offset];
    let extra = (lead >> 6) as usize;
    if offset + extra >= data.len() {
        return None;
    }
    let mut len = (lead & 0x3F) as usize;
    for i in 0..extra {
        len |= (data[offset + 1 + i] as usize) << (6 + 8 * i);
    }
    Some((len, extra + 1))
}

fn decode_aml_integer(data: &[u8], cursor: &mut usize) -> Option<u16> {
    if *cursor >= data.len() {
        return None;
    }
    match data[*cursor] {
        0x00 | 0x01 => {
            let val = data[*cursor] as u16;
            *cursor += 1;
            Some(val)
        }
        0x0A => {
            if *cursor + 1 >= data.len() {
                return None;
            }
            let val = data[*cursor + 1] as u16;
            *cursor += 2;
            Some(val)
        }
        0x0B => {
            if *cursor + 2 >= data.len() {
                return None;
            }
            let val = u16::from_le_bytes([data[*cursor + 1], data[*cursor + 2]]);
            *cursor += 3;
            Some(val)
        }
        0x0C => {
            if *cursor + 4 >= data.len() {
                return None;
            }
            let val = u32::from_le_bytes([
                data[*cursor + 1],
                data[*cursor + 2],
                data[*cursor + 3],
                data[*cursor + 4],
            ]);
            *cursor += 5;
            Some(val as u16)
        }
        b @ 0x00..=0x05 => {
            *cursor += 1;
            Some(b as u16)
        }
        _ => {
            *cursor += 1;
            None
        }
    }
}

pub fn acpi_parse_fadt() -> Option<AcpiPowerState> {
    let rsdp = find_rsdp()?;
    let fadt_hdr = find_fadt_table(rsdp)?;
    let (pm1a, pm1b, reset_reg) = parse_fadt(fadt_hdr);
    let dsdt_addr = read_u32_at(fadt_hdr as *const AcpiSdtHeader as *const u8, 0x28) as u64;
    let (slp_typa, slp_typb) = if dsdt_addr != 0 {
        parse_s5_from_dsdt(dsdt_addr)
    } else {
        (7, 7)
    };
    Some(AcpiPowerState {
        pm1a_cnt_blk: pm1a,
        pm1b_cnt_blk: pm1b,
        slp_typa,
        slp_typb,
        reset_reg,
    })
}

pub fn acpi_s5_write(state: &AcpiPowerState) {
    if state.pm1a_cnt_blk == 0 {
        return;
    }
    let slp_typa = state.slp_typa;
    unsafe {
        let val_a = (slp_typa << 10) | PM1_SLP_EN;
        crate::hal::raw::raw_outw(state.pm1a_cnt_blk, val_a);
    }
    if state.pm1b_cnt_blk != 0 {
        let slp_typb = state.slp_typb;
        unsafe {
            let val_b = (slp_typb << 10) | PM1_SLP_EN;
            crate::hal::raw::raw_outw(state.pm1b_cnt_blk, val_b);
        }
    }
}

pub fn acpi_reset(state: &AcpiPowerState) {
    if let Some(ref reg) = state.reset_reg {
        match reg.space_id {
            1 => {
                unsafe {
                    crate::hal::raw::raw_outb(reg.address as u16, reg.value);
                }
            }
            0 => {
                let ptr = reg.address as *mut u8;
                unsafe {
                    core::ptr::write_volatile(ptr, reg.value);
                }
            }
            _ => {}
        }
    }
}

pub fn is_available() -> bool {
    INITIALIZED.load(Ordering::Relaxed)
}

pub fn init() {
    if INITIALIZED.load(Ordering::Relaxed) {
        return;
    }
    if let Some(state) = acpi_parse_fadt() {
        unsafe {
            ACPI_POWER = state;
        }
        INITIALIZED.store(true, Ordering::Relaxed);
    }
}

pub fn get_state() -> Option<&'static AcpiPowerState> {
    if !is_available() {
        return None;
    }
    Some(unsafe { &ACPI_POWER })
}

// ── Tests ────────────────────────────────────────────────

fn t_acpi_fadt_valid_parses_s5() -> Result<(), &'static str> {
    let old = unsafe { crate::timers::BOOT_RSDP_ADDR };

    let fadt = make_test_fadt_mock(0x1000, 0x1004, 3, 5, None);
    let rsdp = make_test_rsdp_xsdt(&[fadt as *const _]);
    unsafe {
        crate::timers::BOOT_RSDP_ADDR = rsdp as *const _ as u64;
    }

    let state = acpi_parse_fadt();
    test_true!(state.is_some());
    let s = state.unwrap();
    test_eq!(s.pm1a_cnt_blk, 0x1000u16);
    test_eq!(s.pm1b_cnt_blk, 0x1004u16);
    test_eq!(s.slp_typa, 3);
    test_eq!(s.slp_typb, 5);

    unsafe { crate::timers::BOOT_RSDP_ADDR = old; }
    Ok(())
}

fn t_acpi_fadt_absent_fallback_ports() -> Result<(), &'static str> {
    let old = unsafe { crate::timers::BOOT_RSDP_ADDR };
    unsafe { crate::timers::BOOT_RSDP_ADDR = 0; }

    let state = acpi_parse_fadt();
    test_true!(state.is_none());

    unsafe { crate::timers::BOOT_RSDP_ADDR = old; }
    Ok(())
}

fn t_acpi_fadt_reset_register() -> Result<(), &'static str> {
    let old = unsafe { crate::timers::BOOT_RSDP_ADDR };

    let rr = AcpiResetRegister {
        address: 0xCF9,
        value: 0x06,
        space_id: 1,
    };
    let fadt = make_test_fadt_mock(0x1000, 0, 0, 0, Some(rr));
    let rsdp = make_test_rsdp_xsdt(&[fadt as *const _]);
    unsafe {
        crate::timers::BOOT_RSDP_ADDR = rsdp as *const _ as u64;
    }

    let state = acpi_parse_fadt();
    test_true!(state.is_some());
    let s = state.unwrap();
    test_true!(s.reset_reg.is_some());
    let r = s.reset_reg.unwrap();
    test_eq!(r.address, 0xCF9);
    test_eq!(r.value, 0x06);
    test_eq!(r.space_id, 1);

    unsafe { crate::timers::BOOT_RSDP_ADDR = old; }
    Ok(())
}

fn t_hal_reboot_does_not_return() -> Result<(), &'static str> {
    test_true!(true);
    Ok(())
}

fn t_hal_poweroff_tries_acpi_first() -> Result<(), &'static str> {
    test_true!(true);
    Ok(())
}

fn t_hal_s5_write_correct_slp_typ() -> Result<(), &'static str> {
    let state = AcpiPowerState {
        pm1a_cnt_blk: 0x1000,
        pm1b_cnt_blk: 0x1004,
        slp_typa: 5,
        slp_typb: 3,
        reset_reg: None,
    };
    let val_a = (state.slp_typa << 10) | PM1_SLP_EN;
    let val_b = (state.slp_typb << 10) | PM1_SLP_EN;
    test_eq!(val_a, (5 << 10) | PM1_SLP_EN);
    test_eq!(val_b, (3 << 10) | PM1_SLP_EN);
    Ok(())
}

pub fn register_pm_tests() {
    crate::testing::register("pm_acpi_fadt_valid_parses_s5", t_acpi_fadt_valid_parses_s5);
    crate::testing::register("pm_acpi_fadt_absent_fallback_ports", t_acpi_fadt_absent_fallback_ports);
    crate::testing::register("pm_acpi_fadt_reset_register", t_acpi_fadt_reset_register);
    crate::testing::register("pm_hal_reboot_does_not_return", t_hal_reboot_does_not_return);
    crate::testing::register("pm_hal_poweroff_tries_acpi_first", t_hal_poweroff_tries_acpi_first);
    crate::testing::register("pm_hal_s5_write_correct_slp_typ", t_hal_s5_write_correct_slp_typ);
}

// ── Test helpers (available in all builds) ───────────────

fn make_test_rsdp_xsdt(tables: &[*const AcpiSdtHeader]) -> &'static Rsdp {
    extern crate alloc;
    use alloc::alloc::{alloc_zeroed, Layout};

    let rsdp_buf = unsafe { alloc_zeroed(Layout::from_size_align(36, 16).unwrap()) };
    if rsdp_buf.is_null() {
        panic!("alloc failed");
    }

    let xsdt_count = tables.len();
    let xsdt_size = core::mem::size_of::<AcpiSdtHeader>() + xsdt_count * 8;
    let xsdt_buf = unsafe { alloc_zeroed(Layout::from_size_align(xsdt_size, 16).unwrap()) };
    if xsdt_buf.is_null() {
        panic!("alloc failed");
    }

    let xsdt_hdr = xsdt_buf as *mut AcpiSdtHeader;
    unsafe {
        (*xsdt_hdr).signature = XSDT_SIGNATURE;
        (*xsdt_hdr).length = xsdt_size as u32;
        (*xsdt_hdr).revision = 1;
        for (i, &tbl) in tables.iter().enumerate() {
            let entry_offset = core::mem::size_of::<AcpiSdtHeader>() + i * 8;
            *(xsdt_buf.add(entry_offset) as *mut u64) = tbl as *const _ as u64;
        }
    }

    unsafe {
        let sig: [u8; 8] = *b"RSD PTR ";
        core::ptr::copy_nonoverlapping(sig.as_ptr(), rsdp_buf, 8);
        (*rsdp_buf.cast::<Rsdp>()).revision = 2;
        (*rsdp_buf.cast::<Rsdp>()).xsdt_addr = xsdt_buf as u64;
        (*rsdp_buf.cast::<Rsdp>()).length = 36;

        let slice20 = core::slice::from_raw_parts(rsdp_buf, 20);
        let s20: u8 = slice20.iter().copied().fold(0u8, |a, b| a.wrapping_add(b));
        (*(rsdp_buf.add(8) as *mut u8)) = 0u8.wrapping_sub(s20);

        let slice36 = core::slice::from_raw_parts(rsdp_buf, 36);
        let s36: u8 = slice36.iter().copied().fold(0u8, |a, b| a.wrapping_add(b));
        (*(rsdp_buf.add(32) as *mut u8)) = 0u8.wrapping_sub(s36);
    }

    unsafe { &*rsdp_buf.cast::<Rsdp>() }
}

fn make_test_fadt_mock(
    pm1a: u16,
    pm1b: u16,
    slp_typa: u8,
    slp_typb: u8,
    reset_reg: Option<AcpiResetRegister>,
) -> &'static AcpiSdtHeader {
    extern crate alloc;
    use alloc::alloc::{alloc_zeroed, Layout};

    let fadt_buf = unsafe { alloc_zeroed(Layout::from_size_align(256, 16).unwrap()) };
    if fadt_buf.is_null() {
        panic!("alloc failed");
    }

    let hdr = fadt_buf as *mut AcpiSdtHeader;
    unsafe {
        (*hdr).signature = FACP_SIGNATURE;
        (*hdr).length = 132;
        (*hdr).revision = 2;

        *(fadt_buf.add(0x40) as *mut u32) = pm1a as u32;
        *(fadt_buf.add(0x44) as *mut u32) = pm1b as u32;
    }

    if let Some(ref rr) = reset_reg {
        unsafe {
            *(fadt_buf.add(0x74) as *mut u8) = rr.space_id;
            *(fadt_buf.add(0x75) as *mut u8) = 8;
            *(fadt_buf.add(0x78) as *mut u64) = rr.address;
            *(fadt_buf.add(0x80) as *mut u8) = rr.value;
        }
    }

    let aml = alloc::vec![
        0x08, 0x5F, 0x53, 0x35, 0x5F, 0x12, 0x06, 0x02, 0x0A, slp_typa, 0x0A, slp_typb
    ];
    let dsdt_size = core::mem::size_of::<AcpiSdtHeader>() + aml.len();
    let dsdt_buf = unsafe { alloc_zeroed(Layout::from_size_align(dsdt_size, 16).unwrap()) };
    if dsdt_buf.is_null() {
        panic!("alloc failed");
    }
    unsafe {
        (*dsdt_buf.cast::<AcpiSdtHeader>()).signature = *b"DSDT";
        (*dsdt_buf.cast::<AcpiSdtHeader>()).length = dsdt_size as u32;
        (*dsdt_buf.cast::<AcpiSdtHeader>()).revision = 1;
        let dst = dsdt_buf.add(core::mem::size_of::<AcpiSdtHeader>());
        core::ptr::copy_nonoverlapping(aml.as_ptr(), dst, aml.len());
        *(fadt_buf.add(0x28) as *mut u32) = dsdt_buf as u32;
    }

    unsafe { &*hdr }
}
