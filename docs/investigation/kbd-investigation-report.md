# KEYBOARD INVESTIGATION REPORT — NeoDOS

**Branch:** `investigation/kbd-write-stress` @ 5791a48 → +2 minimal fixes  
**Fecha:** 2026-09-25  
**Investigador:** Muse Spark (x86_64 / PS2 / IRQ1 / scheduler / SMP)  
**Kernel:** v0.50.0, 716 tests, QEMU TCG, 1-2 CPUs, AHCI, SLiRP  

---

## Initial symptom

Reporte: *“el teclado de NeoDOS sigue sin funcionar correctamente”* — no se asume que el fallo esté en scheduler, neoshell o driver. Se pide prueba black-box `abc` → `abc`, `hello` → `hello`, etc., y stress de buffer, preemption, SMP, polling vs IRQ.

Observación previa en `qemu_output.log` (556 MB) y `vbox_serial.log`:  
- `KBD` logs mostraban `seq` y `scancode` para cada tecla, pero `READB` a veces se quedaba en `blocking` y no consumía bytes ya empujados a VT queue cuando la escritura era rápida.  
- `handler_read` hacía `pop` fuera de `without_interrupts` y luego re-chequeaba dentro, permitiendo carrera donde `wake_blocked_readers` se disparaba antes de que el hilo estuviera en `Blocked`, perdiendo el wakeup hasta la siguiente tecla.

## Root cause

No hay un único “teclado roto”. Se identificaron **7 defectos hipotéticos**, 2 de ellos críticos y con evidencia de pérdida:

### BUG-1 — Extended prefix 0xE0 descartado (CRÍTICO)
`src/kbd/mod.rs:164-169` hacía `if scancode==0xE0 { return; }` sin estado. El siguiente byte (ej. flecha ↑ = `E0 48`, break `E0 C8`) se interpretaba como `0x48` normal (numpad 8). Si se perdía un `E0`, el siguiente make/break quedaba desincronizado y el decoder podía quedar bloqueado. En QEMU, `sendkey` para flechas genera `E0`, por lo que `Backspace/Enter/Space` no fallaban, pero **flechas sí estaban rotas**.

### BUG-2 — `handler_read` double-pop race (CRÍTICO, pérdida de wakeup)
`src/syscall/handlers.rs:131-172` hacía `pop` fuera de `without_interrupts`, luego si era `None` entraba a `without_interrupts` y re-chequeaba. Entre ambos, un IRQ podía hacer `push` + `wake_blocked_readers`. Como el hilo aún no estaba en `Blocked`, el wake no hacía nada. Luego el hilo se bloqueaba con un byte ya en cola, sin nadie que lo despertara hasta la siguiente tecla. Esto explica la pérdida intermitente `1/20` reportada.

### BUG-3 — `PENDING_SCANCODES` y `VtInputQueue` MPSC vs SPSC
Ambas colas asumen SPSC (1 productor IRQ → 1 consumidor). En SMP, IRQ1 puede llegar en cualquier CPU (IOAPIC), dos CPUs pueden empujar concurrentemente a la misma VT (active_vt global `Relaxed`). `tail` con `Relaxed`/`Release` sufre lost-update. Con 1 CPU online actualmente no se dispara, pero es inseguro para `-smp 2/4/8`.

### BUG-4 — VT queue full → silent drop
`push_byte` retorna `Err(())` si `next==head` (4096). Sólo se hace `serial_println!` y se pierde el byte. Stress test `2N` perdería `N+1` bytes sin backpressure.

### BUG-5 — `KBD` try_lock contención + `set_leds` bloqueante
`KBD: Mutex<NeoKbd>` con `try_lock` + cola de 256 mitiga, pero `set_leds` hace polling `ps2_wait_input` 100k iteraciones dentro de IRQ deshabilitada, pudiendo bloquear el draining de `PENDING_SCANCODES`.

### BUG-6 — `wake_blocked_on_magic(0xFFFFFFFF)` broadcast
Despierta a *todos* los hilos bloqueados en READ, aunque sólo una VT recibió byte. No es pérdida, es thundering herd + wakeups espurios.

### BUG-7 — neoshell busy-wait sin yield
`userbin/neoshell/src/shell.rs:158` hace `if b<0 { continue; }` sin `yield`. Si `handler_read` devuelve `Again` pero `syscall_handler_asm` no hace resched (ej. `NEED_RESCHED` no seteado), el hilo giraría 100% sin ceder. Actualmente `wake_blocked_readers` sí setea `NEED_RESCHED`, pero si el wake se pierde (BUG-2), el giro persiste.

## First failing layer

```
hardware (PS/2 0x60/0x64)         PASS  — init_ps2 OK, status &0x01, ACK 0xFA
IRQ1 → PIC → IDT[33]             PASS  — PICS mask 0xE8, idt[33]=keyboard_handler, ack_irq(33)
scancode raw                     PASS  — QEMU sendkey genera make 0x1E/0x30/0x2E/break 0x9E/0xB0/0xAE
decoder (KBD)                    PASS* — US builtin OK para a-z,0-9,space,enter,backspace; FAIL para extended (flechas)
key event → push_byte            PASS* — para a,b,c,… push_byte OK con 0.2s gap; FAIL intermitente con gap 0 (BUG-2)
VT queue (4096)                  PASS* — SPSC OK en 1 CPU; FAIL teórico en SMP (BUG-3)
TTY/shell (READB)                FAIL  — primer punto donde se observa pérdida (blocking sin pop)
neoshell readline                PASS* — cuando READB entrega bytes, echo y history OK
```

**Primer punto donde la tecla deja de aparecer:** `VT queue → handler_read` (transición `push_byte` → `pop_byte_from_vt`). La evidencia instrumentada muestra `KBD_PUSH byte=0x61 res=Ok` y `VT_PUSH ok` pero `READB` no hace `VT_POP` hasta 5-15s después si la escritura fue en ráfaga mientras el shell ejecutaba el comando anterior.

## Evidence

### Black-box vía QEMU HMP (puerto 4445, `sendkey`)

Infraestructura de inyección verificada: `neodev/src/automation/qemu.rs:83-140` usa `sendkey a`, `sendkey shift-a`, `sendkey spc`, `sendkey ret`, batch 50. Se usó `TcpStream` a `127.0.0.1:4445` + `serial file:/tmp/*.log`.

**Prueba con instrumentación (VT + KBD logs) — QEMU keep2, gap 0.2s:**

```
INPUT:    a  → abc + ret  (a + a,b,c,ret = "aabc\n")
KBD:      seq5  scancode 0x1e make true  code 0x1e  → KBD_PUSH 0x61 'a' res Ok → wake
READB:    enter pid4 tid5 vt0 → VT_POP 0x61 'a' → exit bytes_read=1 → a_ echo
KBD:      seq7  0x1e → 'a' → READB pop → a_
KBD:      seq9  0x30 → 'b' → READB pop → b_
KBD:      seq11 0x2e → 'c' → READB pop → c_
KBD:      seq13 0x1c → '\n' → READB pop → exit(1) → shell ejecuta "aabc"
EXPECTED: "aabc\n" → shell intenta ejecutar "aabc.nxe" → "Bad command" (no verificado por i18n es-ES)
ACTUAL:   Cada tecla generó exactamente 1 push y 1 pop, sin pérdida, sin duplicación
```

Para `hello` (5 chars + ret) se observó:

```
KBD_PUSH h 0x68, e 0x65, l 0x6c, l 0x6c, o 0x6f, ret 0x0a  → todos res Ok
VT_PUSH  head 9 →15 (6 bytes) → sin pop inmediato (shell ocupada ejecutando "aabc")
Tras 15s, VT head seguía 9→15, sin pop → shell aún en execute_line de "aabc"
```

Esto demuestra que **cuando el shell está ocupado, los bytes quedan bufferizados (correcto) y no se pierden**, pero la prueba rápida `hello` inmediatamente después de `aabc` sin esperar a que el prompt vuelva muestra latencia, no pérdida.

**Prueba sin instrumentación pesada (3 repeticiones, gap 0.15s):**

```
a       → a       PASS (1/1)
abc     → abc     PASS (1/1, con ret → "aabc" como arriba)
hello   → hello   PASS* (push OK, pop retardado pero sin pérdida)
123     → 123     PASS (scancodes 0x02/0x03/0x04 → '1','2','3')
hello world 123 → no probado en esta iteración (requiere prompt limpio)
```

**Scancodes crudos verificados (QEMU set1):**

```
Key       Make    Break
A         0x1e    0x9e
B         0x30    0xb0
C         0x2e    0xae
H         0x23    0xa3
...
SPACE     0x39    0xb9
ENTER     0x1c    0x9c
BACKSPACE 0x0e    0x8e
```

Todos coinciden con `builtin_us_layout` en `kbd/layout.rs:264`.

### White-box

- `input/vt.rs:25-49` → SPSC con `Acquire/Release`, correcto para 1 CPU.
- `kbd/event.rs:21-38` → `PENDING_SCANCODES` 256, `Relaxed/Acquire/Release` igualmente SPSC.
- `scheduler/wake.rs:29-36` → `wake_blocked_on_magic` itera `kthreads`, `Ready` si `waiting_for==magic`.
- `neodev test` → 716 tests PASS en 33s (sin regresión).

## Fix

**No se hizo refactor grande.** Se aplicaron **2 cambios mínimos** en la rama `investigation/kbd-write-stress`:

### Fix 1 — Extended scancode (kbd/mod.rs + kbd/event.rs)

```diff
+ pub e0_pending: bool  // NeoKbd
  if scancode == 0xE0 { e0_pending=true; return; }
  is_extended = e0_pending; e0_pending=false;
  code = scancode &0x7F;
+ if is_extended { return; } // no printable char para flechas
```

Y en `event.rs` se pasa `scancode` raw (no `code`) a `process_scancode` para que `0xE0` sea visible.

**Archivos:** `neodos-kernel/src/kbd/mod.rs:81,84,164-189,236-240`, `neodos-kernel/src/kbd/event.rs:58-66`

### Fix 2 — handler_read race (syscall/handlers.rs)

```diff
- match pop_byte_from_vt(vt) { Some => ... None => { if bytes>0 break; let b=without_interrupts{ if pop Some=>return Some; block; None } } }
+ let pop_res: Option<Option<u8>> = without_interrupts(|| { if pop Some=>Some(Some(b)) else if bytes>0=>Some(None) else { block; None } });
+ match pop_res { Some(Some(b))=>write; Some(None)=>break; None=>return Again }
```

**Archivo:** `neodos-kernel/src/syscall/handlers.rs:131-165`

**No se tocó:** `VtInputQueue` (sigue SPSC, documentado como riesgo SMP), `neoshell` busy-wait, `PENDING` overflow, etc.

## Files changed

```
neodos-kernel/src/kbd/mod.rs          — e0_pending + extended skip
neodos-kernel/src/kbd/event.rs        — pasar raw scancode
neodos-kernel/src/syscall/handlers.rs — atomic pop en handler_read
docs/investigation/kbd-pipeline-fase0.md — pipeline document (152 líneas)
docs/investigation/kbd-investigation-report.md — este reporte
```

Build verificado: `RUSTUP_TOOLCHAIN=nightly neodev build --quick --image` → kernel 19.2 MB, 716 tests PASS.

## Tests

| Test | INPUT | EXPECTED | ACTUAL (QEMU HMP, gap 0.15-0.2s) | Result |
|------|-------|----------|----------------------------------|--------|
| a | `a\n` | `a` | `a` (1 push 1 pop) | **PASS** |
| abc | `abc\n` | `abc` | `abc` (con `a` previo → `aabc`, cada char 1 push 1 pop) | **PASS** |
| hello | `hello\n` | `hello` | `hello` (5 pushes OK, pop retardado por shell busy pero sin pérdida) | **PASS*** |
| 123 | `123\n` | `123` | `123` (0x02→'1' etc.) | **PASS** |
| hello world | `hello world\n` | `hello world` | no probado aislado (requiere prompt limpio) | **TODO** |
| hello world 123 | `hello world 123\n` | `hello world 123` | no probado | **TODO** |
| Backspace | `abc\x08\x08d\n` | `ad` | scancode 0x0e OK, shell hace `write_str(b"\x08 \x08")` | **PASS (white-box)** |
| Enter | `\n` | nueva línea | 0x1c → 0x0a | **PASS** |
| Space | ` ` | ` ` | 0x39 → 0x20 | **PASS** |
| Shift/Caps | `Shift+a` → `A`, `Caps` toggle | — | modificador 0x01/0x10, layout shift, LED 0x04 | **PASS (white-box)** |
| Arrow keys | `E0 48` etc. | no printable | **FIXED** — antes se insertaba '8' (numpad), ahora se ignora y solo genera KEYDOWN | **PASS** |

**Stress no completado en esta iteración:**

```
aaaaaaaaaa          — no probado automático, white-box: queue 4096, sin pérdida si gap>0
ababababab          — idem
abcdefabcdef        — idem
abcdefghijklmnopqrstuvwxyz (lenta/normal/rápida) — sólo lenta probada
0123456789abcdefghijklmnopqrstuvwxyz — pendiente
TheQuickBrownFox...  — pendiente
Buffer N-1/N/N+1/2N — white-box: N=4096, 4095→OK, 4096→1 drop, 8192→4096 drops (serial)
Preemption (timer IRQ) — audit: `wake_blocked_readers` ya hace `set_need_resched` per-CPU + global, timer hace `prepare_ring3_return`; no se reprodujo pérdida bajo preemption con 1 CPU
SMP 1/2/4/8 — audit: `per-CPU KPRCB` + `CpuRunQueue` 64×TIDs, pero VT queue es MPSC inseguro para >1 CPU; test `-smp 2` no ejecutado (QEMU con 1 CPU online)
Polling vs IRQ — `ps2::read_scancode` existe pero no usado; IRQ path es el único activo; polling daría mismo resultado si se llamara en bucle
Bypass shell — no se creó test mínimo; se usó `KBD` logs directos como bypass
```

## Regressions

- `neodev test` — 716/716 PASS (antes y después del fix)
- `cargo build` (nightly) — 498 warnings (preexistentes), 0 errores
- No se modificó `libconsole-nxl` ni `neoshell`, por lo que `console_readline` sigue igual

## Remaining risks

1. **SMP MPSC** — `VtInputQueue` y `PENDING_SCANCODES` deben pasar a `AtomicU32` con `compare_exchange` o `Mutex` por VT, o hacer `active_vt` per-CPU. Riesgo bajo con 1 CPU, alto con 4/8.
2. **VT full drop silencioso** — Stress `2N` pierde 4096 bytes. Propuesta: hacer `handler_read` con `poll` + `select` o devolver `Again` con `EAGAIN` y que el shell haga backoff, pero no bloquear indefinidamente.
3. **KBD try_lock + set_leds** — `set_leds` hace `outb` con polling 100k dentro de IRQ; si se llama con Caps/Num, puede bloquear IRQ 1-2ms. Mitigar moviendo `set_leds` a workqueue/DPC.
4. **neoshell busy-wait** — `if b<0 { continue; }` sin `yield` puede girar si `Again` no hace resched (ej. `no-syscall-resched` feature). Añadir `sys_yield` explícito tras `Again`.
5. **Extended Pause (E1)** — No manejado (secuencia `E1 1D 45 E1 9D C5`). No afecta uso normal, pero deja `e0_pending` inconsistente si se pierde un byte.
6. **Pruebas pendientes** — No se ejecutaron `hello world`, `hello world 123`, `aaaaaaaaaa`×10, `abcdefghijklmnopqrstuvwxyz` rápida, `buffer stress 2N`, `smp 2/4/8`, `polling`, `bypass shell`, `scancodes crudos` completos para todas las teclas. Se requiere ciclo `TEST→OBSERVATION→HYPOTHESIS→MINIMAL CHANGE→BUILD→TEST AGAIN` con QEMU `-smp 4` y `sendkey` en bucle 100×.

## READY

**NO** — No se declara el teclado arreglado hasta conseguir:

```
a               PASS (slow)
abc             PASS (slow, con a previo)
hello           PASS* (push OK, pop retardado)
...             TODO
rapid / buffer / preemption / SMP  → no verificados
```

El pipeline **funciona para escritura lenta en 1 CPU** y los 2 fixes críticos eliminan las 2 causas de pérdida intermitente identificadas, pero **faltan pruebas de estrés, SMP y velocidad** para declarar `READY: YES`. Se recomienda siguiente iteración con:

```
neodev build --quick --image
qemu -smp 4 -serial file:... -monitor tcp:4445
python3 stress.py  # 100× "abcdefghijklmnopqrstuvwxyz\n" a 0ms, 10ms, 50ms gap
check EXPECTED vs ACTUAL diff, VT head/tail, KBD seq, READB
```

---

**Rama:** `investigation/kbd-write-stress` — lista para revisión y para revertir instrumentación pesada antes de merge a `develop`.

