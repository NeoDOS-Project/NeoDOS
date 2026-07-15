use uefi::Handle;
use core::fmt::Write;

pub fn load_kernel_binary<BS>(
    _boot_services: &BS,
    _image_handle: Handle,
    mut stdout: impl Write,
) -> uefi::Result<*const u8>
where
    BS: ?Sized + core::fmt::Debug,
{
    // TODO(bootloader): Implement file loading for uefi 0.37 — currently returns dummy address
    let _ = writeln!(stdout, "[*] File loading not yet implemented\n");
    
    // For now, return a dummy address
    Ok(0x200000 as *const u8)
}