# Kernel Object Ownership & Lifetime Contract

> Normative companion to `docs/architecture/source-of-truth.md`. It records, per
> resource, who owns it, how ownership is acquired/released, where it transfers,
> and the condition under which it is finally reclaimed. Every statement cites
> the current code; if the code changes, update this document in the same PR.

## Lock order

Canonical order (outermost first); inversions are checked by
`infra/lock_order.rs` (rank constants) and documented in `syscall/util.rs`:

```text
SCHEDULER -> USER_MEMORY_LOCK
          -> ZOMBIE_QUEUE / KSTACK_QUARANTINE        (siblings)
          -> OB_TABLE -> OB_NAMESPACE
          -> VFS -> MOUNT_MANAGER -> PAGE_CACHE -> BLOCK_DEVICES
          -> (transitively) buddy / slab allocator
```

`ZOMBIE_QUEUE`, `KSTACK_QUARANTINE` and `OB_TABLE` are never nested with each
other: the reaper drops the zombie lock before `recycle_terminated` takes the Ob
locks, and drains the kstack quarantine before locking the queue.

## Resource matrix

| Resource | Owner / initial reference | Acquire / release | Transfer | Final reclaim | Lock / CPU constraint |
|---|---|---|---|---|---|
| `ObObject` (`object/table.rs:37-46`) | `OB_TABLE`; `create` sets `refcount = 1` (`:94-121`) | `reference` / `dereference` (`:136-155`); anomaly counters `OB_REF_OVERFLOW`/`OB_REF_UNDERFLOW` | handle open (`ob_open_path`) adds refs | `take_for_destroy` validates `refcount <= 1` and unlinks (`:169-189`) | global `OB_TABLE` spin mutex |
| `HandleEntry` (`infra/handle.rs:13-21`) | per-`Eprocess` `HandleTable` | `close` → `ob_close_object` if `has_ob_object() && is_valid()` (`:105-110`) | fd install on create/open | slot reset to `closed()` (id 0); reused | mutated under `SCHEDULER` |
| Namespace entry (`object/namespace/types.rs:44-52`) | `OB_NAMESPACE`; stores a **raw** `ObId` (no ref) | insert on create; `ob_remove_by_id` on destroy | — | removed when the object is destroyed/closed | `OB_NAMESPACE` mutex |
| `SecurityDescriptor` (`object/table.rs:402-409`) | `OB_SECURITY` keyed by `ObId` | `ob_set_security` insert | — | removed in `take_for_destroy` (`:186`) | `OB_TABLE -> OB_SECURITY` |
| `Eprocess` (`scheduler/types.rs:278-300`) | `Scheduler.eprocesses[slot]`; `thread_count = 1` | — | slot move at create | `free_eprocess_resources` then `clear_eprocess_slot` (`lifecycle.rs:988`) | global `SCHEDULER` |
| `Kthread` (`scheduler/types.rs:178-242`) | `Scheduler.kthreads[slot]`; owns `kernel_stack` | — | dispatch moves runqueue ownership | `guard_kstack_reclaim` then `clear_kthread_slot` (`lifecycle.rs:982`) | `SCHEDULER`; KPRCB identity (Rule 6.1.5) |
| Kernel stack (`Box<AlignedKStack>`) | `Kthread.kernel_stack` | `take_kernel_stack` → `quarantine_push` | `guard_kstack_reclaim` on switch-out conflict (`lifecycle.rs:919-935`) | dropped, or freed by `drain_kstack_quarantine` when no `reclaim_conflict` (`:356-369`) | `SCHEDULER -> KSTACK_QUARANTINE`; `ACTIVE`/`KS` Release/Acquire (`diag/kstack.rs`) |
| User memory slot | `alloc_user_slot` (atomic claim + `SLOT_OWNER`) | `free_user_slot` (AlreadyFree FREE_BAD) | `user_slot.take()` in `free_eprocess_resources` | slot reusable | `SCHEDULER` / syscall |
| Process heap | `alloc_heap_slot`; frames via `heap_alloc_page` | `heap_free_range` / `free_heap_slot` | `free_eprocess_resources` resets `heap_base = 0` | idempotent range free | `SCHEDULER` |
| mmap region | `Eprocess.mmap_regions` VMA list | `mmap_free_range` (idempotent) | — | clears the VMA list | `SCHEDULER` / `USER_MEMORY_LOCK` |
| Pipe | `PIPE_MANAGER` buffer + `PIPE_OPS` | `inc/dec_read_ref` / `inc/dec_write_ref`; `free_pipe` at 0 | `on_destroy` → `free_pipe` | `free_pipe` resets the slot | `OB_TABLE` (callback runs **outside** the lock) |
| IRP | global pool (64) | `irp_alloc` / `irp_free` | callback dispatch | freed after the callback | pool lock |
| Physical frame | `memory/buddy.rs` | `alloc_frames` / `free_frames` | — | free lists | buddy mutex |
| Kernel heap object | `SlabAllocator` (+ fallback) | `GlobalAlloc` alloc/dealloc with FREE_BAD detection | — | dealloc | slab locks / per-CPU hot cache |

## Object destruction protocol (mandatory)

`ob_close_object` and `ob_destroy_object` MUST use the two-phase protocol:

1. Under `OB_TABLE`: validate (`RefCountHeld` if `refcount > 1`), then
   **unlink** the object (`take_for_destroy`) and drop its `OB_SECURITY` entry.
   Unlinking while the lock is held closes the finalize window, so a concurrent
   `reference`/`open` cannot resurrect the object and a second close/destroy
   cannot run `on_destroy` again.
2. Outside `OB_TABLE`: invoke `ObOperations::on_destroy`, then remove the
   namespace entry.

The callback MUST NOT run while holding `OB_TABLE` (some ops re-enter the Object
Manager, e.g. `PipeObOps::on_destroy -> PIPE_MANAGER.free_pipe -> ob_destroy_object`).

## Process/thread lifetime

- A terminated PID is enqueued in the `ZombieQueue` and reclaimed (whole
  process) by `recycle_terminated` only when no CPU runs it
  (`is_pid_running_on_any_cpu`) and its thread count is 0.
- The queue never evicts a pending record to satisfy `ZOMBIE_HARD_CAP`; overflow
  is retained and surfaced via `ZOMBIE_OVERFLOW`.
- PIDs and TIDs are monotonic and never reused (Rule 6.3.2), so no generation
  counter is required today.

## Known gaps / tracked follow-ups

- [`#670`](https://github.com/NeoDOS-Project/NeoDOS/issues/670): deterministic
  two-CPU interleaving harness for the reclamation/finalize windows.
- [`#666`](https://github.com/NeoDOS-Project/NeoDOS/issues/666): buddy allocator
  does not enforce INV-5 (double/invalid-free detection).
- [`#667`](https://github.com/NeoDOS-Project/NeoDOS/issues/667): lifecycle lock
  ranks registered, production lock-site instrumentation pending.
