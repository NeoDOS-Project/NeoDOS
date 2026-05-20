# NEM — NeoDOS Test Driver Format (v1)

## Overview

NEM is a minimal binary format for test drivers in the NeoDOS validation ecosystem.

Test drivers validate:
- NeoFS metadata system (permissions + timestamps)
- NEM binary format correctness
- Driver lifecycle management
- System stability under repeated operations

NEM is NOT a production driver format. It exists solely for infrastructure validation.

## Binary Layout

```
Offset  Size  Field          Description
─────────────────────────────────────
  0      4    magic           "NEM\0" (0x004D454E)
  4      4    version         Format version = 1
  8      1    driver_type     0=null, 1=echo, 2=lifecycle,
                              3=mutation, 4=fault, 5=burst
  9      1    header_size     Must be 32
 10      2    entry_offset    Offset from file start to entry point
 12      4    code_offset     Offset from file start to code section
 16      4    code_size       Size of code section in bytes
 20      2    api_version     ABI version = 1
 22      2    compat_flags    Bit 0: requires_fs
 24      8    name            NUL-patted driver name, ASCII
 32      *    code_section    Raw x86-64 machine code
```

Total header: 32 bytes.
Entry point is at `entry_offset` from file start, always within code section.

## Driver Types

| Type | Value | Description |
|------|-------|-------------|
| Null | 0 | Minimal lifecycle — init + return |
| Echo | 1 | Control flow — echoes state |
| Lifecycle | 2 | Stress load/unload cycles |
| Mutation | 3 | Metadata read/write validation |
| Fault | 4 | Controlled failure injection |
| Burst | 5 | Rapid load/unload bursts |

## Driver Lifecycle

All drivers follow:
1. LOAD  — parse NEM header, validate format
2. INIT  — jump to entry point
3. RUN   — execute (optional)
4. UNLOAD — cleanup

## Permissions Model

Test drivers declare minimal permissions via `compat_flags`:
- No hardware access (no IO, no IRQ, no memory mapping)
- READ_METADATA always allowed
- WRITE_METADATA only for mutation type

## Filesystem Layout

```
/System/drivers/test/
  null.nem        — Null driver (minimal lifecycle)
  echo.nem        — Echo driver (control flow)
  stress_lifecycle.nem  — Lifecycle stress driver
  mutation.nem    — Metadata mutation driver
  fault.nem       — Fault injection driver
  burst.nem       — Load/unload burst driver
  invalid_magic.nem     — Corrupted NEM (bad magic)
  invalid_header.nem    — Corrupted NEM (bad header)
```

## Execution Model

1. .nem binaries loaded via the LOAD command or test harness
2. Code runs in Ring 3 (user mode) via flat binary execution
3. NEM header validation occurs before execution
4. Results logged via sys_write (stdout)
5. Test harness monitors exit code via sys_exit

## Failure Detection

Failures detected by:
- Invalid NEM magic → loader rejects
- Invalid header fields → loader rejects
- Process crash (GPF, page fault) → kernel panic classification
- Wrong exit code → harness detects
- Metadata corruption → NeoFS validation tests
