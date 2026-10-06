# Code Health Audit — 2026-09

**Scope:** full NeoDOS kernel + docs, read-only audit.
**Kernel version:** v0.51.2 · **ABI:** v8 · SSDT highest assigned: 99
**Method:** `neodos-mcp` tools (`check_consistency`, `get_build_errors`,
`kernel_index`, `search_symbol`, subsystem info), automated source scans, and
code/doc cross-referencing. **No code was modified.**

This document is the detailed evidence behind the `CH-*` items added to
`roadmap/improvements.md`.

---

## 1. MCP tool observations

| Tool | Result | Assessment |
| --- | --- | --- |
| `check_consistency targets=all` | returns the 10 invariants only | **`targets` is ignored** — `code`, `docs`, `artifacts`, `invariants` produce byte-identical output. No actual validation is performed. |
| `get_build_errors` | `✗ kernel.elf NOT FOUND`, `✗ bootloader.efi NOT FOUND` | False negative. Both exist at `neodos/kernel.elf` and `neodos/bootloader.efi`; the server looks in the wrong directory. |
| `list_loaded_modules` | empty NEM/NXL lists | False negative: `neodos/console.nxl`, `libmath.nxl`, `libneodos.nxl`, `net.nxl` exist. Same path problem. |
| `kernel_index format=summary` | OK | Works. |
| `search_symbol` | OK | Found e.g. `send_ipi` 3×, `ObInfoClass` in kernel + libneodos. |
| `boot_phases` / `get_kernel_architecture` / `memory_layout` / `scheduler_info` / `security_info` / `ipc_info` | OK | Static descriptions, consistent with docs. |

**`get_kernel_architecture` reports “Object Manager (Ob) — 16 types, 7 syscalls”.
Actual `ObType` has 22 variants and there are 9 Ob syscalls (RAX 40-48).**

---

## 2. Dead code

Automated scan (single-pass identifier counting over all `.rs`; a symbol is
“dead” when it has **zero** references beyond its own definition). Full list in
Appendix A.

| Category | Count | Notes |
| --- | --- | --- |
| Unreferenced functions | ~230 | Includes `#[allow(dead_code)]` modules. |
| Unreferenced `static` | ~36 | 32 are `KEEP_*` HAL ABI anchors (intentional). |
| Orphan files | 0 | `object/enum.rs` and `syscall/ob/enum.rs` are declared as `pub mod r#enum;` (false positive). |

High-signal examples (verified by call-site grep):

- `src/syscall/ob/create_process.rs:11` — `handle_process_create` is an explicit
  **placeholder that returns `NoSys`**; a ~140-line comment says the real logic
  lives in `create.rs`. The file exists only to satisfy the module split.
- `src/arch/x64/ipi.rs`, `src/arch/x64/smp.rs`, `src/timers/apic.rs` — `send_ipi_all`
  and `send_ipi_all_excl_self` are defined **three times each and never called**.
- `src/fs/fsck.rs:9` carries `#![allow(dead_code)]`.
- `src/security/token.rs` — `has_privilege`, `enable_privilege`, `disable_privilege`,
  `is_in_group`, `add_group`, `inherit_from` are all unreferenced.
- `src/security/access.rs:50` — `se_access_check_sid` unreferenced (the ACL entry
  point is unused).
- `src/syscall/handlers.rs:238` — `handler_waitpid` unreferenced.
- `src/object/namespace.rs` — `ob_lookup_by_path`, `ob_lookup_path_no_follow`,
  `ob_remove_symlink`, `ob_remove_object_auto`, `ob_insert_symlink_checked`,
  `ob_is_path_protected` unreferenced.
- `src/syscall/mod.rs:219` — `static KEYBOARD_LAYOUT` unreferenced.
- `src/buffer/page_cache.rs` — `flush_inode`, `prefetch`, `needs_async_flush` unreferenced.
- `src/net/socket.rs` — `wake_socket_readers`, `wake_socket_connect_waiters`,
  `wake_socket_accept_waiters` unreferenced (waiter wake paths dead).
- `src/urn/mod.rs` — `urn_read`, `urn_write`, `urn_seek` unreferenced.
- `src/drivers/hotreload.rs` — `reload_driver` unreferenced.
- `src/fs/vfs.rs` — `mount_at_path`, `unmount_path` unreferenced (the live path is
  `vfs/mount.rs`).

> The `KEEP_*` statics under `src/hal/x64/` are deliberate no-elimination ABI
> anchors (`#[used]`-style) and are **not** dead code.

---

## 3. Duplicate code

| Duplicate | Locations | Assessment |
| --- | --- | --- |
| `FsckIntegrity` implementations | `src/fs/fsck.rs` (impl `Ne2Fsck`) and `src/drivers/fsck_neodos.rs` (impl + helpers) | Two impls of the same trait, both with `read_block`/`write_block`/`verify_magic`/`process_leaf_entry`. `fs/fsck.rs` piles `#![allow(dead_code)]` on top. |
| FSCK helpers | `crc32` in `drivers/fsck_neodos.rs:83` and `fs/crc32.rs:3` | Two CRC32 implementations. |
| IPI senders | `arch/x64/ipi.rs`, `arch/x64/smp.rs`, `timers/apic.rs` | 3× `send_ipi`, 3× `send_ipi_all`, 3× `send_ipi_all_excl_self`; only `arch::x64::ipi::send_ipi` is actually wired. |
| MMIO helpers | `drivers/boot_ahci.rs`, `drivers/nvme.rs`, `virtio/transport.rs` | `mmio_read32/64`, `mmio_write32/64` copy-pasted. |
| Port I/O | `hal/x64/io.rs` and `virtio/transport.rs` | `inb/outb/inw/outw/inl/outl` duplicated. |
| ACPI RSDP parsing | `timers/hpet.rs` and `power/acpi.rs` | `acpi_checksum`, `scan_range_for_rsdp`, `validate_rsdp`, `find_rsdp`, `find_table_in_rsdt`, `find_table_in_xsdt` duplicated. |
| Mount tables | `fs/vfs.rs` (`drives[26]`, `mounts[8]`) and `vfs/mount.rs` (`MountManager`, 16) | Two sources of truth; `vfs_unmount_filesystem` manually rolls back both. |
| Caches | `buffer/page_cache.rs` (884 lines) and `cm/cache.rs` (72 lines) | Independent slot/LRU/hit-rate logic. |
| CPU identity methods | `src/cpu.rs` | `vendor_str`/`brand_str` duplicated verbatim on both `CpuInfo` and `CpuInfoFull`; all of those methods plus `cpu_type_str` are unused. |

---

## 4. API inconsistencies — Ob info classes

`ObInfoClass` (`src/object/types.rs:118`) has 39 variants; `ObSetInfoClass`
(`:163`) has 42. Cross-referencing against `src/syscall/ob/query.rs` and
`src/syscall/ob/set.rs`:

- **`ObInfoClass::PowerState = 32` has no handler in `query.rs`.** It is
  documented as live in `docs/kernel/objects.md:206` and
  `docs/kernel/syscalls.md:377`. → documented API not implemented.
- Numeric gaps in both enums (`ObInfoClass`: 26→29; `ObSetInfoClass`: 28→33,
  39→43, 47→49). Some correspond to planned/removed classes (`PowerPlanInfo`,
  `PowerStatus`, `PowerSuspend`, …). ABI values are not contiguous.
- `ObType` has a gap at 19 (Thread=16, Section=17, Socket=18, Service=20) and the
  MCP reports “16 types” while the enum has 22.
- No completeness test asserts that every `ObInfoClass`/`ObSetInfoClass` variant
  has a dispatch arm, which is how the `PowerState` gap went unnoticed.

---

## 5. Docs vs code (stale content)

Highest-signal (full list from the docs review):

1. **Syscall renumbering not propagated.** Ob syscalls moved **RAX 60-66 → 40-48**;
   Cm moved **67-76 → 50-59**. Still stale in:
   `docs/kernel/objects.md` (ob_open “RAX=60” … ob_destroy “RAX=66”),
   `docs/registry/registry.md` (RAX 67-76),
   `docs/architecture/source-of-truth.md` §12.2,
   `docs/architecture/overview.md` syscall table,
   `docs/userland/libneodos.md`, `docs/kernel/ipc.md`, `docs/userland/shell.md`,
   `docs/memory/memory.md`, `docs/scheduler/scheduler.md`,
   `docs/services/power-manager.md`.
2. **`docs/kernel/syscalls.md:363`** — “Total active: 34 | Highest: 59”. Actual: **37
   assigned, highest 99** (`syscall/mod.rs:193`). The `SyscallNum` enum in that doc
   also omits `IcmpPing=36`, `ObSnapshot=48`, `DebugDump=99`.
3. **Test count 738 is stale** (`AGENTS.md:3`, `docs/README.md:3`,
   `docs/boot/boot-flow.md:82`, `docs/architecture/overview.md`, vision/audit docs).
   The in-repo #346 investigation records 752/754.
4. **Wrong `PowerState` / planned classes** — `docs/kernel/objects.md:207-208`
   lists `PowerPlanInfo=33`, `PowerStatus=34`; the real 33/34 are `FsckStatus` /
   `ProcessId`. `docs/kernel/objects.md:415-418` lists `PowerSuspend=39` …
   `PowerSetPolicy=42`; 39 is `FsckRepair`.
5. **Renamed/removed source paths** still referenced by current docs:
   `src/syscall/ob.rs` (now a directory), `src/cm/hive.rs` (now a directory),
   `src/net/e1000.rs` (NEM driver, not kernel source),
   `src/drivers/builtin_drivers.rs`, `buffer/block_cache.rs`, `src/kobj/*`,
   `src/pipe.rs`, `src/slab_container.rs`.
6. **Wrong struct/type names**: `ob_resolve_path()` → `ob_lookup_path()`;
   `MapView`/`UnmapView` → `SectionMapView`/`SectionUnmapView`;
   `NeoDosFs` → `NeoDosFsV2`; `\Device\PowerManager` → `\System\PowerManager`;
   `ObObjectTable` root is a `Vec`, not `BTreeMap`.
7. **Broken relative links** in `docs/filesystem/vfs-patterns.md` (missing `../../`).

---

## 6. Architectural improvement candidates

Detailed, evidence-backed list (see `roadmap/improvements.md` CH-12..CH-16).
Top items:

- **Silent work loss:** `work_queue.rs` capacity 64; `irp_complete`
  (`irp/mod.rs:285`) drops the completion callback when the high queue is full;
  `dpc/mod.rs` drops at 128; `hotreload.rs` silently drops past 16 entries;
  `MAX_ZOMBIES=64` can grow without bound.
- **Global serialization:** single `SCHEDULER` mutex plus O(n) scans
  (`scheduler/mod.rs:271`); single `OB_TABLE` mutex with linear lookup
  (`object/table.rs:218`); global `VFS`/`PAGE_CACHE`/`BLOCK_DEVICES` locks
  (`globals.rs:11`).
- **Fixed-size registries:** `MAX_DRIVERS=16`, `MAX_ISOLATED_DRIVERS=16`,
  `MAX_BLOCK_DEVICES=8`, `MAX_NICS=4`, `MAX_SOCKETS=64`,
  `MAX_TCP_CONNECTIONS=32`, `MAX_PIPES=16`, `MAX_TIMERS=64`,
  `MAX_SEMAPHORES=64`, `MAX_SECTIONS=32`, `MAX_HIVES=8`, `MAX_CELLS=2048`,
  `CACHE_SIZE=128`, `MAX_SUBDIR_MOUNTS=8`, syscall tables `[_;256]`.
- **Missing fault-safe user copy:** only `copy_user_string` exists; `read/write`
  and most Ob handlers validate non-atomically then dereference
  (`syscall/handlers.rs:56`, `syscall/ob/set.rs`, `syscall/ob/wait.rs:28`).
- **Thin Ob abstraction:** `ObOperations` exposes only `on_destroy`
  (`object/table.rs:8`), so syscalls reach into concrete managers — contrary to
  the stated NT-like design.
- **Five driver registries / two mount tables** must be kept in sync manually.

---

## Appendix A — Automated dead-code scan

The following is the raw output of the scan. “Dead” = zero references outside
the definition. Generic trait methods (`new`, `init`, `fmt`, …) are excluded
from the dead list; the duplicate list intentionally excludes those names too.

```text
=== TRULY DEAD FUNCTIONS (no reference anywhere, outside test files) ===
  vfs/io.rs:45  fn acquire_ref
  kbd/mod.rs:108  fn active_layout_mut
  drivers/device/mod.rs:99  fn add_device
  security/token.rs:72  fn add_group
  scheduler/lifecycle.rs:217  fn add_ring3_process
  drivers/dependency/mod.rs:143  fn all_drivers
  timers/apic.rs:320  fn apic_eoi
  net/arp.rs:199  fn arp_cache_entries
  hal/x64/irql.rs:168  fn at_dispatch
  scheduler/mod.rs:620  fn block_current_for_thread
  cpu.rs:28  fn brand_str
  cpu.rs:106  fn brand_str
  arch/x64/smp.rs:828  fn bsp_id
  net/icmp.rs:113  fn build_port_unreachable
  drivers/nem/runtime.rs:67  fn call_fini
  drivers/nem/runtime.rs:37  fn call_init
  drivers/caps.rs:38  fn cap_name
  invariants.rs:85  fn check_kernel_stack
  invariants.rs:75  fn check_stack_alignment
  panic_classification.rs:165  fn classify
  console.rs:554  fn clear_screen
  usermode.rs:358  fn clear_wait_pid
  net/ethernet.rs:50  fn compute_eth_fcs
  net/tcp.rs:76  fn compute_tcp_checksum
  interrupts/msi.rs:244  fn configure_msix_entries
  eventbus/mod.rs:313  fn count_handlers
  cpu.rs:115  fn cpu_type_str
  scheduler/diag.rs:382  fn ctx_frozen
  console.rs:550  fn cursor_blink_enabled
  hal/x64/irql.rs:150  fn deref_mut
  drivers/manifest.rs:132  fn descriptors_for_class
  arch/x64/smp.rs:642  fn detect_apic_id_count
  drivers/device/mod.rs:133  fn devices_without_driver
  security/token.rs:90  fn disable_privilege
  dpc/mod.rs:183  fn dpc_dropped_count
  drivers/nem/event.rs:19  fn driver_entry
  drivers/driver_runtime.rs:603  fn driver_names
  drivers/driver_runtime.rs:658  fn driver_names
  net/ipv4.rs:45  fn dst_ip_octets
  hal/pci.rs:61  fn ecam_read_config_byte
  hal/pci.rs:87  fn ecam_write_config_byte
  hal/pci.rs:77  fn ecam_write_config_word
  security/token.rs:86  fn enable_privilege
  handle.rs:206  fn entries_mut
  drivers/device/mod.rs:121  fn find_by_class
  cm/manager.rs:99  fn find_by_native
  cm/manager.rs:109  fn find_by_native_mut
  security/sam.rs:87  fn find_by_username_mut
  drivers/device/mod.rs:115  fn find_by_vendor_device
  net/tcp.rs:223  fn find_connection_by_addr
  drivers/manifest.rs:115  fn find_descriptor
  drivers/manifest.rs:120  fn find_descriptors_for_device
  cm/manager.rs:54  fn find_hive_by_key
  cm/manager.rs:63  fn find_hive_by_key_mut
  net/tcp.rs:229  fn find_listener
  drivers/gpt.rs:35  fn find_neodos_partition
  vfs/partition.rs:25  fn find_partitions_by_type
  buffer/page_cache.rs:395  fn flush_inode
  cm/hive/serialize.rs:295  fn flush_to_io
  drivers/block.rs:464  fn force_unregister_nem_block_device
  input/manager.rs:44  fn foreground_pid
  drivers/isolation.rs:577  fn format_isolation_info
  object/semaphore.rs:115  fn free_semaphore
  object/timer.rs:148  fn free_timer
  net/types.rs:23  fn from_slice
  syscall/mod.rs:101  fn from_u64
  timers/apic.rs:409  fn get_apic_id_for_cpu
  console.rs:76  fn get_bg
  console.rs:77  fn get_bold
  drivers/driver_runtime.rs:504  fn get_by_driver_type
  drivers/driver_runtime.rs:500  fn get_by_name_mut
  console.rs:74  fn get_col
  console.rs:75  fn get_fg
  log/mod.rs:188  fn get_level
  power/mod.rs:98  fn get_plan_mut
  console.rs:73  fn get_row
  object/semaphore.rs:127  fn get_semaphore_count
  drivers/isolation.rs:460  fn handle_isolated_page_fault
  syscall/ob/create_process.rs:11  fn handle_process_create
  syscall/handlers.rs:238  fn handler_waitpid
  scheduler/mod.rs:254  fn has_non_idle_processes
  apc/mod.rs:147  fn has_pending_kernel_apcs
  security/token.rs:82  fn has_privilege
  drivers/driver_runtime.rs:527  fn increment_tick
  handle.rs:284  fn index_mut
  security/token.rs:94  fn inherit_from
  net/ipv4.rs:89  fn ip_fragment_needed
  apc/mod.rs:297  fn irp_queue_apc_dpc_completion
  net/tcp.rs:57  fn is_ack
  hal/safe/msr.rs:153  fn is_enabled
  net/tcp.rs:58  fn is_fin
  security/token.rs:78  fn is_in_group
  drivers/driver_runtime.rs:197  fn is_operational
  net/tcp.rs:60  fn is_psh
  net/tcp.rs:59  fn is_rst
  handle.rs:123  fn is_stdio
  net/tcp.rs:56  fn is_syn
  object/semaphore.rs:90  fn is_used
  arch/x64/idt.rs:969  fn is_user_mode_interrupt
  net/types.rs:21  fn is_zero
  drivers/isolation.rs:443  fn iter_isolated_regions
  arch/x64/idt.rs:1517  fn kbd_irq_total
  kbd/event.rs:97  fn kbd_next_seq
  scheduler/diag.rs:639  fn kcpu_trace_enabled
  drivers/fat32.rs:253  fn list_directory
  cm/hive/serialize.rs:306  fn load_from_io
  drivers/pci.rs:189  fn map_bar_mmio
  vfs/io.rs:57  fn mark_stale
  memory/mod.rs:365  fn max_phys_addr
  memory/mod.rs:337  fn memory_map
  drivers/boot_ahci.rs:205  fn mmio_read64
  drivers/nvme.rs:149  fn mmio_read64
  fs/vfs.rs:264  fn mount_at_path
  drivers/boot_ahci.rs:810  fn ncq_submit_irp_batch
  buffer/page_cache.rs:573  fn needs_async_flush
  scheduler/thread.rs:47  fn new_idle_bare
  drivers/driver_runtime.rs:595  fn next_driver_id
  net/nic.rs:218  fn next_hop_mac
  net/nic.rs:271  fn nic_default_link_up
  net/nic.rs:361  fn nic_get_gateway
  net/nic.rs:347  fn nic_get_mask
  object/namespace.rs:856  fn ob_insert_symlink_checked
  object/namespace.rs:795  fn ob_is_path_protected
  object/namespace.rs:705  fn ob_lookup_by_path
  object/namespace.rs:701  fn ob_lookup_path_no_follow
  object/namespace.rs:880  fn ob_lookup_symlink
  object/namespace.rs:921  fn ob_remove_object_auto
  object/namespace.rs:884  fn ob_remove_symlink
  memory/mod.rs:361  fn page_size
  drivers/pci.rs:61  fn pci_config_write_byte
  object/pipe.rs:294  fn pipe_peek_read_closed
  power/mod.rs:90  fn plan_count
  drivers/boot_ahci.rs:232  fn port_wait_idle
  buffer/page_cache.rs:578  fn prefetch
  crash/mod.rs:476  fn print_crash_dump_full
  crash/mod.rs:443  fn print_crash_dump_status
  drivers/device/pci_scan.rs:64  fn print_pci_devices
  virtio/transport.rs:70  fn probe_any
  work_queue.rs:177  fn process_all
  work_queue.rs:160  fn process_high_safe
  work_queue.rs:169  fn process_low_safe
  hal/raw/cpu.rs:112  fn raw_invpcid
  hal/raw/cpu.rs:149  fn raw_lgdt
  hal/raw/cpu.rs:159  fn raw_ltr
  hal/raw/cpu.rs:39  fn raw_read_tscp
  drivers/pci.rs:179  fn read_bar64
  drivers/fat32.rs:373  fn read_file_by_path
  interrupts/ioapic.rs:235  fn read_redir_entry
  drivers/ps2.rs:141  fn read_scancode
  power/acpi.rs:194  fn read_u16_at
  drivers/driver_runtime.rs:519  fn record_event
  drivers/driver_runtime.rs:533  fn record_event_and_tick
  drivers/nem/driver.rs:79  fn register_callback
  drivers/nem/runtime.rs:75  fn register_event_bus_handler
  drivers/nem/runtime.rs:21  fn register_inline
  fs/neodos_io.rs:155  fn register_io_tests
  vfs/io.rs:51  fn release_ref
  drivers/hotreload.rs:339  fn reload_driver
  memory/layout.rs:57  fn reserve_at
  log/mod.rs:196  fn reset_all_levels
  log/mod.rs:192  fn reset_level
  scheduler/aging.rs:21  fn reset_time_slice
  arch/x64/smp.rs:563  fn restore_trampoline
  scheduler/diag.rs:449  fn rsp_trace_enabled
  drivers/fsck_neodos.rs:171  fn run_fsck
  power/mod.rs:188  fn save_active_plan_to_registry
  power/mod.rs:162  fn save_plan_to_registry
  scheduler/mod.rs:323  fn sched_dump
  security/access.rs:50  fn se_access_check_sid
  scheduler/address_space.rs:124  fn segment_count
  arch/x64/smp.rs:332  fn send_init_to
  arch/x64/ipi.rs:156  fn send_ipi_all
  arch/x64/smp.rs:862  fn send_ipi_all
  timers/apic.rs:376  fn send_ipi_all
  arch/x64/ipi.rs:167  fn send_ipi_all_excl_self
  arch/x64/smp.rs:875  fn send_ipi_all_excl_self
  timers/apic.rs:392  fn send_ipi_all_excl_self
  arch/x64/smp.rs:320  fn send_sipi_to
  boot_benchmark.rs:355  fn set_ahci_debug_enabled
  arch/x64/cpu_local.rs:393  fn set_apic_id
  boot_benchmark.rs:350  fn set_benchmark_report_enabled
  drivers/driver_runtime.rs:386  fn set_certification_step
  security/acl.rs:95  fn set_dacl
  net/ipv4.rs:38  fn set_dst
  security/acl.rs:91  fn set_group
  log/mod.rs:184  fn set_level
  security/acl.rs:87  fn set_owner
  drivers/isolation.rs:345  fn set_rw_permissions
  drivers/isolation.rs:338  fn set_rx_permissions
  net/ipv4.rs:37  fn set_src
  drivers/driver_runtime.rs:510  fn set_state
  power/mod.rs:74  fn set_state
  usermode.rs:354  fn set_wait_pid
  drivers/mod.rs:54  fn signal_device_event
  net/socket.rs:274  fn socket_get_type
  net/socket.rs:270  fn socket_next_accept_id
  net/socket.rs:308  fn socket_set_local
  net/ipv4.rs:44  fn src_ip_octets
  net/nic.rs:13  fn subnet_mask
  scheduler/types.rs:246  fn take_kernel_stack
  arch/x64/cpu_local.rs:591  fn this_cpu_current_tid
  arch/x64/cpu_local.rs:583  fn this_cpu_in_dispatch_level
  arch/x64/cpu_local.rs:676  fn this_cpu_inc_interrupt_count
  arch/x64/cpu_local.rs:919  fn this_cpu_set_slab_head
  arch/x64/cpu_local.rs:912  fn this_cpu_slab_head
  timers/hpet.rs:688  fn ticks_to_us
  arch/x64/idt.rs:110  fn timer_diag_dump_secondary
  fs/neodos_dir.rs:109  fn to_btree_entry
  arch/x64/smp.rs:823  fn total_cpus
  drivers/hotreload.rs:179  fn track_ob_entry
  fs/vfs.rs:308  fn unmount_path
  drivers/driver_runtime.rs:642  fn unregister_driver
  drivers/hotreload.rs:184  fn untrack_ob_entry
  urn/mod.rs:230  fn urn_read
  urn/mod.rs:260  fn urn_seek
  urn/mod.rs:246  fn urn_write
  drivers/isolation.rs:551  fn validate_driver_data_ptr
  drivers/isolation.rs:530  fn validate_export_str
  cpu.rs:24  fn vendor_str
  cpu.rs:102  fn vendor_str
  vfs/mount.rs:135  fn vfs_get_mount
  vfs/mount.rs:131  fn vfs_unmount
  vfs/mount.rs:172  fn vfs_unmount_filesystem
  input/vt.rs:121  fn vt_counts
  input/vt.rs:35  fn vt_event_trace_enable
  drivers/ps2.rs:131  fn wait_for_key
  net/socket.rs:127  fn wake_socket_accept_waiters
  net/socket.rs:118  fn wake_socket_connect_waiters
  net/socket.rs:109  fn wake_socket_readers
  scheduler/mod.rs:625  fn wake_thread_joiner
  watchdog/mod.rs:291  fn watchdog_stats_string
  globals.rs:97  fn with_block_devices
  vfs/io.rs:151  fn with_device
  globals.rs:73  fn with_page_cache
  drivers/fsck_neodos.rs:100  fn write_block
  fs/fsck.rs:74  fn write_block

=== TEST-ONLY FUNCTIONS (referenced only in test code) ===

=== DUPLICATE FUNCTION NAMES (>1 definition in src) ===
  new  (99x): graphics.rs:18, work_queue.rs:41, work_queue.rs:117, handle.rs:190, console.rs:171, slab.rs:164, slab.rs:291, trace.rs:61, lock_order.rs:74, arch/x64/pic.rs:11, arch/x64/pic.rs:33, arch/x64/cpu_local.rs:64, arch/x64/cpu_local.rs:194, arch/x64/cpu_local.rs:290, arch/x64/serial.rs:8, buffer/page_cache.rs:46, buffer/page_cache.rs:200, drivers/ata.rs:24, drivers/driver_runtime.rs:273, drivers/fsck_neodos.rs:64, drivers/block.rs:18, drivers/block.rs:358, drivers/caps.rs:83, drivers/driver_manager.rs:31, drivers/fat32.rs:74, drivers/hotreload.rs:69, drivers/hotreload.rs:106, drivers/mod.rs:33, drivers/abi/mod.rs:11, drivers/dependency/mod.rs:28, drivers/nem/driver.rs:67, drivers/device/mod.rs:95, eventbus/mod.rs:172, eventbus/mod.rs:221, fs/vfs.rs:126, fs/freelist.rs:25, fs/snapshot.rs:32, fs/btree.rs:40, fs/btree.rs:491, fs/neodos_v2.rs:92, hal/x64/irql.rs:106, irp/mod.rs:143, irp/mod.rs:316, irp/mod.rs:526, memory/layout.rs:27, memory/buddy.rs:26, syscall/table.rs:16, scheduler/address_space.rs:56, scheduler/mod.rs:169, dpc/mod.rs:60, crash/mod.rs:114, vfs/partition.rs:19, vfs/mount.rs:41, vfs/io.rs:26, security/sid.rs:15, security/token.rs:34, security/acl.rs:37, security/acl.rs:73, security/sam.rs:40, security/sam.rs:71, urn/mod.rs:72, exception/dispatcher.rs:69, object/semaphore.rs:31, object/section.rs:40, object/namespace.rs:85, object/namespace.rs:127, object/namespace.rs:148, object/table.rs:60, object/timer.rs:49, object/pipe.rs:90, input/vt.rs:145, input/vt.rs:193, input/manager.rs:15, net/types.rs:15, net/types.rs:45, net/types.rs:85, net/ethernet.rs:19, net/udp.rs:15, net/tcp.rs:29, net/tcp.rs:145, net/counters.rs:15, net/dns.rs:45, net/dns.rs:180, net/dns.rs:210, net/arp.rs:78, net/icmp.rs:47, net/socket.rs:29, net/nic.rs:46, cm/cache.rs:19, cm/manager.rs:29, cm/hive/core.rs:16, cm/hive/types.rs:33, cm/hive/types.rs:55, virtio/vring.rs:42, services/manager.rs:133, power/plan.rs:102, power/mod.rs:58, kbd/event.rs:18, kbd/mod.rs:88
  init  (21x): graphics.rs:46, console.rs:158, allocator.rs:10, boot_benchmark.rs:53, slab.rs:85, slab.rs:316, arch/x64/pic.rs:84, arch/x64/ipi.rs:351, arch/x64/gdt.rs:89, arch/x64/serial.rs:12, arch/x64/serial.rs:64, arch/x64/idt.rs:1642, drivers/rtc_bridge.rs:32, drivers/driver_manager.rs:41, interrupts/msi.rs:287, interrupts/ioapic.rs:114, timers/mod.rs:42, memory/mod.rs:74, input/manager.rs:69, power/acpi.rs:403, log/mod.rs:178
  register_tests  (20x): work_queue.rs:311, handle.rs:310, lock_order.rs:86, testing.rs:218, buffer/page_cache.rs:772, drivers/virtio_blk.rs:292, drivers/pci.rs:220, eventbus/mod.rs:556, hal/pci.rs:99, irp/mod.rs:674, interrupts/ioapic.rs:310, scheduler/accounting.rs:136, scheduler/tests.rs:9, dpc/mod.rs:403, apc/mod.rs:483, vfs/io.rs:227, object/pipe.rs:350, input/vt.rs:202, input/mod.rs:10, power/mod.rs:297
  from_raw  (14x): hal/raw/mod.rs:23, hal/raw/mod.rs:30, hal/safe/msr.rs:8, hal/safe/msr.rs:17, hal/safe/msr.rs:26, hal/safe/msr.rs:35, hal/safe/msr.rs:44, hal/safe/msr.rs:53, hal/safe/msr.rs:62, hal/safe/msr.rs:71, hal/safe/msr.rs:80, hal/safe/msr.rs:89, hal/safe/msr.rs:98, hal/safe/msr.rs:107
  to_str  (12x): panic_classification.rs:123, drivers/driver_runtime.rs:85, drivers/driver_runtime.rs:120, drivers/abi/mod.rs:41, nem/mod.rs:54, nem/mod.rs:86, vfs/mount.rs:19, urn/mod.rs:32, object/types.rs:35, object/types.rs:80, power/plan.rs:12, power/mod.rs:25
  into_raw  (12x): hal/safe/msr.rs:9, hal/safe/msr.rs:18, hal/safe/msr.rs:27, hal/safe/msr.rs:36, hal/safe/msr.rs:45, hal/safe/msr.rs:54, hal/safe/msr.rs:63, hal/safe/msr.rs:72, hal/safe/msr.rs:81, hal/safe/msr.rs:90, hal/safe/msr.rs:99, hal/safe/msr.rs:108
  fmt  (11x): panic_classification.rs:205, drivers/nem/driver.rs:55, fs/vfs.rs:25, scheduler/types.rs:126, scheduler/types.rs:132, scheduler/types.rs:224, security/sid.rs:55, urn/mod.rs:59, net/types.rs:34, net/types.rs:73, net/types.rs:89
  free  (10x): slab.rs:119, slab.rs:215, fs/freelist.rs:67, irp/mod.rs:118, irp/mod.rs:554, syscall/permission.rs:16, object/semaphore.rs:53, object/section.rs:74, object/timer.rs:70, object/pipe.rs:57
  set_base_lba  (10x): drivers/ata.rs:28, drivers/block.rs:152, drivers/block.rs:242, drivers/block.rs:275, drivers/block.rs:319, drivers/block.rs:415, drivers/boot_ahci.rs:909, drivers/nvme.rs:731, drivers/virtio_blk.rs:276, fs/fsck.rs:265
  base_lba  (10x): drivers/ata.rs:29, drivers/block.rs:154, drivers/block.rs:243, drivers/block.rs:279, drivers/block.rs:323, drivers/block.rs:419, drivers/boot_ahci.rs:913, drivers/nvme.rs:732, drivers/virtio_blk.rs:277, fs/fsck.rs:264
  is_empty  (9x): work_queue.rs:89, arch/x64/cpu_local.rs:96, drivers/caps.rs:107, irp/mod.rs:351, irp/mod.rs:579, scheduler/types.rs:118, security/acl.rs:59, object/table.rs:195, net/arp.rs:165
  len  (9x): handle.rs:214, arch/x64/cpu_local.rs:158, drivers/fsck_neodos.rs:78, irp/mod.rs:355, scheduler/types.rs:117, object/table.rs:191, net/udp.rs:26, net/dns.rs:106, net/arp.rs:164
  alloc  (9x): slab.rs:108, slab.rs:186, slab.rs:384, fs/freelist.rs:40, irp/mod.rs:95, object/semaphore.rs:37, object/section.rs:46, object/timer.rs:55, object/pipe.rs:119
  read_sector  (9x): drivers/ata.rs:49, drivers/block.rs:156, drivers/block.rs:283, drivers/block.rs:327, drivers/block.rs:423, drivers/fat32.rs:95, drivers/nvme.rs:734, drivers/virtio_blk.rs:279, vfs/io.rs:138
  read_blocks  (9x): drivers/ata.rs:85, drivers/block.rs:145, drivers/block.rs:226, drivers/block.rs:267, drivers/block.rs:311, drivers/block.rs:403, drivers/boot_ahci.rs:897, drivers/virtio_blk.rs:257, fs/fsck.rs:234
  write_blocks  (9x): drivers/ata.rs:108, drivers/block.rs:148, drivers/block.rs:238, drivers/block.rs:271, drivers/block.rs:315, drivers/block.rs:409, drivers/boot_ahci.rs:901, drivers/virtio_blk.rs:264, fs/fsck.rs:247
  lookup  (9x): drivers/fat32.rs:418, fs/vfs.rs:54, fs/btree.rs:102, fs/neodos_v2.rs:237, irp/mod.rs:546, object/table.rs:104, net/dns.rs:59, net/arp.rs:92, cm/cache.rs:28
  write  (8x): trace.rs:73, drivers/fat32.rs:414, fs/vfs.rs:53, fs/vfs.rs:242, fs/neodos_v2.rs:205, hal/safe/msr.rs:140, hal/safe/msr.rs:165, object/pipe.rs:251
  drop  (8x): lock_order.rs:81, drivers/boot_ahci.rs:287, drivers/nvme.rs:791, drivers/virtio_blk.rs:230, hal/x64/irql.rs:156, scheduler/tests.rs:1583, scheduler/tests.rs:1658, scheduler/tests.rs:1945
  write_sector  (8x): drivers/ata.rs:68, drivers/block.rs:162, drivers/block.rs:287, drivers/block.rs:331, drivers/block.rs:429, drivers/nvme.rs:747, drivers/virtio_blk.rs:285, vfs/io.rs:145
  submit_irp  (8x): drivers/block.rs:136, drivers/block.rs:196, drivers/block.rs:249, drivers/block.rs:293, drivers/block.rs:385, drivers/boot_ahci.rs:860, drivers/virtio_blk.rs:240, fs/fsck.rs:260
  read  (8x): drivers/fat32.rs:396, fs/vfs.rs:52, fs/vfs.rs:232, fs/neodos_v2.rs:190, hal/safe/msr.rs:136, hal/safe/msr.rs:148, hal/safe/msr.rs:169, object/pipe.rs:231
  clear  (7x): graphics.rs:32, arch/x64/cpu_local.rs:100, drivers/dependency/mod.rs:135, scheduler/address_space.rs:120, net/dns.rs:80, net/arp.rs:135, cm/cache.rs:69
  pop  (7x): work_queue.rs:70, arch/x64/cpu_local.rs:85, eventbus/mod.rs:191, eventbus/mod.rs:295, irp/mod.rs:334, input/vt.rs:169, kbd/event.rs:30
  get  (7x): handle.rs:218, buffer/page_cache.rs:83, drivers/driver_runtime.rs:488, drivers/block.rs:37, drivers/hotreload.rs:133, vfs/mount.rs:92, net/nic.rs:124
  remove  (7x): arch/x64/cpu_local.rs:126, buffer/page_cache.rs:97, drivers/driver_runtime.rs:474, drivers/block.rs:72, drivers/caps.rs:103, net/arp.rs:131, services/manager.rs:268
  on_destroy  (7x): object/semaphore.rs:104, object/section.rs:177, object/power.rs:10, object/table.rs:9, object/table.rs:17, object/timer.rs:129, object/pipe.rs:316
  push  (6x): work_queue.rs:53, arch/x64/cpu_local.rs:74, eventbus/mod.rs:180, irp/mod.rs:324, input/vt.rs:153, kbd/event.rs:21
  register  (6x): testing.rs:18, drivers/driver_runtime.rs:282, drivers/block.rs:25, drivers/hotreload.rs:110, net/nic.rs:64, services/manager.rs:151
  insert  (6x): buffer/page_cache.rs:53, drivers/fsck_neodos.rs:68, fs/btree.rs:119, net/dns.rs:63, net/arp.rs:96, cm/cache.rs:48
  count  (6x): drivers/driver_runtime.rs:544, drivers/block.rs:98, drivers/caps.rs:125, drivers/device/mod.rs:111, vfs/mount.rs:114, net/nic.rs:140
  create  (6x): drivers/fat32.rs:495, fs/vfs.rs:57, fs/vfs.rs:356, fs/snapshot.rs:41, fs/neodos_v2.rs:284, object/table.rs:69
  name  (6x): drivers/nem/net_bridge.rs:33, scheduler/process.rs:80, scheduler/types.rs:251, net/nic.rs:8, net/tests.rs:25, net/tests.rs:44
  write_str  (5x): panic_classification.rs:56, console.rs:128, arch/x64/serial.rs:47, arch/x64/serial.rs:86, crash/mod.rs:125
  name_str  (5x): drivers/driver_runtime.rs:191, object/namespace.rs:103, object/namespace.rs:138, object/table.rs:36, kbd/layout.rs:45
  num_sectors  (5x): drivers/block.rs:126, drivers/block.rs:377, drivers/boot_ahci.rs:905, drivers/nvme.rs:758, drivers/virtio_blk.rs:237
  serialize  (5x): fs/freelist.rs:124, fs/snapshot.rs:87, fs/neodos_dir.rs:67, fs/btree.rs:48, cm/hive/serialize.rs:8
  deserialize  (5x): fs/freelist.rs:148, fs/snapshot.rs:105, fs/neodos_dir.rs:89, fs/btree.rs:67, cm/hive/serialize.rs:119
  handler  (4x): work_queue.rs:191, eventbus/mod.rs:470, eventbus/mod.rs:512, eventbus/mod.rs:526
  is_valid  (4x): handle.rs:141, drivers/abi/mod.rs:23, vfs/io.rs:62, net/ipv4.rs:40
  as_str  (4x): boot_benchmark.rs:92, scheduler/types.rs:113, cm/hive/types.rs:64, log/mod.rs:23
  default  (4x): trace.rs:55, drivers/driver_runtime.rs:162, scheduler/types.rs:122, kbd/mod.rs:67
  is_bsp  (4x): arch/x64/msr.rs:42, arch/x64/smp.rs:833, hal/safe/msr.rs:158, timers/apic.rs:338
  flush  (4x): arch/x64/serial.rs:39, buffer/page_cache.rs:372, drivers/block.rs:150, drivers/virtio_blk.rs:271
  sector_size  (4x): drivers/block.rs:130, drivers/block.rs:381, drivers/nvme.rs:706, drivers/virtio_blk.rs:238
  readdir  (4x): drivers/fat32.rs:429, fs/vfs.rs:55, fs/vfs.rs:247, fs/neodos_v2.rs:248
  mkdir  (4x): drivers/fat32.rs:491, fs/vfs.rs:56, fs/vfs.rs:340, fs/neodos_v2.rs:263
  stat  (4x): drivers/fat32.rs:499, fs/vfs.rs:58, fs/vfs.rs:237, fs/neodos_v2.rs:297
  volume_label  (4x): drivers/fat32.rs:514, fs/vfs.rs:68, fs/vfs.rs:252, fs/neodos_v2.rs:346
  mac_address  (4x): drivers/nem/net_bridge.rs:32, net/nic.rs:7, net/tests.rs:24, net/tests.rs:43
  ip_address  (4x): drivers/nem/net_bridge.rs:41, net/nic.rs:12, net/tests.rs:32, net/tests.rs:48
  set_ip_address  (4x): drivers/nem/net_bridge.rs:42, net/nic.rs:11, net/tests.rs:31, net/tests.rs:47
  send_packet  (4x): drivers/nem/net_bridge.rs:47, net/nic.rs:9, net/tests.rs:26, net/tests.rs:45
  poll_packet  (4x): drivers/nem/net_bridge.rs:58, net/nic.rs:10, net/tests.rs:30, net/tests.rs:46
  tick  (4x): object/timer.rs:98, object/timer.rs:140, net/dns.rs:52, net/arp.rs:85
  dummy  (3x): work_queue.rs:250, work_queue.rs:301, eventbus/mod.rs:495
  alloc_handle  (3x): handle.rs:231, handle.rs:300, urn/mod.rs:89
  send_ipi  (3x): arch/x64/ipi.rs:145, arch/x64/smp.rs:849, timers/apic.rs:354
  send_ipi_all  (3x): arch/x64/ipi.rs:156, arch/x64/smp.rs:862, timers/apic.rs:376
  send_ipi_all_excl_self  (3x): arch/x64/ipi.rs:167, arch/x64/smp.rs:875, timers/apic.rs:392
  contains  (3x): arch/x64/cpu_local.rs:107, buffer/page_cache.rs:116, drivers/fsck_neodos.rs:74
  peek  (3x): arch/x64/cpu_local.rs:164, buffer/page_cache.rs:333, irp/mod.rs:343
  unregister  (3x): drivers/driver_runtime.rs:464, drivers/hotreload.rs:129, net/nic.rs:117
  get_mut  (3x): drivers/driver_runtime.rs:492, irp/mod.rs:123, net/nic.rs:132
  mmio_read32  (3x): drivers/boot_ahci.rs:197, drivers/nvme.rs:143, virtio/transport.rs:137
  mmio_write32  (3x): drivers/boot_ahci.rs:201, drivers/nvme.rs:146, virtio/transport.rs:140
  mmio_write64  (3x): drivers/boot_ahci.rs:209, drivers/nvme.rs:152, virtio/transport.rs:143
  read_sectors  (3x): drivers/fat32.rs:99, drivers/nvme.rs:708, vfs/io.rs:87
  fs_type  (3x): drivers/fat32.rs:527, fs/vfs.rs:74, fs/neodos_v2.rs:358
  total_sectors  (3x): drivers/fat32.rs:531, fs/vfs.rs:77, fs/neodos_v2.rs:359
  is_link_up  (3x): drivers/nem/net_bridge.rs:45, net/nic.rs:18, net/tests.rs:49
  remove_file  (3x): fs/vfs.rs:59, fs/vfs.rs:372, fs/neodos_v2.rs:303
  remove_dir  (3x): fs/vfs.rs:62, fs/vfs.rs:388, fs/neodos_v2.rs:314
  rename  (3x): fs/vfs.rs:65, fs/vfs.rs:404, fs/neodos_v2.rs:331
  set_volume_label  (3x): fs/vfs.rs:71, fs/vfs.rs:258, fs/neodos_v2.rs:351
  snapshot_create  (3x): fs/vfs.rs:84, fs/vfs.rs:423, fs/neodos_v2.rs:361
  snapshot_restore  (3x): fs/vfs.rs:88, fs/vfs.rs:428, fs/neodos_v2.rs:369
  snapshot_list  (3x): fs/vfs.rs:92, fs/vfs.rs:433, fs/neodos_v2.rs:375
  snapshot_purge  (3x): fs/vfs.rs:96, fs/vfs.rs:438, fs/neodos_v2.rs:401
  mount  (3x): fs/vfs.rs:145, vfs/mount.rs:47, cm/manager.rs:33
  unmount  (3x): fs/vfs.rs:151, vfs/mount.rs:73, cm/manager.rs:45
  read_node  (3x): fs/btree.rs:16, fs/btree.rs:495, fs/neodos_v2.rs:47
  write_node  (3x): fs/btree.rs:17, fs/btree.rs:500, fs/neodos_v2.rs:61
  from_u8  (3x): nem/mod.rs:46, nem/mod.rs:75, log/mod.rs:33
  alloc_frames  (3x): memory/buddy.rs:169, memory/buddy.rs:268, memory/mod.rs:349
  free_frames  (3x): memory/buddy.rs:192, memory/buddy.rs:272, memory/mod.rs:353
  allocate_frame  (3x): memory/buddy.rs:219, memory/buddy.rs:260, memory/mod.rs:341
  free_frame  (3x): memory/buddy.rs:223, memory/buddy.rs:264, memory/mod.rs:345
  empty  (3x): scheduler/types.rs:52, scheduler/snapshot.rs:105, kbd/layout.rs:16
  from_str  (3x): scheduler/types.rs:57, urn/mod.rs:41, power/plan.rs:20
  unused  (3x): object/semaphore.rs:16, object/section.rs:24, object/timer.rs:32
  from_u32  (3x): net/types.rs:50, power/plan.rs:29, power/plan.rs:49
  ttl  (3x): net/ipv4.rs:31, net/dns.rs:231, net/dns.rs:247
  vendor_str  (2x): cpu.rs:24, cpu.rs:102
  brand_str  (2x): cpu.rs:28, cpu.rs:106
  pending_count  (2x): work_queue.rs:94, dpc/mod.rs:136
  alloc_two_handles  (2x): handle.rs:247, handle.rs:304
  _print  (2x): console.rs:152, arch/x64/serial.rs:71
  watchdog_check  (2x): boot_benchmark.rs:272, watchdog/mod.rs:110
  is_full  (2x): slab.rs:149, irp/mod.rs:584
  dump  (2x): trace.rs:88, object/namespace.rs:753
  slot  (2x): lock_order.rs:34, cm/hive/core.rs:67
  lapic_write_icr  (2x): arch/x64/ipi.rs:118, arch/x64/smp.rs:278
  noop  (2x): arch/x64/ipi.rs:389, dpc/mod.rs:75
  mark_dirty  (2x): buffer/page_cache.rs:355, cm/cache.rs:58
  entry_count  (2x): buffer/page_cache.rs:470, security/sam.rs:109
  stats  (2x): buffer/page_cache.rs:478, memory/mod.rs:292
  hit_rate  (2x): buffer/page_cache.rs:507, cm/cache.rs:64
  set_capabilities  (2x): drivers/driver_runtime.rs:396, drivers/driver_runtime.rs:673
  get_capabilities  (2x): drivers/driver_runtime.rs:417, drivers/driver_runtime.rs:678
  check_driver_cap  (2x): drivers/driver_runtime.rs:423, drivers/driver_runtime.rs:668
  get_by_name  (2x): drivers/driver_runtime.rs:496, drivers/hotreload.rs:137
  set_state  (2x): drivers/driver_runtime.rs:510, power/mod.rs:74
  loaded_count  (2x): drivers/driver_runtime.rs:556, drivers/driver_manager.rs:351
  driver_names  (2x): drivers/driver_runtime.rs:603, drivers/driver_runtime.rs:658
  driver_count  (2x): drivers/driver_runtime.rs:654, drivers/dependency/mod.rs:139
  crc32  (2x): drivers/fsck_neodos.rs:83, fs/crc32.rs:3
  read_block  (2x): drivers/fsck_neodos.rs:99, fs/fsck.rs:65
  write_block  (2x): drivers/fsck_neodos.rs:100, fs/fsck.rs:74
  total_blocks  (2x): drivers/fsck_neodos.rs:101, fs/fsck.rs:84
  root_btree_lba  (2x): drivers/fsck_neodos.rs:102, fs/fsck.rs:88
  verify_magic  (2x): drivers/fsck_neodos.rs:103, fs/fsck.rs:92
  verify_superblock_checksum  (2x): drivers/fsck_neodos.rs:104, fs/fsck.rs:96
  process_leaf_entry  (2x): drivers/fsck_neodos.rs:106, fs/fsck.rs:104
  get_child_lba  (2x): drivers/fsck_neodos.rs:107, fs/fsck.rs:135
  repair_superblock  (2x): drivers/fsck_neodos.rs:108, fs/fsck.rs:143
  release  (2x): drivers/block.rs:59, object/semaphore.rs:59
  find_by_name  (2x): drivers/block.rs:103, services/manager.rs:141
  poll_irp  (2x): drivers/block.rs:140, drivers/boot_ahci.rs:918
  mmio_read64  (2x): drivers/boot_ahci.rs:205, drivers/nvme.rs:149
  probe  (2x): drivers/boot_ahci.rs:332, drivers/virtio_blk.rs:68
  resolve_path  (2x): drivers/fat32.rs:213, fs/vfs.rs:208
  read_file  (2x): drivers/fat32.rs:365, drivers/nem/loader.rs:46
  from  (2x): drivers/fat32.rs:385, fs/vfs.rs:31
  alloc_contig  (2x): drivers/nvme.rs:185, drivers/virtio_blk.rs:49
  free_contig  (2x): drivers/nvme.rs:199, drivers/virtio_blk.rs:56
  zero_phys  (2x): drivers/nvme.rs:204, drivers/virtio_blk.rs:60
  write_sectors  (2x): drivers/nvme.rs:720, vfs/io.rs:114
  set_leds  (2x): drivers/ps2.rs:123, kbd/mod.rs:151
  free_page  (2x): drivers/virtio_blk.rs:44, hal/x64/mem.rs:48
  description  (2x): drivers/nem/net_bridge.rs:37, net/nic.rs:22
  vendor_id  (2x): drivers/nem/net_bridge.rs:43, net/nic.rs:20
  device_id  (2x): drivers/nem/net_bridge.rs:44, net/nic.rs:21
  push_event  (2x): eventbus/mod.rs:235, eventbus/mod.rs:396
  fsck  (2x): fs/vfs.rs:80, fs/neodos_v2.rs:405
  region_count  (2x): fs/freelist.rs:118, memory/layout.rs:86
  dir_count  (2x): fs/neodos_dir.rs:150, object/namespace.rs:581
  checksum  (2x): fs/fsck.rs:45, net/ipv4.rs:35
  inb  (2x): hal/x64/io.rs:5, virtio/transport.rs:105
  outb  (2x): hal/x64/io.rs:11, virtio/transport.rs:120
  inw  (2x): hal/x64/io.rs:17, virtio/transport.rs:110
  outw  (2x): hal/x64/io.rs:23, virtio/transport.rs:125
  inl  (2x): hal/x64/io.rs:29, virtio/transport.rs:115
  outl  (2x): hal/x64/io.rs:35, virtio/transport.rs:130
  reboot  (2x): hal/x64/cpu.rs:31, power/coordinator.rs:16
  read_cr2  (2x): hal/x64/cpu.rs:77, hal/safe/msr.rs:174
  acpi_checksum  (2x): timers/hpet.rs:98, power/acpi.rs:62
  scan_range_for_rsdp  (2x): timers/hpet.rs:107, power/acpi.rs:87
  validate_rsdp  (2x): timers/hpet.rs:123, power/acpi.rs:70
  find_rsdp  (2x): timers/hpet.rs:150, power/acpi.rs:103
  find_table_in_rsdt  (2x): timers/hpet.rs:193, power/acpi.rs:133
  find_table_in_xsdt  (2x): timers/hpet.rs:206, power/acpi.rs:145
  reserve_region  (2x): memory/layout.rs:31, memory/layout.rs:115
  iter  (2x): memory/layout.rs:90, vfs/mount.rs:118
  init_bitmap  (2x): memory/buddy.rs:104, memory/buddy.rs:252
  init_from_regions  (2x): memory/buddy.rs:112, memory/buddy.rs:256
  mark_used_region  (2x): memory/buddy.rs:227, memory/buddy.rs:276
  free_pages  (2x): memory/buddy.rs:239, memory/buddy.rs:280
  new_idle  (2x): scheduler/process.rs:7, scheduler/thread.rs:8
  new_ring3  (2x): scheduler/process.rs:55, scheduler/thread.rs:77
  to_u8  (2x): scheduler/types.rs:161, net/types.rs:120
  state_name  (2x): scheduler/snapshot.rs:220, scheduler/schedule.rs:36
  spawn_net_kthread  (2x): scheduler/stack.rs:40, net/mod.rs:116
  current_pid  (2x): scheduler/mod.rs:118, scheduler/mod.rs:572
  apc_callback  (2x): apc/mod.rs:347, apc/mod.rs:366
  irp_callback  (2x): apc/mod.rs:423, apc/mod.rs:449
  is_admin  (2x): security/sam.rs:50, security/sam.rs:105
  map_view  (2x): object/section.rs:83, object/section.rs:188
  unmap_view  (2x): object/section.rs:104, object/section.rs:192
  init_power_manager  (2x): object/power.rs:15, power/mod.rs:198
  lookup_mut  (2x): object/table.rs:109, cm/cache.rs:38
  reset  (2x): object/pipe.rs:40, virtio/transport.rs:152
  as_err_code  (2x): object/types.rs:95, services/manager.rs:69
  active_vt  (2x): input/manager.rs:40, input/manager.rs:72
  switch_vt  (2x): input/manager.rs:49, input/manager.rs:73
  push_byte  (2x): input/manager.rs:60, input/manager.rs:74
  pop_byte_from_vt  (2x): input/manager.rs:64, input/manager.rs:75
  broadcast  (2x): net/types.rs:16, net/types.rs:48
  is_broadcast  (2x): net/types.rs:19, net/types.rs:60
  is_multicast  (2x): net/types.rs:20, net/types.rs:63
  payload_len  (2x): net/ipv4.rs:29, net/udp.rs:27
  src_port  (2x): net/udp.rs:24, net/tcp.rs:43
  dst_port  (2x): net/udp.rs:25, net/tcp.rs:44
  flags  (2x): net/tcp.rs:50, net/dns.rs:193
  allocate_ephemeral_port  (2x): net/tcp.rs:207, net/socket.rs:38
  evict_expired  (2x): net/dns.rs:84, net/arp.rs:139
  evict_oldest  (2x): net/dns.rs:92, net/arp.rs:145
  rtype  (2x): net/dns.rs:229, net/dns.rs:245
  rdlength  (2x): net/dns.rs:232, net/dns.rs:246
  register_net_tests  (2x): net/mod.rs:313, net/tests.rs:62
  register_pm_tests  (2x): power/acpi.rs:508, power/mod.rs:207

=== UNUSED STATICS ===
  boot_benchmark.rs:191  static AHCI_RETRIES
  usermode.rs:30  static EXIT_R12
  usermode.rs:32  static EXIT_R13
  usermode.rs:34  static EXIT_R14
  usermode.rs:36  static EXIT_R15
  usermode.rs:38  static EXIT_RBP
  usermode.rs:28  static EXIT_RBX
  hal/x64/cpu.rs:116  static KEEP_CPU_DISABLE_INTERRUPTS
  hal/x64/cpu.rs:114  static KEEP_CPU_ENABLE_INTERRUPTS
  hal/x64/cpu.rs:130  static KEEP_CPU_FLUSH_TLB
  hal/x64/cpu.rs:118  static KEEP_CPU_HALT
  hal/x64/cpu.rs:134  static KEEP_CPU_HLT_ONCE
  hal/x64/cpu.rs:136  static KEEP_CPU_INFO
  hal/x64/cpu.rs:132  static KEEP_CPU_INTERRUPTS_ENABLED
  hal/x64/cpu.rs:122  static KEEP_CPU_POWEROFF
  hal/x64/cpu.rs:124  static KEEP_CPU_READ_CR2
  hal/x64/cpu.rs:126  static KEEP_CPU_READ_CR3
  hal/x64/cpu.rs:120  static KEEP_CPU_REBOOT
  hal/x64/cpu.rs:128  static KEEP_CPU_WRITE_CR3
  hal/x64/io.rs:41  static KEEP_IO_INB
  hal/x64/io.rs:49  static KEEP_IO_INL
  hal/x64/io.rs:45  static KEEP_IO_INW
  hal/x64/io.rs:43  static KEEP_IO_OUTB
  hal/x64/io.rs:51  static KEEP_IO_OUTL
  hal/x64/io.rs:47  static KEEP_IO_OUTW
  hal/x64/irq.rs:50  static KEEP_IRQ_ACK_IRQ
  hal/x64/irq.rs:48  static KEEP_IRQ_REGISTER_IRQ
  hal/x64/mem.rs:101  static KEEP_MEM_ALLOC_PAGE
  hal/x64/mem.rs:103  static KEEP_MEM_FREE_PAGE
  hal/x64/mem.rs:105  static KEEP_MEM_MAP_PAGE
  hal/x64/mem.rs:109  static KEEP_MEM_MEMORY_BARRIER
  hal/x64/mem.rs:107  static KEEP_MEM_UNMAP_PAGE
  hal/x64/time.rs:49  static KEEP_TIME_GET_TICKS
  hal/x64/time.rs:55  static KEEP_TIME_GET_TICK_RATE
  hal/x64/time.rs:53  static KEEP_TIME_INCREMENT_TICKS
  hal/x64/time.rs:57  static KEEP_TIME_INIT_TIMER
  hal/x64/time.rs:51  static KEEP_TIME_SLEEP_HINT
  syscall/mod.rs:219  static KEYBOARD_LAYOUT

=== ORPHAN .rs FILES (no `mod X;` declaration anywhere) ===
  object/enum.rs
  syscall/ob/enum.rs
```
