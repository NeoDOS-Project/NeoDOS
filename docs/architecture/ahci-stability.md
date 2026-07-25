# AHCI Stability Improvement: Multi-Sector Read Batching

## Issue

NeoDOS boot is **intermittently unstable** under QEMU TCG emulation due to the AHCI driver failing on back-to-back single-sector (512-byte) READ DMA commands. Under KVM or real hardware the driver works reliably, but TCG emulation exposes a race condition:

### Symptoms

- Boot hangs at `[INIT_DEBUG] before inode lookup` or `[INIT_DEBUG] before file read`
- No GPF, no BUGCHECK, no panic — the AHCI command never completes
- `PORT_CI` bit stays set after issuing the command (command never clears)
- No timeout fires within the 10M iteration spin loop (or fires after the system is already stuck)
- **Intermittent**: same binary, same QEMU arguments, sometimes boots fine (~1/3), sometimes hangs (~2/3)

### When it happens

The hang occurs specifically when loading `C:\Programs\neoinit.nxe` (17,552 bytes = ~35 sectors) via the FAT32 driver. Each sector is read individually:

```
FAT32::read_file_by_cluster()
  → for each sector in cluster:
      → read_sector(lba + i)        // 1 sector at a time
        → IoStack::read_sector()    // single-sector API
          → Cache miss → dev.read_blocks() → AHCI dma_xfer(lba, 1, ...)
```

This generates **35+ sequential AHCI commands** on the same port slot. Under QEMU TCG, the AHCI state machine can stall after several (typically 5-6) back-to-back commands when reusing the same command slot (`PxCI` bit 0).

## Root Cause

### QEMU AHCI emulation quirk

QEMU's AHCI controller (`hw/ide/ahci.c`) uses a single IDE bus state machine per port. When a command on slot N completes, the controller transitions the IDE bus from `BSY` to idle. If the next write to `PxCI` for the same slot arrives before the emulated IDE bus has fully released (reading the D2H FIS status, clearing `PxIS`, flushing PRDBC), the controller may not start the new command. The slot stays busy forever.

### Single-slot reuse

The current `dma_xfer()` always uses command slot 0:

```rust
port_write32(abar, port, PORT_CI, 1);    // bit 0 → slot 0
```

After the command completes (CI bit 0 clears), the next call reuses the same slot. QEMU's IDE state machine may not be ready for the new command by the time the kernel polls `PxCI`.

### No inter-command synchronization

Between commands, the AHCI driver:
- Does NOT read `PxIS` to clear pending interrupt status bits (write-1-to-clear)
- Does NOT re-read the completed command's PRDBC from the `CmdHeader`
- Does NOT verify the IDE bus is idle (`PxTFD` without `BSY`/`DRQ`) before issuing the next command

While these are not strictly required by the AHCI 1.3 spec (which allows polling `PxCI` as the only completion signal), QEMU's implementation relies on them to advance the internal state machine.

## Current Workarounds (already implemented)

Two changes reduce but do not eliminate the intermittent hang:

1. **Slot alternation**: `dma_xfer()` alternates between slot 0 and slot 1 (`next_slot + 1 % 2`), reducing the frequency of same-slot back-to-back commands.

2. **PxIS clear + PxTFD sync**: After each command completes, the driver acknowledges interrupt status and syncs device status.

3. **FAT32 multi-sector batching**: `read_file_by_cluster()` and `find_entry_in_directory()` now batch up to 8 consecutive sectors into a single `read_sectors()` call. This reduces AHCI commands for a 35-sector file read from 35 down to ~5 commands.

All workarounds are in `boot_ahci.rs:dma_xfer()` and `fat32.rs` read functions.

## Remaining Issue

Even with batching and slot alternation, QEMU TCG AHCI intermittently stalls (~1/3 boots hang). The stall happens during FAT32 I/O (directory lookup or file read). The only reliable mitigation is **AHCI command timeout + port reset + retry**.

### Proposed retry mechanism

```
dma_xfer():
  1. Issue command via PORT_CI
  2. Poll PORT_CI for 10M iterations (current)
  3. If timeout:
     a. Read PORT_IS, clear by writing IS bits
     b. Read PORT_SERR, clear by writing SERR bits
     c. Stop command engine (clear PORT_CMD.ST)
     d. Wait for PORT_CMD.CR to clear
     e. Start command engine (set PORT_CMD.ST | FRE | POD | SUD)
     f. Re-issue the same command (same slot, same parameters)
     g. Poll again
     h. If second timeout → return error
```

This is safe because the AHCI command table and DMA buffer are not modified by the device when the command is stuck (PORT_CI never clears). The port can be safely reset and the command re-issued.

The slot alternation + PxIS clear + batching should be kept as first-line defenses, with the retry as a last-resort recovery for QEMU emulation quirks.


## Proposed Solution: Multi-Sector Read Batching

### Strategy

Instead of issuing one AHCI command per 512-byte sector, batch consecutive sectors into a single READ DMA command. The FAT32 driver already iterates sectors sequentially; the IoStack already supports `read_sectors(lba, count)` with `count > 1`. The missing piece is **batching at the FAT32 level**.

### Design

```
FAT32::read_file_by_cluster()
  → for each cluster in chain:
      → compute consecutive sector range (N contiguous sectors)
      → self.io_stack.read_sectors(lba, N, &mut buf[offset..])
        → Cache: single miss lookup for first sector, populate for all N
        → Device: dev.read_blocks(lba, N, buf) → AHCI dma_xfer(lba, N, ...)
          → 1 AHCI command for up to 8 sectors (DMA_BUF_SIZE=4096)
```

### Implementation Steps

#### 1. FAT32 multi-sector batching (IMPLEMENTED)

Added `read_sectors(&self, lba, count, buf)` to `Fat32Driver` and modified both `read_file_by_cluster` and `find_entry_in_directory` to batch consecutive sector reads up to 8 sectors per AHCI command.

```rust
// Before: 1 sector per AHCI command
for i in 0..sectors_per_cluster {
    let sector = self.read_sector(lba + i)?;
    buf[offset..offset + 512].copy_from_slice(&sector);
    offset += 512;
}

// After: up to 8 sectors per AHCI command
const MAX_BATCH: u32 = 8;
while batch_offset < sectors_per_cluster {
    let batch = remaining.min(MAX_BATCH);
    self.read_sectors(lba + batch_offset, batch, &mut buf[offset..offset + bytes])?;
    offset += bytes;
    batch_offset += batch;
}
```

#### 2. AHCI command retry with port reset (NOT YET IMPLEMENTED)

The current timeout in `dma_xfer()` returns an error immediately. Implement a retry loop:

```rust
for retry in 0..2 {
    port_write32(abar, port, PORT_CI, ci_mask);
    // poll loop ...
    if !timed_out { break; }
    if retry == 0 {
        // Clear error states and reset command engine
        port_write32(abar, port, PORT_IS, 0xFFFF_FFFF);
        port_write32(abar, port, PORT_SERR, 0xFFFF_FFFF);
        let cmd = port_read32(abar, port, PORT_CMD);
        port_write32(abar, port, PORT_CMD, cmd & !(CMD_ST | CMD_FRE));
        // wait for CR to clear ...
        port_write32(abar, port, PORT_CMD, CMD_ST | CMD_FRE | CMD_POD | CMD_SUD);
        // wait for CR to set then clear ...
        // re-write command table (same data), then retry
    } else {
        return Err(()); // second timeout
    }
}
```

#### 3. AHCI trace points (IMPLEMENTED, gated)

The `boot_ahci.rs` driver has built-in `boot_benchmark` hooks (`ahci_cmd_start`, `ahci_cmd_polled`, `ahci_cmd_timeout`, `ahci_dma_failure`) that can be extended with optional serial output.

Recommended trace points for future debugging:

```
[AHCI_CMD] slot=X lba=X count=X
[AHCI_DONE] slot=X status=X
[AHCI_TIMEOUT] slot=X lba=X
```

These can be added behind a `#[cfg(feature = "ahci_trace")]` or runtime flag.

### Acceptance Criteria

1. **Deterministic boot**: 10/10 consecutive QEMU TCG boots reach `C:\>` prompt without caching.
2. **All tests pass**: `neodev test` reports 678/678 (or latest count) tests passed.
3. **No regression on KVM/hardware**: Boot and I/O work identically with KVM acceleration.
4. **Performance improvement**: A `C:\Programs\neoinit.nxe` load should issue <10 AHCI commands (down from 35+).
5. **No scheduler or TSS changes**: Only AHCI/FAT32/IO stack modified.

### Non-goals

- NCQ path improvements: NCQ is already batched (up to 32 tags) and works correctly.
- FAT32 write: read-only during boot; write can be batched separately.
- VFS cache changes: caching strategy is orthogonal to AHCI command generation.

## Tracing (optional, for future debugging)

Recommended trace points (gated behind `AHCI_DEBUG` compile flag or a runtime `AtomicBool`):

```
[AHCI_CMD] slot=X lba=X count=X         // before issuing
[AHCI_DONE] slot=X status=X poll=Y      // after completion
[AHCI_TIMEOUT] slot=X lba=X CI=0x...    // if poll loop expires
```

These were proven effective in the investigation — they identified the exact LBA and slot where the command stalled.
