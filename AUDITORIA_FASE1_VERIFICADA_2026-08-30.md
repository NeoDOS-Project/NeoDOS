# NeoDOS — Auditoría Forense Fase 1: GPF en `iretq` al seleccionar `netd` (TID 2) — Verificada contra código y binario

**Fecha:** 2026-08-30  
**Versión auditada:** v0.50.x, `neodos-kernel/target/x86_64-unknown-none/release/neodos_kernel` ELF 64-bit `0x4000000`, `lto=true`  
**Síntoma:** `#GP err=0x3ae0/0x8ae0 RIP=iretq (0x400dbc9) RSP=0x24772c8` al retornar de `timer_handler_asm` tras `schedule()` seleccionar `netd` por primera vez. `FINAL_*` reporta `RIP=0x4078230 CS=0x08 RFLAGS=0x202` FRAME CORRECT.  
**Método:** reconstrucción instrucción-por-instrucción contra `objdump -drwC` + `nm` + `readelf` + `rustc --print target-spec-json`, sin asumir auditoría previa.

---

## A. Hallazgos confirmados

### A1 — `timer_handler_asm` `src/arch/x64/idt.rs:322-385` vs `0x400db1c:400dbc9` verificado

```
HW Ring0→Ring0: push RFLAGS,CS,RIP (3×8)  RSP_irq = RSP_old-24
push rbp,r15,r14,r13,r12,r11,r10,r9,r8,rdi,rsi,rdx,rcx,rbx,rax  15×8=120  RSP2=RSP_irq-120
mov rdi,rsp ; call timer_handler_inner  (RSP2-8 → RSP2)
mov r12,rax ; mov rdi,rax ; add rdi,0x78 ; call timer_trace_iretq_frame  (lee [RDI]=RIP [RDI+8]=CS)
mov rax,r12 ; mov rsp,rax (=next_rsp 0x2477250)
pop rax,rbx,rcx,rdx,rsi,rdi,r8,r9,r10,r11,r12,r13,r14,r15  14×8  RSP=next_rsp+112
lea rbp,[rsp+8] ; mov [rip+FINAL_RSP],rbp   // 0x24772c8
mov rbp,[rsp+8]  ; mov [rip+FINAL_RIP],rbp  // RIP
mov rbp,[rsp+16] ; mov [rip+FINAL_CS],rbp   // 0x08
mov rbp,[rsp+24] ; mov [rip+FINAL_RFLAGS],rbp
mov rbp,ds/es/fs/gs/ss ; mov [rip+FINAL_*],rbp  // sin tocar RSP
pop rbp  // consume slot14 (0)  RSP=next_rsp+120 = 0x24772c8
iretq    // lee [RSP]=RIP [RSP+8]=CS [RSP+16]=RFLAGS
```

* 14 pops + `pop rbp` = 15 restaura exacto. `FINAL_CS` lee `[RSP+16]` que es misma dirección que `iretq` lee como `CS` (`[iret_rsp+8]`). Entre `FINAL_CS` y `iretq` solo `pop rbp` (slot14) — no modifica frame. `disp 0x221fbe` a `FINAL_*` (`.bss 0x422fb30`) verificado `<2 GiB`, `R_X86_64_PC32` correcto, sección `WA`.

### A2 — `init_ring0_frame` `src/scheduler/mod.rs:213-226`

```rust
sp = ks_top & !0xF; stack[-1]=0x202; stack[-2]=0x08; stack[-3]=entry; for j in 4..19 stack[-j]=0; sp-=144;
```

18 slots, lay-out `sp+0..112` = 15× GPR cero (`rax→rbp`), `+120`=RIP, `+128`=CS, `+136`=RFLAGS. `init = ks_top-144`, `iret = init+120 = ks_top-24`, `ks_top=init+144`. Con `ks_top 0x24772e0` → `init 0x2477250` → `iret 0x24772c8` coincide con log `base 0x24732e0 top 0x24772e0`. Canary `0xDEADBEEFCAFEBABE` `src/scheduler/mod.rs:52` OK.

### A3 — `#GP err` semántica SDM Vol.3 6.13

`err=EXT|IDT|TI|index` — `0x3ae0 = EXT0 IDT0 TI0 idx 0x75c GDT`, `0x8ae0 idx 0x115c`, `0x0008 idx1`. `iretq` Ring0→Ring0 solo valida `RIP` canónico, `CS` (present/code/DPL0/L) y `RFLAGS` reserved. `err≠0` ⇒ selector fallido es `CS`. `RFLAGS 0x202` válido, `RIP 0x4078230` canónico ⇒ `CS` es único origen. `iretq` no carga `SS/RSP` ni `DS/ES/FS/GS` a mismo privilegio.

### A4 — GDT/TSS/IDT

* `GDT` `lazy_static` en `.bss 0x4224010 0x60` (no `.rodata`), construido `src/arch/x64/gdt.rs:22-38` con `P=1 S=1 DPL0 L=1 type 0xA` para `0x08`. `gdt_dump:276` decodifica correcto. `GDTR.limit` esperado `0x2F`.
* `TSS` `#[link_section=".data"] 0x41763c0` `src/arch/x64/gdt.rs:17` writable, `TSS.RSP0` solo por `set_kernel_stack:62`/`prepare_ring3_return:112`/`prepare_timer_return:873`. En timer path `TSS.RSP0=0x24772e0 ≠0`.
* `IDT[32]` `src/arch/x64/idt.rs:529` `set_handler_addr(timer_handler_asm)` sin IST, tipo `0xE` `IF=0`.

### A5 — Red zone **deshabilitada** — corrección de auditoría previa

`rustc +nightly --print target-spec-json --target x86_64-unknown-none` → `"disable-redzone": true`. `neodos-kernel/.cargo/config.toml:5` sin `no-red-zone` es correcto (target ya lo deshabilita). Disassembly `timer_handler_inner 0x4058aa0: sub rsp,0x98` sin `[rsp-128]`. Hipótesis red-zone descartada (`err 0x3ae0≠0` además no sería `movaps`).

### A6 — Binario ELF

`.text 0x4000000 RX 0x1763a0` (incluye `.rodata*` por `kernel.ld:9`), `.data 0x41763c0 RW`, `.bss 0x421c000 NOBITS 0x2c97c`. `timer_handler_asm 0x400db1c iretq 0x400dbc9`, `netd_entry_wrapper 0x4078230` (`push rax; call netd_entry` — entry divergente `!`, no retorno), `FINAL_* 0x422faf8-0x422fb38 .bss WA`, distancia RIP-relative `~2.2 MB`.

### A7 — Scheduler / estabilidad

`Vec<Option<Box<Kthread>>>:403` puntero estable; `find_kthread_ptr:1269` retorna `&**Box`; `ensure_slots:761` pre-reserva fuera del lock; `schedule:1282` `remove_from_run_queue` antes de `Running`; `current_thread/pid` bajo `IrqMutex:DISPATCH` + `IF=0`.

---

## B. Hipótesis descartadas

| Hipótesis | Evidencia que la descarta |
|---|---|
| Red zone 128 B | `disable-redzone:true` + no `[rsp-128]` + `err≠0` |
| GDT en `.rodata` LTO | GDT en `.bss`, `TSS` en `.data`, `gdt_dump` OK |
| `r12` clobber | `r12` callee-saved, `timer_trace` solo 3× `read`, `FINAL` y `FRAME_DUMP` coinciden |
| `RSP` off-by-8 (14 vs 15 pops) | Matemática `144%16=0`, `FINAL_RSP=init+120`, `FINAL_RIP/CS` correctos |
| `err` es `SS/DS/GS` residual | `iretq` Ring0→Ring0 no carga `SS`/`DS`; `RIP` del `#GP` es `iretq` no `netd_entry_wrapper`; `FINAL_SS/DS=0x10` |
| `init_ring0_frame` mal alineado | `ks_top & !0xF`, `init%16=0`, `iret%16=8`, `init+144==ks_top` |
| DMA `0x30000000` o `e1000` pisa `0x24772d0` | Región DMA aislada, `walk_ptes_4k` huge-page aborta, ventana `IF=0` 1 instr `pop rbp`, sin I/O en tick 4538 |

---

## C. Hipótesis abiertas (ordenada)

1. **Alta — CPU lee selector físicamente distinto al leído por `FINAL_CS` pese a mismo `virt 0x24772d0`**: aliasing físico / TLB incoherente / PTE `HUGE_PAGE` no split (`walk_ptes_4k:11` retorna `None` si `HUGE_PAGE`). `0x3ae0` determinista sugiere contenido físico de metadata (heap/buddy) mapeado doble. Requiere `walk_ptes_4k(0x24772d0)` + `cr3` capture + QEMU watchpoint.
2. **Media — `GDTR` corrupto en instante `iretq` no visible post-`#GP`**: `gdt_dump` lee `sgdt` tras `cli` en `gpf_handler`, no en `iretq`. Si `base` corrupto, `idx1 (0x08)` podría decodificar descriptor de `idx 0x75c`. Requiere `sgdt` atómico pre-`iretq`.
3. **Media-baja — `KPRCB.GS` corrupto desvía `run_queue` y `schedule()` retorna stack stale**: `cpu_local.rs:588` usa `gs_read_u64(0)` como base; `raw_set_gs` bug previo podría; `DBG_GS` pre-`iretq` lo distingue.
4. **Baja — `prepare_timer_return:873` lee `next_rsp+128` sin validar canonicidad**: `next_rsp` basura → `#PF` en `DISPATCH_LEVEL` enmascara, pero `netd_record_select` ya validó `+128==0x08`.

---

## D. Bugs reales independientes

* **D1 (CRITICAL latente) `src/hal/raw/cpu.rs:164`** `asm!("mov ds,{0:x}; mov es,{0:x}; mov ss,{0:x}", in(reg) ds)` usa `ds` tres veces. Binario `4001201: 66 8e db / 66 8e c3 / 66 8e d3` es `mov ds,bx; mov es,bx; mov ss,bx` con `bx=0x10` — correcto solo porque `gdt::init:83` pasa `0x10` tres veces. Fix: `in(reg) ds, in(reg) es, in(reg) ss`.
* **D2 (CRITICAL SMP) `src/arch/x64/smp.rs:348` `alloc_idt_page`** 4 KiB zerado, `limit 4095`, sin copiar `IDT:498`. `IPI 0xF0` en AP → #GP → triple fault. Solo BSP operativo.
* **D3 `neodos-kernel/kernel.ld:9` `.rodata*` dentro de `.text`** — datos RX, viola W^X.
* **D4 `src/scheduler/mod.rs:47` sin guard page `NO_PRESENT`** — canary no atrapa overflow grande sobre `RIP/CS`.
* **D5 `src/slab.rs:413` magic `0x534C4142`** colisión con `linked_list_allocator` header.
* **D6 `src/hal/raw/cpu.rs:180` `raw_set_gs` con `println!` bajo `asm` + `IF=0`** — deadlock potencial.

---

## E. Reconstrucción exacta

```
IRQ 32 HPET/APIC → CPU push RFLAGS/CS/RIP (RSP_irq=RSP_old-24)
→ timer_handler_asm 15×push RSP_cur=RSP_irq-120 RDI=RSP_cur call timer_handler_inner
→ timer_handler_inner:883 get_ticks 4538, scheduler.lock, on_timer_tick(RSP_cur)=Ready, *(RSP_cur+128)=0x08 KERN should_preempt, k.rsp=RSP_cur, schedule()=tid2 prio-scan, next_rsp=0x2477250 TSS.RSP0=0x24772e0 current_thread=Box 0x1bca5a20 ack_irq return next_rsp
→ timer_handler_asm: mov r12,rax; mov rdi,rax+120=0x24772c8 call timer_trace_iretq_frame ([0x24772c8]=0x4078230 [0x24772d0]=0x08) mov rax,r12 mov rsp,rax pop×14 RSP=0x24772c0 FINAL_* (RIP/CS/RFLAGS/DS...) pop rbp RSP=0x24772c8 iretq ([RSP]=RIP [RSP+8]=CS) → #GP err 0x3ae0 RIP 0x400dbc9
→ gpf_handler:716 decode GPF err 0x3ae0 FINAL CORRECT vs CPU err 0x3ae0
```

---

## F. Primera inconsistencia

> `FINAL_CS == 0x08` en `[0x24772d0]` leído por `mov rbp,[rsp+16]` y `timer_trace_iretq_frame` **vs** `error_code == 0x3ae0` (selector GDT `idx 0x75c`) entregado por CPU para `iretq` en `RSP=0x24772c8`. No hay instrucción que modifique `[RSP+8]` entre ambas lecturas salvo `pop rbp` (slot14). `IF=0` excluye IRQ. Si memoria virtual fuese coherente, `err` debería ser `0x0008` o `0`. La máquina afirma dos valores distintos para misma dirección — primera violación de invariante arquitectónico.

---

## G. Experimento siguiente (uno solo)

**Archivo:** `neodos-kernel/src/arch/x64/idt.rs:322` `global_asm! timer_handler_asm`  
**Modificación** (12 instr, sin `call`/stack/lock, preserva `RSP`):

```asm
    // RSP=next_rsp+112 (tras 14 pops, antes de pop rbp)
    mov rax, [rsp+8]    // RIP
    mov rcx, [rsp+16]   // CS
    mov rdx, [rsp+24]   // RFLAGS
    mov rsi, rsp
    add rsi, 8          // iret_rsp
    mov [rip + DBG_RIP], rax
    mov [rip + DBG_CS], rcx
    mov [rip + DBG_RFLAGS], rdx
    mov [rip + DBG_RSP], rsi
    sgdt [rip + DBG_GDTR]
    str word ptr [rip + DBG_TR]
    mov rax, cr3
    mov [rip + DBG_CR3], rax
    mov ax, gs
    mov [rip + DBG_GS], ax
    pop rbp
    iretq
DBG_RIP: .8byte 0; DBG_CS: .8byte 0; DBG_RFLAGS: .8byte 0; DBG_RSP: .8byte 0
DBG_GDTR: .space 10,0; DBG_TR: .2byte 0; DBG_CR3: .8byte 0; DBG_GS: .2byte 0
```
`DBG_*` en `.bss` `# [no_mangle] static mut` → disp `~0x221fbe` verificado `<2 GiB` con `objdump -drwC`.

**Volcado** `src/arch/x64/idt.rs:716` `gpf_handler` primera línea:

```rust
crate::serial_println!("[DBG_IRET] rsp={:#x} rip={:#x} cs={:#x} rflags={:#x} cr3={:#x}", unsafe{DBG_RSP}, unsafe{DBG_RIP}, unsafe{DBG_CS}, unsafe{DBG_RFLAGS}, unsafe{DBG_CR3});
let mut g:[u8;10]=[0;10]; unsafe{core::arch::asm!("sgdt [{}]", in(reg) g.as_mut_ptr())};
crate::serial_println!("[DBG_GDT] pre base {:#x} lim {:#x} tr {:#x} gs {:#x} post {:#x}", u64::from_le_bytes(unsafe{DBG_GDTR}[2..10].try_into().unwrap()), u16::from_le_bytes(unsafe{DBG_GDTR}[0..2].try_into().unwrap()), unsafe{DBG_TR}, unsafe{DBG_GS}, u64::from_le_bytes([g[2],g[3],g[4],g[5],g[6],g[7],g[8],g[9]]));
```

**Comandos:**

```bash
neodev build --quick --image --neodos-path /home/amartinper/rust-os/neodos
timeout 90 neodev run --headless --serial /tmp/neodos-audit.serial --net user --neodos-path /home/amartinper/rust-os/neodos
grep -E "DBG_|GPF_DIAG|FINAL" /tmp/neodos-audit.serial
objdump -drwC target/x86_64-unknown-none/release/neodos_kernel | grep DBG_
```

**Interpretación:**

| `DBG_*` | Conclusión | Siguiente |
|---|---|---|
| `DBG_CS==0x08 && err==0x3ae0 && GDTR OK` | CPU leyó otro físico que `FINAL_CS` → aliasing/TLB (C1) | `walk_ptes_4k(0x24772d0)` + QEMU `info tlb` + watchpoint `0x24772d0` |
| `DBG_CS==0x3ae0` | Memoria ya `0x3ae0` → offset equivocado o corrupción previa | Corregir `+120` vs `init` |
| `DBG_GS/TR==0x3ae0` | SS/GS corrupto | Fix D1 |
| `DBG_RIP!=0x4078230` | RSP equivocado | Recalibrar 18 slots |

No aplicar `no-red-zone`/GDT/scheduler fixes hasta `DBG_CS` vs `err`.

---

## H. No aplicar fix definitivo

Ningún parche de D se aplica antes de `DBG_*`. El fix mínimo cuando se pruebe causalidad será solo `src/hal/raw/cpu.rs:164` (segment regs). `disable-redzone` ya es `true` en target.

---
*Auditoría verificada instrucción-a-instrucción. Si `DBG_CS` confirma `0x08`, la causa raíz es incoherencia entre vista virtual y carga de `iretq` (TLB/phys alias), no frame lógico.*
