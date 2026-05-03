use core::fmt::Write;

pub fn print_memory_map<BS>(
    boot_services: &BS,
    mut stdout: impl Write,
) -> uefi::Result<()>
where
    BS: ?Sized + core::fmt::Debug,
{
    let _ = writeln!(stdout, "[*] Memory map support not yet implemented\n");
    Ok(())
}