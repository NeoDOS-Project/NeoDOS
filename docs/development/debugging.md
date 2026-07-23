# Debugging NeoDOS

NeoDOS is typically debugged under QEMU using serial output, the QEMU monitor, and GDB.

## Run QEMU

From `neodos/`:

```bash
bash scripts/build.sh
bash scripts/qemu-debug.sh
```

This writes a full transcript to `neodos/qemu_output.log`.

## QEMU Monitor

The launch script enables the monitor on:

```text
telnet 127.0.0.1 4444
```

Useful commands:

```text
info registers
x /16x 0x4000000
info mem
```

## GDB (remote)

The launch script enables QEMU’s GDB server on:

```text
target remote localhost:1234
```

Example session:

```gdb
(gdb) target remote localhost:1234
(gdb) break *0x4000000
(gdb) continue
```

## Tips

- If you don’t hit a breakpoint at `0x4000000`, confirm the kernel entry printed by the bootloader and that you rebuilt the disk image.
- If the shell boots but reboots later, check the serial log (`qemu_output.log`) first; it usually contains the panic location.
