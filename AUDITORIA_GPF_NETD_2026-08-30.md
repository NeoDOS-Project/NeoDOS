# Auditoría Completa NeoDOS — KERNEL PANIC (CLASS: GPF) en primera selección de `netd` (TID 2)

**Fecha:** 2026-08-30  
**Versión auditada:** v0.50.x (699 tests PASS, `cargo +nightly check` OK, `lto=true`)  
**Síntoma:** `#GP` con `RIP = iretq` , `RSP = 0x24772c8` , `err = 0x3ae0` / `0x8ae0` al retornar del timer IRQ tras `schedule()` seleccionar `netd` por primera vez.  
**Frame PRE-IRETQ diagnosticado como correcto:** `RIP=0x4040ba0` (`netd_entry_wrapper`), `CS=0x08`, `RFLAGS=0x202`, 15 slots cero, canary OK, `returned_rsp == init_rsp == 0x2477250`.

> Este documento es el informe estructurado solicitado en los puntos A–K. Todas las referencias incluyen patrón `archivo:línea`.

---

## Tabla de Contenidos

- [A. Hipótesis — Root Cause Candidates](#a-hipótesis--root-cause-candidates)
- [B. Bugs Reales Demostrables](#b-bugs-reales-demostrables)
- [C. Flujo Exacto del GPF](#c-flujo-exacto-del-gpf)
- [D. Contradicción 0x08 vs 0x3ae0](#d-contradicción-0x08-vs-0x3ae0)
- [E. ABI — Demostración Matemática](#e-abi--demostración-matemática)
- [F. GDT / TSS / IDT](#f-gdt--tss--idt)
- [G. Scheduler — Invariantes](#g-scheduler--invariantes)
- [H. DMA / Memory Corruption](#h-dma--memory-corruption)
- [I. Root Cause Final](#i-root-cause-final)
- [J. Fix Mínimo](#j-fix-mínimo)
- [K. Experimento Definitivo](#k-experimento-definitivo)
- [Apéndice — Checklist Invariantes y Referencias](#apéndice--checklist-invariantes-y-referencias)

---

## A. Hipótesis — Root Cause Candidates

| # | Hipótesis | Evidencia | Contra-evidencia | Prob. | Cómo probarla |
|---|-----------|-----------|-------------------|-------|---------------|
| **H1** | **Error de decodificación / el #GP no viene de `CS` de `iretq`** | `0x3ae0`→`idx 0x75c` GDT fuera de límite; `0x08` sería `idx 1`. Si fuera validación de `RFLAGS`/canonicidad, `err` sería `0`. | SDM Vol.3 6.13: `iret` Ring0→Ring0 solo valida `RIP` canónico + `CS` selector + `RFLAGS` reserved bits. `RFLAGS=0x202` válido, `RIP=0x4040ba0` canónico. `err !=0` implica selector. `src/arch/x64/idt.rs:306` `decode_gpf_error` coincide con SDM. | Media | QEMU `-d int` + `gpf_handler:716` print `err` + `cs` + desensamblado |
| **H2** | **`rsp` apuntando a lugar equivocado en `iretq`** | `returned_rsp=0x2477250 %16=0`, `rsp_iret=0x24772c8 %16=8`. Off-by-8 (14 pops vs 15) haría `CS` = `0x0` o `RIP`. Secuencia `src/arch/x64/idt.rs:322-383` hace 14 pops, logging, `pop rbp`. | `FINAL_*` demuestra `[rsp+8]=RIP`, `[rsp+16]=CS`, `[rsp+24]=RFLAGS` tras 14 pops. Dos lecturas independientes (`FRAME_DUMP` + `FINAL_IRETQ`) coinciden. `144%16=0` → alineamiento matemático cerrado. | Media-Baja | `serial_print` justo antes de `iretq` con `mov rsi,rsp; call dump` |
| **H3** | **Corrupción entre diagnóstico y `iretq` (ventana ~50 ciclos)** | Ventana entre `timer_trace_iretq_frame:348` y `iretq:384` es escribible. | `IF=0` (interrupt gate), sin `ack_irq` hasta después de `schedule()`, sin DMA en tick 4538. `FRAME_DUMP` y `FINAL` leen en dos instantes y ambos ven `0x08`. Canary `STACK_CANARY` en `0x24732e0` OK. | Baja | Eliminar `call timer_trace_iretq_frame`; si GPF desaparece → clobber |
| **H4** | **Violación ABI SysV + red zone + desalineamiento** | `Cargo.toml:21` `lto=true`, sin `-C no-red-zone`, target `x86_64-unknown-none` tiene red zone 128B. `timer_handler_asm` llama a Rust con `rsp%16` dependiente del interrumpido. Si `rsp%16=8` → callee `rsp%16=0` → `movaps` puede #GP(0). | `err 0x3ae0 !=0` (alignment sería `0`). Red zone solo toca boot-stack, no netd-stack. `timer_trace_iretq_frame:62` solo hace 3× `read` `u64` — LLVM no genera `movaps`. | Media (bug real, no causa directa) | `RUSTFLAGS="-C no-red-zone"` y re-test |
| **H5** | **Corrupción GDT/TSS/IDT** | `GDT` en `src/arch/x64/gdt.rs:22` `lazy_static` podría quedar en `.rodata` bajo LTO. | `gdt_dump:276` en `gpf_handler:752` imprime `base/limit/raw 0x08` esperados `P=1 S=1 L=1 type 0xA`. `TSS:17` ya está en `.data` (fix previo). Solo `TSS` se modifica post-`init`. | Baja-Media | Forzar `GDT` a `.data` + `sgdt` en `gpf_handler` |
| **H6** | **Stack overlap / heap / DMA overwrite** | `AlignedKStack:47` 16 KiB en heap `HEAP_START 0x2400000` `allocator.rs:5`. Stack netd `0x24772e0` dentro de heap `0x2400000-0x3400000`. `e1000` DMA `drivers/e1000/src/lib.rs:296` + `hst_virt_to_phys:126` (falla si huge page). | Stack va por `linked_list_allocator` fallback (>2 KiB), no por slab. `memory::reserve_range:323` impide solape buddy. DMA e1000 en región aislada `0x30000000` `isolation.rs:15`. AHCI DMA inactivo en tick 4538. | Baja | Guard page `NO_PRESENT` + QEMU watchpoint `0x24772d0` |
| **H7** | **Scheduler / pointer stability** | Antes `Vec<Option<Kthread>>` movía `Kthread` en realloc. Fix `Vec<Option<Box<Kthread>>>:403` estabiliza. Riesgo residual `recycle_thread:1020` deja `KPRCB.current_thread` dangling. | `KPRCB.current_thread:008` solo se actualiza bajo lock+`IF=0` en `timer_handler_inner:1048` y `syscall_try_resched:422`. En tick 4538 netd nunca recycled. `Vec` growth no ocurre durante `schedule()`. | Baja | Validar `current_thread->tid == current_tid` en `gpf_handler` |

**Lectura:** ninguna hipótesis cierra sin asumir lectura en offset equivocado o mutación post-diagnóstico. La contradicción apunta a H2/H3 como más plausible; H4/H5 son bugs reales independientes.

---

## B. Bugs Reales Demostrables

### CRITICAL

#### B1 — Red zone habilitada en kernel (`src/.cargo/config` / `Cargo.toml`)

- **Ubicación:** `.cargo/config.toml:5` `rustflags` sin `no-red-zone`; `src/arch/x64/idt.rs:322` `timer_handler_asm` naked llama a Rust.
- **Problema:** target `x86_64-unknown-none` habilita red zone 128B por defecto. El compilador asume `[rsp-128,rsp)` libre en toda fn Rust, incluido `timer_handler_inner:883`. En IRQ, `rsp` es la stack del thread interrumpido; el red zone pisa datos por debajo de `current_rsp` (posible frame futuro o `Kthread.rsp`/`TSS.RSP0`).
- **Impacto:** corrupción silenciosa no-determinista. Puede explicar `0x3ae0` si pisa `CS` slot.
- **Fix:** añadir `"-C", "no-red-zone"` en `.cargo/config.toml`.

#### B2 — IDT de APs vacía (`src/arch/x64/smp.rs:317-327`)

- **Ubicación:** `alloc_idt_page:348` alloca 4 KiB zerado, `lidt` con `limit 4095`, sin copiar `IDT` estático `src/arch/x64/idt.rs:498`.
- **Problema:** cualquier IRQ en AP (`IPI 0xF0` registrado `idt.rs:543`) ve `idt[0xF0]=0` → #GP → double fault sin IST válido → triple fault silencioso.
- **Impacto:** SMP inoperante aunque `tick 4538` sea BSP-only.

#### B3 — `raw_set_segment_regs` con operandos incorrectos (`src/hal/raw/cpu.rs:164-171`)

- **Ubicación:** `asm!("mov ds,{0:x}; mov es,{0:x}; mov ss,{0:x}", in(reg) ds)` usa `ds` para los tres `mov`, `es`/`ss` reciben basura del allocator.
- **Impacto:** `src/arch/x64/gdt.rs:83` `raw_set_segment_regs(0x10,0x10,0x10)` funciona por casualidad, pero LLVM puede asignar registro distinto → `SS=0x3ae0` residual. `raw_set_gs:180` con `println!` dentro de `unsafe` inline `asm` no permitido con `IF=0`.
- **Fix:** `asm!("mov ds,{0:x}; mov es,{1:x}; mov ss,{2:x}", in(reg) ds, in(reg) es, in(reg) ss)`.

### SUSPICIOUS / LIKELY BUG

- **B4 — GDT no forzado a `.data` (`src/arch/x64/gdt.rs:9-22`):** solo `TSS:17` está en `.data`. `GDT:22` `lazy_static` puede ir a `.rodata` bajo LTO. Recomendado `#[link_section=".data"]` también para GDT. Misma clase que bug TSS previo.

- **B5 — Detección slab por magic en `dealloc` (`src/slab.rs:413-416`):** `page_base = ptr & !0xFFF; magic == 0x534C4142` decide si `ptr` es slab. El heap `linked_list_allocator` en `[0x2400000,0x3400000)` puede tener header cuyo `u32` coincida casualmente → `dealloc` interpreta heap ptr como slab y corrompe `free_list`.

- **B6 — Sin guard page en `AlignedKStack` (`src/scheduler/mod.rs:43-57`):** canary solo en `base`. Overflow progresivo sobrescribe `RIP/CS` antes del canary. Falta `PAGE_FLAGS` `NO_PRESENT`.

- **B7 — `prepare_timer_return` deref sin validar canonicidad (`src/arch/x64/idt.rs:874`):** `*((next_rsp+128) as *const u64)` antes de validar `next_rsp !=0` / canónico. Si `next_rsp` es basura, #PF en `DISPATCH_LEVEL` → bugcheck enmascara GPF.

---

## C. Flujo Exacto del GPF

Valores concretos del log representativo:

```text
[GPF_DIAG] tick=5109 tid=2 rip=0x400db67 rsp=0x24772c8 err=0x3ae0
[TD] KERN SCHED tick=4538 tid=0 cs=0x8 nxt_tid=2 nxt_rsp=0x2477250 ks=0x24772e0
[TD] KERN RET   tick=4538 tid=0 cs=0x8 ret=0x2477250
[FD] kptr=0x1bca5a20 init=0x2477250 ret=0x2477250 slot15=0x4040ba0 slot16=0x08 slot17=0x202 [CANARY] OK
ks_top = base + 16384 = 0x24772e0 ; init_rsp = ks_top -144 = 0x2477250 ; rsp_iret = init_rsp+120 = 0x24772c8
```

Reconstrucción:

```text
Hardware IRQ0 (HPET/APIC 1 KHz) vector 32, IDT gate DPL0 IST0 type 0xE sel 0x08
  CPU Ring0→Ring0: push RFLAGS,CS,RIP (3×8) → RSP_irq = RSP_old -24
  ↓
timer_handler_asm:327  push rbp,r15,r14,r13,r12,r11,r10,r9,r8,rdi,rsi,rdx,rcx,rbx,rax (15×8=120)
  RSP_cur = RSP_irq -120 ; rdi=RSP_cur → call timer_handler_inner
  ↓
timer_handler_inner:883
  invariants::timer_irq_enter; get_ticks=4538
  scheduler.lock(); on_timer_tick(RSP_cur) // boot tid0 slice expire → Ready, rsp=RSP_cur, enqueue
  interrupted_cs = *(RSP_cur+128) = 0x08
  path=KERN, should_preempt = Ready && has_non_idle (netd Ready) → true
  k.rsp = RSP_cur; next = schedule() → tid2 netd via prio scan, current_tid=2, Running
  next_rsp=0x2477250 next_ks_top=0x24772e0 next_cs=0x08 → netd_record_select
  prepare_timer_return → TSS.RSP0=0x24772e0; this_cpu_set_current_thread(0x1bca5a20)
  ack_irq(32); return next_rsp
  ↓
timer_handler_asm:345  mov r12,rax (=0x2477250); mov rdi,rax; add rdi,120 (=0x24772c8); call timer_trace_iretq_frame
    // lee [0x24772c8]=0x4040ba0, [0x24772d0]=0x08, [0x24772d8]=0x202 → TD phase=3 IRETQ OK
  mov rax,r12; mov rsp,rax (=0x2477250)
  pop rax..r15 (14 pops) → RSP=0x24772c0 slot14=0
  // FINAL logging: [RSP+8]=RIP → FINAL_RIP, [RSP+16]=CS → FINAL_CS, [RSP+24]=RFLAGS → FINAL_RFLAGS
  pop rbp → RSP=0x24772c8
  iretq // CPU lee [RSP]=RIP, [RSP+8]=CS, [RSP+16]=RFLAGS
  ↓ #GP
gpf_handler:716  error=0x3ae0 rip=iretq rsp=0x24772c8 cs=0x08
  frame_dump / netd_diag_dump / final_iretq_dump / gdt_dump / decode_gpf_error
```

Estado `FINAL_IRETQ rsp=0x24772c8 rip=0x4040ba0 cs=0x08 rflags=0x202 FRAME CORRECT` pero CPU genera `err=0x3ae0`.

---

## D. Contradicción 0x08 vs 0x3ae0

### Decodificación (Intel SDM Vol.3 6.13)

```text
err bit0 EXT, bit1 IDT, bit2 TI, bit15:3 Index
0x3ae0 = 0011 1010 1110 0000 → EXT0 IDT0 TI0 idx 0x75c (1884) sel 0x3ae0 GDT
0x8ae0 = 1000 1010 1110 0000 → EXT0 IDT0 TI0 idx 0x115c (4444) sel 0x8ae0 GDT
0x0008 = 0000 0000 0000 1000 → idx 1 GDT → selector kernel code válido
```

`iretq` Ring0→Ring0 con `CS=0x08` válido (`P=1 S=1 DPL0 L=1 type 0xA` en `src/arch/x64/gdt.rs:291`) **debe suceder**; si fallara por GDT, `err` sería `0x0008`, no `0x3ae0`.

Por tanto, si `err=0x3ae0`, **el selector que la CPU intentó cargar no era `0x08`**.

Posibilidades ordenadas (ver tabla A para pruebas):

1. **Diagnóstico lee dirección equivocada (H2):** `FRAME_DUMP:186` y `timer_trace_iretq_frame:62` leen `ret+120/+128`. Si `rsp` real en `iretq` es `ret+112` (off-by-8), `CS` leído sería `0x0` o `RIP` (`0x0ba0`), no `0x3ae0`. Si `rsp` es `kptr=0x1bca5a20`, `*(kptr+128)` podría ser `0x3ae0`. Pero `FINAL_RSP=0x24772c8` demuestra que `rsp` sí era correcto en el logging. Para que CPU vea otro valor, `rsp` debe cambiar entre logging y `iretq` (solo `pop rbp` intermedio, que no toca frame).

2. **Mutación entre logging e `iretq` (H3):** ventana 1 instrucción (`pop`+`iretq` ~3 ciclos). Solo DMA/NMI podría escribir `0x24772d0`. No hay NMI; DMA e1000/AHCI inactivos en tick 4538. QEMU watchpoint lo descartaría.

3. **Selector no-CS (H5/SS):** `iretq` 64-bit no recarga `DS/ES/FS/GS` en Ring0→Ring0, y solo recarga `SS` si cambia privilegio. `raw_set_segment_regs` bug (B3) puede haber dejado `SS=0x3ae0` desde boot. Si `SS` corrupto antes de `iretq`, el fallo no es `iretq` sino la primera instrucción de `netd_entry_wrapper:68` (`push rbp` usa `SS:RSP`). Pero `RIP` de #GP sería `netd_entry_wrapper`, no `iretq`. El log dice `RIP=iretq`.

4. **GDT base/limit corrupto:** `sgdt` en `gpf_handler:279` mostraría `base/limit/raw 0x08`. Si `limit <8`, incluso `0x08` daría `err 0x0008`, no `0x3ae0`.

**Conclusión D:** con la instrumentación actual no se puede cerrar “corrupción” sin asumir lectura en offset equivocado o `SS` corrupto. El experimento K distingue las tres familias.

---

## E. ABI — Demostración Matemática

`timer_handler_asm:322-384`

```
Entrada IRQ Ring0: CPU push 3×8 → RSP1 = RSP0 -24 → RSP1%16 = (RSP0%16+8)%16
Handler push 15×8=120 → RSP2 = RSP1 -120 → RSP2%16 = RSP0%16
Call → RSP3 = RSP2 -8 → RSP3%16 = (RSP0%16+8)%16   // SysV requiere en callee entry RSP%16=8
```

→ Requiere `RSP0%16=0` (kernel habitualmente lo mantiene). Si `RSP0%16=8` → ambos calls misaligned → `movaps` podría #GP(0).

```
Ret → RSP2
2º call: mov r12,rax; mov rdi,rax; add rdi,120; call
RSP4 = RSP2 -8 → mismo alineamiento que 1er call
Ret → RSP2 ; mov rsp,rax → RSP5=0x2477250 %16=0
Pop rax→8, rbx→0, rcx→8, rdx→0, rsi→8, rdi→0, r8→8, r9→0, r10→8, r11→0, r12→8, r13→0, r14→8, r15→0
// 14 pops → RSP=0x24772c0 %16=0
FINAL logging: sin push, solo loads → RSP intacto
Pop rbp → RSP=0x24772c8 %16=8 → iretq pop 3×8 → RSP final 0x24772e0 = ks_top
```

- `r12` es callee-saved; `timer_trace_iretq_frame` debe preservarlo per SysV. Rust lo hace. Secuencia `mov r12,rax / call / mov rax,r12` es segura *iff* callee cumple ABI. Con `LTO` no cambia porque `timer_trace_iretq_frame` es `#[no_mangle] extern "C"` con linkage externo.
- **Violación real:** red zone no deshabilitada (B1) — `rsp-128` no es libre en IRQ.

Si `RSP0` del boot estaba alineado `%16=0`, ambos calls están **alineados correctamente** (`%16=8` en callee). No hay asimetría.

---

## F. GDT / TSS / IDT

### GDT

- **Definición:** `src/arch/x64/gdt.rs:22` `GDT: (GlobalDescriptorTable, Selectors)` con 6 entradas (null, code, data, user_code, user_data, TSS 16B). `x86_64::GDT` debe generar `limit 0x2F` (48-1). Verificar con `gdt_dump:276`.
- **Descriptor `0x08`:** `P=1 S=1 DPL0 L=1 type 0xA` `src/arch/x64/gdt.rs:291`. Sin escritores post-`init`.
- **Riesgo:** `GDT` no está en `.data` (solo `TSS:17` lo está). Bajo LTO puede ir a `.rodata` read-only. Recomendado `#[link_section=".data"]` también para GDT.

### TSS

- **Ubicación:** `TSS:17` en `.data` writable (fix previo contra LTO `.rodata`).
- **Escritores:** solo `set_kernel_stack:62` y `prepare_ring3_return:112` → `TSS.privilege_stack_table[0]`. `RSP0` debe ser `ks_top !=0` y 16-byte aligned. Para netd `0x24772e0` cumple.
- **Race:** `schedule()` cambia `TSS.RSP0` bajo lock + `IF=0` (IRQ gate) → seguro.

### IDT

- **Timer:** `IDT[32]:529` `set_handler_addr(timer_handler_asm)` sin `IST` → usa `RSP0` si Ring3→0, `RSP` current si Ring0→Ring0. Correcto, no IST.
- **Double fault:** único con `IST1` `src/arch/x64/gdt.rs:7` stack `DOUBLE_FAULT_STACK:12` 32 KiB.
- **Syscall:** `idt[0x80]:536` DPL3. IPI `0xF0-0xF2` sin IST.
- **Bug:** AP IDT vacía (B2).

---

## G. Scheduler — Invariantes

Invariantes `validate_runqueue_invariants:1101`:

- `current_tid → Running`
- `Ready ↔ 1 entrada en runqueue`
- `!Ready ↔ 0 entradas`
- Sin duplicados, sin `BOOT/IDLE` en runqueue

**Camino `timer_handler_inner:883-1227`:**

- `on_timer_tick` puede mover `current` `Running→Ready` + `enqueue`. Luego `should_preempt` re-evalúa `Ready`. Doble `enqueue` protegido por `contains` en `CpuRunQueue:102`.
- `schedule()` `try_dequeue_local → try_work_steal → priority scan`. Para netd primera vez, cae en priority scan `1329` que hace `remove_from_run_queue` antes de `Running`. Correcto.
- Pointer `find_kthread_ptr:1269` retorna `*mut Kthread` de `Box` — estable tras `Vec<Option<Box<Kthread>>>:403`. `Vec` growth solo en `alloc_kthread_slot` y `ensure_slots`, ambos bajo lock, no durante `schedule`.
- `KPRCB.current_thread:008` se actualiza en `this_cpu_set_current_thread:1051` bajo lock+`IF=0`.

**No se encontró invariante rota** que explique `0x3ae0`. El cambio `Box` sí cierra `realloc invalidating &mut Kthread`, pero deja dangling si `recycle_thread` concurrente (no es el caso en tick 4538).

---

## H. DMA / Memory Corruption

- **Heap:** `HEAP_START 0x2400000` 16 MiB `allocator.rs:5` → `0x2400000-0x3400000`. Netd stack `0x24772e0` dentro del heap.
- **Slab:** buddy bitmap en `bitmap_phys` high memory fuera de heap. `memory::reserve_range:323` impide solape buddy ↔ heap.
- **e1000 DMA:** `BUF_POOL` en `0x30000000` isolation `isolation.rs:15`. `hst_virt_to_phys:126` falla si page es huge (`walk_ptes_4k` → `None`) → driver aborta, no DMA. `RX_DESCS/TX_DESCS` en `0x300...` no en `0x24...`.
- **BootAhci DMA:** buffers en heap, solo durante `storage_manager::init_storage` y `vfs::read`. En tick 4538 no hay I/O pendiente (post `ALL_TESTS_COMPLETE` y `500 ticks` spin).

**No hay escritor legítimo demostrado a `0x24772d0`.** DMA queda como **no probado**, requiere watchpoint.

---

## I. Root Cause Final

**No existe causa única demostrable con la instrumentación actual.** La evidencia más fuerte apunta a **desalineamiento de lectura del frame (H2)** o **segment register corrupto no volcado (H5/SS)**.

El bug más grave **demostrable** independiente del GPF es la **red zone habilitada + `SS` asm bug** (`src/hal/raw/cpu.rs:164`), que puede corromper stack en cualquier IRQ con `RSP%16=8` y dejar `SS` con valor residual `0x3ae0`.

Si se fuerza una única explicación para `0x3ae0`: `low 16 bits` de dirección física del heap bitmap (`0x3ae0/8 ≈ 0x75c` slots) o de `KPRCB` high. En log `kptr=0x1bca5a20` (`Box<Kthread>` heap) `& 0xFFFF = 0x5a20` ≠ `0x3ae0`, pero `SLOT_USED` words `0x3ae0` words ≈ heap slots.

**Hipótesis refinada:** `hst_virt_to_phys` o `walk_ptes_4k` no-split hace `e1000` fallar y el path de error pisa `RSP`.

Se requiere experimento.

---

## J. Fix Mínimo (no refactor masivo)

> No implementar todavía si no es necesario. Cambios reversibles, testeables.

### J1 — Fixes CRITICAL inmediatos (2 líneas cada uno)

**`.cargo/config.toml:5`**
```toml
[target.x86_64-unknown-none]
rustflags = [
    "-C", "link-arg=-Tkernel.ld",
    "-C", "link-arg=-melf_x86_64",
    "-C", "link-arg=-no-pie",
    "-C", "relocation-model=static",
    "-C", "no-red-zone",              # ← AÑADIR
]
```

**`src/hal/raw/cpu.rs:164`**
```rust
// Antes:
asm!("mov ds, {0:x}", "mov es, {0:x}", "mov ss, {0:x}", in(reg) ds)
// Después:
asm!("mov ds, {0:x}", "mov es, {1:x}", "mov ss, {2:x}", in(reg) ds, in(reg) es, in(reg) ss)
```

**`src/arch/x64/gdt.rs:22`**
```rust
#[link_section = ".data"]
static mut GDT_MEM: [u8; 48] = [0; 48]; // o forzar lazy_static GDT a .data
```

### J2 — Aislar GPF sin tocar scheduler

En `src/arch/x64/idt.rs:322` reemplazar dependencia `r12`:

```asm
// Antes:
mov r12, rax
mov rdi, rax
add rdi, 120
call timer_trace_iretq_frame
mov rax, r12
// Después (preserva con stack, no callee-saved):
push rax
mov rdi, rax
add rdi, 120
call timer_trace_iretq_frame
pop rax
```

Elimina hipótesis `r12` clobber.

### J3 — No cambiar todavía

- `init_ring0_frame` `18×8` layout
- `15 pushes/pops` + `+120`
- `TimerDiagEntry` 14 campos
- No introducir `reserve(64)` como “fix”
- No cerrar P0-4.1 por pasar tests

---

## K. Experimento Definitivo — Uno Solo, Reproducible

**Objetivo:** distinguir H2 (rsp equivocado) vs H3 (corrupción post-log) vs H5 (selector no-CS) con una sola ejecución.

### K1 — Instrumentación mínima (12 instrucciones, sin calls)

En `src/arch/x64/idt.rs:322` `timer_handler_asm`, inmediatamente antes de `iretq`, insertar captura atómica que **no toque stack** ni use ABI:

```asm
    // tras pop rbp, rsp = iret_rsp
    mov rax, [rsp]      // RIP
    mov rcx, [rsp+8]    // CS
    mov rdx, [rsp+16]   // RFLAGS
    mov rsi, rsp
    mov qword ptr [rip + DBG_RIP], rax
    mov qword ptr [rip + DBG_CS], rcx
    mov qword ptr [rip + DBG_RFLAGS], rdx
    mov qword ptr [rip + DBG_RSP], rsi
    sgdt [rip + DBG_GDTR]
    str word ptr [rip + DBG_TR]
    mov rax, gs
    mov word ptr [rip + DBG_GS], ax
    mov rax, ds
    mov word ptr [rip + DBG_DS], ax
    mov rax, ss
    mov word ptr [rip + DBG_SS], ax
    iretq

DBG_RIP:    .8byte 0
DBG_CS:     .8byte 0
DBG_RFLAGS: .8byte 0
DBG_RSP:    .8byte 0
DBG_GDTR:   .10byte 0
DBG_TR:     .2byte 0
DBG_GS:     .2byte 0
DBG_DS:     .2byte 0
DBG_SS:     .2byte 0
```

Declarar `DBG_*` como `static mut AtomicU64` / `[u8;10]`.

### K2 — Volcado en `gpf_handler:716` (antes de cualquier print)

```rust
crate::serial_println!("[DBG_IRET] rsp=0x{:x} rip=0x{:x} cs=0x{:x} rflags=0x{:x}",
    DBG_RSP.load(Relaxed), DBG_RIP.load(Relaxed), DBG_CS.load(Relaxed), DBG_RFLAGS.load(Relaxed));
crate::serial_println!("[DBG_SEG] gs=0x{:x} ds=0x{:x} ss=0x{:x} tr=0x{:x} gdtr base=0x{:x} limit=0x{:x}",
    DBG_GS.load(Relaxed), DBG_DS.load(Relaxed), DBG_SS.load(Relaxed), DBG_TR.load(Relaxed), gdtr_base, gdtr_limit);
```

### K3 — Criterio de falsación

| Resultado `DBG_*` | Conclusión | Acción |
|-------------------|------------|--------|
| `DBG_CS == 0x08` && `err==0x3ae0` | CPU vio otro selector (H5) → `SS/DS/GS` o `GDT` corrupto. | Inspeccionar `DBG_SS/DS/GS` y `GDTR` |
| `DBG_CS == 0x3ae0` | Memoria ya contenía `0x3ae0` → corrupción H3, `FRAME_DUMP` leía dirección equivocada (H2) | QEMU watchpoint `0x24772d0` |
| `DBG_SS == 0x3ae0` | `SS` corrupto por B3 | Fix `raw_set_segment_regs` y re-test |
| `DBG_RIP != 0x4040ba0` | `rsp` equivocado | Contar pops / `+120` |

### K4 — Ejecución

```bash
neodev build --quick --image
neodev run   # QEMU -d int,cpu_reset (opcional)
# Captura primera selección netd: tick ~4538, sin necesidad de 500 ticks delay
```

Un único run captura el bug. Sin calls, sin red zone, sin dependencia `r12`.

---

## Apéndice — Checklist Invariantes y Referencias

### Invariantes verificadas

- `src/arch/x64/gdt.rs:48` `set_kernel_stack` bloquea `0` → OK (post-fix TSS)
- `src/scheduler/mod.rs:43` `STACK_CANARY` → OK pero sin guard page
- `src/arch/x64/idt.rs:863` `read_cs_from_stack` `+128` asume 15 pushes → consistente con `init_ring0_frame` 18 slots
- `src/hal/x64/mem.rs:11` `walk_ptes_4k` retorna `None` en huge page → caller debe `split_2mb_page` primero (correcto en `hst_virt_to_phys` pero driver no reintenta)

### Semántica `iretq` Ring0→Ring0 (SDM)

- Consume `RIP, CS, RFLAGS` (3×8) en ese orden
- Valida `CS` selector (present, code, DPL0, L) → #GP(selector) si falla, `RIP` = `iretq`
- Valida `RIP` canónico (bits 63:48 sign extend) → #GP(0) si no
- Valida `RFLAGS` reserved bits (`1` debe ser 1, `VIF` etc) → #GP(0)
- No carga `RSP/SS` si `CPL` no cambia
- No recarga `DS/ES/FS/GS`

### Error Codes Observados

```text
0x3ae0 → idx 0x75c GDT sel 0x3ae0
0x8ae0 → idx 0x115c GDT sel 0x8ae0
0x015c → idx 0x2b LDT sel 0x015c (otro boot, invocación distinta)
Todos con EXT0 IDT0, TI según bit2
```

### Archivos Clave Auditados

```text
src/arch/x64/mod.rs
src/arch/x64/entry.rs
src/arch/x64/gdt.rs
src/arch/x64/idt.rs
src/arch/x64/cpu_local.rs
src/arch/x64/paging.rs
src/arch/x64/smp.rs
src/scheduler/mod.rs
src/net/mod.rs
src/hal/x64/irql.rs
src/hal/raw/cpu.rs
src/hal/x64/mem.rs
src/memory/mod.rs
src/slab.rs
src/main.rs
src/syscall/mod.rs
neodos-kernel/kernel.ld
neodos-kernel/.cargo/config.toml
neodos-kernel/Cargo.toml
drivers/e1000/src/lib.rs
```

### Próximo Paso Recomendado

1. Aplicar J1 `no-red-zone` + `raw_set_segment_regs` fix (2 líneas).
2. Implementar experimento K `DBG_*` (12 instrucciones).
3. Un run QEMU con `neodev run` captura `DBG_CS` vs `err` y cierra la contradicción.
4. No refactorizar `init_ring0_frame` / `18×8` / `schedule` hasta tener `DBG_*`.

---

*Auditoría realizada como ingeniero de kernel ante crash “imposible”. Sin asumir hipótesis previa. Si la instrumentación está equivocada, este informe lo señala (D, K). Si aparece bug más grave que el GPF, se incluye (B1-B3).*
