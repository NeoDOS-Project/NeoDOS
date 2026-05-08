# NeoDOS Phase 1

Phase 1 focuses on establishing a working boot pipeline and a minimally useful kernel environment.

## Done (Current State)

- UEFI bootloader that:
  - initializes GOP framebuffer
  - loads `kernel.elf`
  - calls `ExitBootServices` and passes the final UEFI memory map to the kernel via `BootInfo`
- Kernel that boots reliably and provides:
  - serial logging + framebuffer renderer + VGA fallback
  - GDT / IDT / PIC initialization
  - ATA + block cache + NeoDOS FS mount
  - custom page tables (currently 4 GiB identity map)
  - DOS-like shell with basic commands
  - `CPUINFO` (CPUID vendor/brand) and `MEM` (memory stats)

## How to Run

```bash
cd neodos
bash scripts/build.sh
bash scripts/qemu-debug.sh
```

See `docs/DEBUG.md` for GDB/monitor usage.

