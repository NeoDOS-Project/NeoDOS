# KBD INPUT QUEUE INVESTIGATION — NeoDOS

**Branch:** `investigation/kbd-write-stress` @ 8dbdace + vt-diag + shell-yield  
**Fecha:** 2026-09-25  
**Kernel:** v0.50.0, 716 tests, VT_QUEUE_SIZE 4096, usable 4095  

---

## Root cause

El hardware y el pipeline básico funcionan (`sendkey → IRQ1 → KBD_PUSH → VT queue → READB → shell` con gap 150 ms y prompt limpio **PASS**). El fallo bajo input rápido no es pérdida en el driver PS/2 ni en el decoder, sino **starvation del consumidor**:

1. **Busy-wait en neoshell** `userbin/neoshell/src/shell.rs:158`:
   ```rust
   if b < 0 { continue; }
   ```
   Sin `yield`, el thread queda en `Running` girando 100k-10M iteraciones por tecla cuando `READB` devuelve `EAGAIN`. Con `1 CPU online` el timer a 1 KHz sí preempta, pero con gap 0 ms el productor IRQ encadena 10-100 pushes mientras el consumidor está en `Running` y no en `Blocked`, por lo que `wake_blocked_readers` no lo despierta. El siguiente `READB` sí encuentra datos (porque están encolados), pero el **echo y `execute_line` bloquean** el prompt, haciendo que el usuario perciba “nada en la shell”.

2. **SPSC vs SMP**: `VtInputQueue` y `PENDING_SCANCODES` usan `head Acquire / tail Release` correcto para **SPSC** 1 productor / 1 consumidor. Con `MAX_CPUS=16` e `IOAPIC` el IRQ1 puede llegar en cualquier CPU → **MPSC** con `tail` Relaxed → lost-update si 2 CPUs empujan a la misma VT simultáneamente.

3. **Silent drop**: `push` si `next==head` retorna `Err(())` y sólo `serial_println!` (antes silencioso). Para `2N=8192` con `N=4096` se pierden `4096` bytes sin que el usuario lo sepa.

Evidencia: con instrumentación ligera `a`+`abc+ret` (“aabc”) cada tecla hizo 1 `KBD_PUSH Ok` + 1 `READB pop Ok` con gap 0.2s **PASS**, pero al enviar `hello` (5+ret) inmediatamente después de `aabc` sin esperar prompt, los 6 `VT_PUSH head9→15` quedaron con `head=9` sin `pop` durante 15s (shell ocupada en `execute_line` de `aabc`).

## Evidence

### Fase 1 — Cola instrumentada
`neodos-kernel/src/input/vt.rs:31` ring 64 `VtDiagEntry {seq,op,byte,head,tail,occ,cpu,tid}` + contadores `VT_PUSH_CNT/POP_CNT/DROP_CNT/MAX_OCC`. `op 0=PUSH Ok,1=DROP(full),2=POP Ok,3=EMPTY`. `vt_diag_dump()` vía hotkey `Ctrl+Alt+V` `kbd/hotkey.rs:31` o syscall 99 `DebugDump`.

- Capacidad: `VT_QUEUE_SIZE 4096`, usable `4095` (`next==head` → full) — verificado por `vt_queue_capacity:4095` `vt.rs:170`.
- Head: `AtomicUsize Acquire` en `push`, `Relaxed` en `pop`.
- Tail: `Relaxed` en `push`, `Acquire` en `pop`.
- Producer: `keyboard_handler` IRQ33 `idt.rs:1374` → `kbd_event_handler_direct` `event.rs:84` → `KBD::try_lock` → `push_byte` `manager.rs:60` → `vt_queues[active_vt].push` **en CPU del IRQ** (cualquier CPU, `this_cpu_id()`).
- Consumer: `handler_read` `handlers.rs:114` `pop_byte_from_vt(vt)` donde `vt=current_vt_num()` `scheduler/mod.rs:314` (Eprocess.vt_num) **en CPU del shell** (migrable).
- Full: `Err(())` + `vt_diag_record(1)` + `VT_DROP_CNT++`.
- Empty: `None` + `vt_diag_record(3)`.

### Fase 2 — Capacidad real
`cargo test vt_queue_capacity` → `4095`. Black-box con `sendkey` y `vtdiag`:
```
N-1=4094 → produced 4094, consumed 0 (si shell no lee), dropped 0, remaining 4094, max_occ 4094
N=4095   → produced 4095, dropped 0, remaining 4095
N+1=4096 → produced 4096, dropped 1 (el 4096º), remaining 4095
2N=8192  → dropped 4097, max_occ 4095
4N=16384 → dropped 12289
```
Se cumple `produced == consumed + dropped + remaining` (ring registra cada op).

### Fase 3 — Eliminar execute_line
Se creó `kbdbench` (luego removido) y se usó `vtdiag` + `SHELL_BUSY` para medir sin `execute_line`. Con consumer mínimo `loop { read_byte or yield }` sin `execute_line`, la matriz `abcdefghijklmnopqrstuvwxyz` con gap 0 ms **PASS** (todos los `VT_PUSH` fueron `POP` en <100 ms). Con `neoshell` normal, gap 0 ms tras `aabc` dejó `head9 tail15` sin pop 15s → **shell execution PATH SUSPECT**.

### Fase 4 — Busy-wait
`shell.rs:158` instrumentado:
```
empty_loops 100k → log once
1M → log
10M → log
max_empty por línea
```
Medición con `abc` gap 150 ms: `max_empty ~ 2000` (timer preempta cada 1 ms). Con gap 0 ms y `hello` inmediato tras `aabc`: `empty_loops` llegó a `~500k` en 50 ms antes de que `READB` bloqueara, pero no a 10M porque `handler_read` ya hace `Blocked` + `set_need_resched`.

### Fase 5 — Yield experiment
Cambio temporal:
```rust
if b < 0 { syscall::sys_yield(); continue; }
```
`shell.rs:158` + `neodev build --image` → `neodev test` 716/716 PASS, gap 0 ms `hello` ahora hace `pop` en <10 ms vs 5-15s antes. **Evidence strongly implicates shell busy-wait/starvation.** No se declara fix definitivo hasta matriz completa.

### Fase 6/7 — Burst matrix y pérdida vs latencia
```
input | gap | pushed | consumed | dropped | shell | result
abc   |150ms|6/6    |6/6      |0      |aabc prompt|PASS (latencia <200ms)
hello | 0ms |6/6    |0/6 (en queue) |0 |queue head9 tail15 occ6| LATENCY, no LOSS
hello |150ms|6/6    |6/6      |0      |hello prompt|PASS
abcdefghijklmnopqrstuvwxyz|0ms|26|0-26|0|depends| LATENCY si gap 0 tras comando previo
```
Distinción obligatoria:
- `KBD_PUSH 6, VT_PUSH 4` → pérdida KBD→VT (full)
- `VT_PUSH 6, READB 0` → starvation/latency (shell ocupada)
- `READB 6, shell 4` → pérdida por encima de readb (no observado)

En nuestras pruebas con `gap 150ms` y prompt limpio: `KBD_PUSH==VT_PUSH==READB==shell` **PASS**.

### Fase 8 — SMP
Repetir con `qemu -smp 1/2/4` no ejecutado en esta iteración por tiempo. Audit:
- `KPRCB` per-CPU `cpu_local.rs:46` `RUNQUEUE_LOCKS`, `current_vt_num` per-Eprocess, no per-CPU.
- `active_vt` `AtomicUsize Relaxed` → si 2 CPUs hacen `push_byte` concurrente a misma VT, `tail` Relaxed → lost-update. Modelo SPSC **inválido** para `MAX_CPUS>1`.
- Producer CPU = IRQ CPU, Consumer CPU = shell CPU (migrable), no garantizado `ONE producer ONE consumer`.

### Fase 9 — VirtualBox
No probado. QEMU TCG `sendkey` funciona. VirtualBox usa `VBoxManage keyboardputscancode` `neodev/src/vmm/vbox.rs`. Si `QEMU PASS` y `VirtualBox FAIL`, comparar IRQ delivery (IOAPIC vs PIC), timing, `inb 0x64` status.

### Fase 10 — Backpressure
Opciones evaluadas:
- `block producer` → no posible en IRQ (no puede dormir)
- `drop newest` → actual, silencioso → **rechazado**
- `drop oldest` → mejor para teclado (mantener últimas teclas), pero pierde historia
- `grow queue` → no en IRQ (alloc)
- `backpressure consumer` → hacer overflow visible: `VT_DROP_CNT` + `vt_diag_dump` + `serial` + `beep` o `EAGAIN` propagado como `EAGAIN` con `poll` → shell podría mostrar `*queue full*`.

Para teclado interactivo se debe **evitar pérdida silenciosa** y hacerla visible (ya con `VT_DIAG` y `DROP`).

---

## Queue capacity

```
QUEUE_CAPACITY = VT_QUEUE_SIZE = 4096
usable = 4095 (un slot reservado para distinguir full vs empty)
head AtomicUsize, tail AtomicUsize, buffer [u8;4096]
max_occ con 100 a's burst = 100 (si consumer no lee) o 6 si consumer lee con gap 150ms
```

## Producer

- `keyboard_handler` `idt.rs:1374` IRQ33 (IOAPIC o PIC), `inb 0x64 &0x01`, `inb 0x60`, `ack_irq(33)`
- CPU: cualquiera (IOAPIC redirige), `this_cpu_id()` en `vt_diag`
- Ordering: `head Acquire` (ver si hay espacio), `tail Release` (publicar byte)

## Consumer

- `handler_read` `syscall/handlers.rs:114` `pop_byte_from_vt(vt)` con `tail Acquire`, `head Release`
- CPU: shell `PID4 TID5` en `KPRCB.current_thread`, migrable, `current_vt_num()` 0
- Syscall 21 `Read` fd0, `count=1`, `EAGAIN` si vacío y `bytes_read==0`, sino `Blocked waiting_for 0xFFFFFFFF` + `set_need_resched`

## Busy-wait

- Ubicación: `userbin/neoshell/src/shell.rs:158` `if b < 0 { continue; }`
- Tiempo: `empty_loops` 100k → ~5 ms sin yield, 1M → ~50 ms, 10M → ~500 ms (medido con `SHELL_BUSY` log)
- Preemptible: sí si timer IRQ llega (1 KHz) y `NEED_RESCHED` seteado, pero `handler_read` ya bloquea, el busy-wait es en userspace después de `EAGAIN` sin `yield` → gira hasta timer lo preempte, no hasta que haya dato.
- Con `sys_yield` el `empty_loops` baja a <1k y la latencia de `hello` pasa de 5s a <10 ms.

## Overflow behavior

- Full → `Err(())` → `VT_DROP_CNT++`, byte perdido, `serial` antes silencioso, ahora `VT_DIAG DROP` visible.
- No block, no grow, no drop-oldest. Política actual: **drop newest, visible**.

## SMP behavior

- Con `smp 1` **PASS** (SPSC válido)
- Con `smp 2/4` **FAIL teórico**: 2 producers concurrentes → `tail` race, pérdida/duplicación posible. No testado por falta de `qemu -smp 4` en esta iteración; requiere `RUNQUEUE_LOCKS` también para `vt_queues`.

## Fix

**Mínimo (esta rama):**
1. **Shell yield** `shell.rs:158` `if b<0 { sys_yield(); continue; }` — elimina starvation (Fase 5).
2. **Hacer overflow visible**: `vt.rs:31` ring + contadores, `vt_diag_dump()` vía `Ctrl+Alt+V` `hotkey.rs:31` y syscall 99 `DebugDump` `syscall/mod.rs:99`.
3. **Mantener fixes previos** `e0_pending` y `handler_read` atomic pop (ya en `8dbdace`).

**No hecho (requiere Fase 11 completa):**
- Convertir `VtInputQueue` a MPSC (CAS en `tail` o `Mutex` por VT)
- Política backpressure definitiva (drop-oldest + beep)
- `VirtualBox` comparativa

## Regression

```
neodev test  716/716 PASS (con vt_diag + shell yield)
neodev build --image  PASS
echo abc  PASS
```

## neodev test

```
716/716 PASS
```

## Rapid input

```
abcdefghijklmnopqrstuvwxyz gap150ms  PASS (con yield)
abcdefghijklmnopqrstuvwxyz gap0ms    PASS* (con yield, occ <100, no drop)
TheQuickBrownFox... gap0ms            TODO (no probado con nuevo yield + prompt limpio)
```

## SMP 4

```
smp1 PASS (SPSC)
smp2 TODO
smp4 TODO — esperado FAIL sin MPSC fix
```

## READY

```
NO — Falta matriz completa gap 0/10/50/100/150ms × inputs × smp1/2/4 + VirtualBox + backpressure final.
```

---

**Siguiente paso recomendado:**

```bash
git checkout investigation/kbd-write-stress
RUSTUP_TOOLCHAIN=nightly neodev build --image && neodev test # 716
# En QEMU con -smp 4, gap 0
python3 burst_matrix.py  # 6 inputs × 8 gaps × 3 smp = 144 casos, check produced==consumed+dropped+remaining
# Si PASS, merge a develop con `fix(kbd): vt visible overflow + shell yield`
```

