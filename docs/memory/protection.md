# Memory Protection & User/Kernel Isolation

Source of truth: `neodos-kernel/src/arch/x64/paging.rs`,
`neodos-kernel/src/arch/x64/features.rs`, `neodos-kernel/src/hal/safe/cpu.rs`.
Tracking issue: NEODOS-09 (#639).

## Address-space model

NeoDOS uses a **single kernel page table** (`PML4[0]`) that identity-maps
`0..4 GiB` with 2 MB huge pages (`init_custom_page_tables`, `paging.rs`). There
is no per-process `CR3`/`PML4` yet: processes are isolated by a 32 MB user
window plus per-process slot/heap/mmap regions, all in the same address space.

The user window is the *only* region additionally marked `USER_ACCESSIBLE`. Every
other page (kernel image, kernel heap, page tables, drivers, MMIO) is
kernel-only.

## `USER_ACCESSIBLE` audit

`is_user_range(addr)` (`paging.rs:523`) grants `USER_ACCESSIBLE` only for
`USER_BASE..USER_LIMIT` (`0x0040_0000..0x0240_0000`). The 4 KB splits that grant
`USER_ACCESSIBLE` are constrained to user ranges. Audited regions:

| Region | Range | User-accessible | Notes |
|--------|-------|-----------------|-------|
| User slot window | `0x400000..0x2400000` | **yes** | 2 MB huge pages `PRESENT\|WRITABLE\|USER` (user code+data) |
| User heap | `0x10000000..0x12000000` | **yes** | demand-paged 4 KB, `USER\|WRITABLE\|NX` |
| mmap | `0x20000000..0x22000000` | **yes** | demand-paged 4 KB, `USER\|WRITABLE\|NX` |
| NXL region | `0x1E000000..0x1E200000` | **yes** | shared libraries, split at init |
| Kernel image | `0x4000000..` | no | `PRESENT\|WRITABLE`, kernel-only (W^X pending) |
| Kernel heap | `0x2400000..0x3400000` | no | kernel-only |
| Page tables / KPRCB / MMIO | various | no | kernel-only |

No kernel region is reachable from Ring 3: the only `USER_ACCESSIBLE` bits are set
by `is_user_range` (user window), `set_pd_user_accessible` (NXL), and the
heap/mmap 4 KB splits.

## SMEP / SMAP / NX

Detection and enablement live in `arch/x64/features.rs`; the raw asm stays in
`hal/raw` and is wrapped by `hal/safe` (`read_cr4`, `write_cr4`, `cpuid`,
`stac`, `clac`, `with_user_access`) per the HAL-RAW-SAFE rule.

- **Detection** (CPUID, boot-time): SMEP `CPUID.7.0:EBX[7]`, SMAP `EBX[20]`,
  NX `CPUID.80000001h:EDX[20]`.
- **Enablement** (`enable_on_this_cpu`, BSP + each AP): sets `CR4.SMEP`/`CR4.SMAP`
  and `EFER.NXE` **only if supported**, so hardware without the feature still
  boots. State is exposed via `smep/smap/nx_{supported,enabled}()`.
- **SMAP and legitimate copies**: the kernel may only touch user pages with
  `RFLAGS.AC` set. `hal::safe::with_user_access` brackets the validated access in
  the `copy_from_user`/`copy_to_user` helpers (`syscall/util.rs`), so syscall
  copies keep working while stray kernel accesses to user memory fault.
- **NX / W^X**: `EFER.NXE` is enabled when supported; user **heap and mmap** data
  pages are mapped `NO_EXECUTE` (`paging.rs heap_alloc_page`/`mmap_alloc_page` via
  `hal::map_page` bit 63). User code lives in the executable user window, so this
  is safe (`mmap` prot has no execute bit).

## Remaining hardening (follow-up on #639)

- **Kernel image W^X**: the kernel RW segment is still executable. Marking it NX
  requires splitting its 2 MB huge pages and setting NX on the data range while
  keeping the text range executable.
- **Recoverable fault probe test**: `page_fault_handler` is fatal for kernel-mode
  faults, so a "Ring-0 read of a user page traps" test needs a fixup/recovery
  mechanism that does not exist yet. Until then, the property is verified by the
  gated feature test plus the fault-safe copy helpers.
- **Per-process `PML4`/`CR3`**: `AddressSpace` tracks ELF segments only; real
  per-process address spaces are out of scope here.
