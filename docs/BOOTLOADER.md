# NeoDOS Bootloader

The NeoDOS bootloader is a UEFI application that loads the kernel ELF and transfers control to the kernel in long mode.

## Responsibilities

- Initialize UEFI services and logging
- Initialize GOP framebuffer and build a `FramebufferInfo`
- Read `EFI\\NeoDOS\\kernel.elf` from the ESP
- Parse the ELF and load `PT_LOAD` segments into memory
- Call `ExitBootServices` and capture the final UEFI memory map
- Call the kernel entry point as `extern "sysv64" fn(&BootInfo) -> !`

## BootInfo ABI

The kernel is entered using the System V AMD64 ABI:

- `RDI` = pointer to `BootInfo`

`BootInfo` includes:

- `fb_info`: framebuffer base/size/resolution
- `memory_map_*`: raw pointer + size + descriptor metadata for the UEFI memory map returned at ExitBootServices

Note: after `ExitBootServices` the UEFI pool allocator is no longer usable. The bootloader therefore *leaks* the memory map buffer (via `core::mem::forget`) so the kernel can read it.

## Files

- `neodos-bootloader/src/main.rs`: bootloader entry and ELF loader

## Notes / Limitations

- This bootloader is intentionally minimal and assumes QEMU/OVMF-style execution.
- ELF loading currently expects the kernel to be linked at the addresses used by the linker script (see `neodos-kernel/kernel.ld`).

