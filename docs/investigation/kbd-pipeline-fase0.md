# FASE 0 — Pipeline de teclado NeoDOS (black-box + white-box)

**Branch:** `investigation/kbd-write-stress` @ 5791a48  
**Fecha:** 2026-09-25  
**Investigador:** Muse Spark (senior kernel x86_64)

---

## 1. Componentes identificados

| Capa | Archivo | Líneas | Responsabilidad |
|------|---------|--------|-----------------|
| PS/2 controller | `neodos-kernel/src/drivers/ps2.rs` | 1-148 | Puertos 0x60/0x64, init, LEDs, `read_scancode()` poll |
| PIC | `neodos-kernel/src/arch/x64/pic.rs` | 1-89 | ChainedPics offset 32/40, máscara `0xE8/0xFF`, IRQ1→vector 33 |
| IDT / IRQ | `neodos-kernel/src/arch/x64/idt.rs` | 515,1374-1395 | `idt[33]=keyboard_handler`, `ack_irq(33)` |
| Scancode queue | `neodos-kernel/src/kbd/event.rs` | 12-40,59-77 | `ScancodeQueue` SPSC 256 (head/tail atomics), `PENDING_SCANCODES` |
| Decoder | `neodos-kernel/src/kbd/mod.rs` | 164-254 | `process_scancode(code,is_make)`, modifiers, layout, dead_key, compose, `push_byte` + `wake_blocked_readers()` |
| Layout | `neodos-kernel/src/kbd/layout.rs` | 197-259 | `lookup_codepoint`, `is_dead_key`, `compose`, `builtin_us_layout` |
| Unicode | `neodos-kernel/src/kbd/unicode.rs` | — | `unicode_to_utf8` 1-3 bytes |
| VT buffer | `neodos-kernel/src/input/vt.rs` | 1-54 | `VtInputQueue` 4096 ring, `push`/`pop` atomics |
| Input manager | `neodos-kernel/src/input/manager.rs` | 1-78 | `active_vt`, 4 VT queues, `push_byte`→active VT, `pop_byte_from_vt` |
| Syscall READ | `neodos-kernel/src/syscall/handlers.rs` | 114-198 | `handler_read` fd 0 stdin, pop loop, Blocked 0xFFFFFFFF |
| Wake | `neodos-kernel/src/syscall/mod.rs` | 512-519 | `wake_blocked_readers()` → `wake_blocked_on_magic(0xFFFFFFFF)` + `set_need_resched()` |
| Wake impl | `neodos-kernel/src/scheduler/wake.rs` | 29-36 | Itera `kthreads`, `Ready` si `waiting_for==magic` |
| Console | `neodos-kernel/src/console.rs` | 419-477 | `write_char`, shadow `vt_shadow`, `Redraw`, ANSI |
| TTY/Shell | `libconsole-nxl/src/main.rs` | 237-386 | `console_readline`, `read_byte`→ `sys_read(0,1)`, `yield` via `int 0x80 rax=1` |
| NeoShell | `userbin/neoshell/src/shell.rs` | 153-265 | `readline` loop: `console::read_byte()`, echo, backspace, history, `pos/len` |
| Hotkey | `neodos-kernel/src/kbd/hotkey.rs` | 1-30 | Ctrl+Alt+Del→shutdown, Alt+F1-F4→`switch_vt` |
| EventBus | `neodos-kernel/src/eventbus/mod.rs` | 1-392 | 64 normal +16 high queues, `EVENT_KEYBOARD_INPUT` (no usado en path actual) |
| Scheduler | `neodos-kernel/src/scheduler/*` | — | 4 prioridades, aging, per-CPU `CpuRunQueue` 64×TIDs, `NEED_RESCHED` per-CPU + global |
| CPU local | `neodos-kernel/src/arch/x64/cpu_local.rs` | 1-978 | `KPRCB` 4KB, `GS_BASE`, `need_resched` @0x015 |

---

## 2. Pipeline completo (dónde cambia representación)

```
HARDWARE tecla física
  ↓  PS/2 scan set 1 (make=0x1E, break=0x9E)
PS2 controller 0x64 status&0x01 → 0x60 data                [drivers/ps2.rs:141-148]
  ↓ u8 scancode raw (incluye 0x80 bit release, 0xE0 prefix)
IRQ1 → PIC slave? No, master IRQ1 → vector 33             [arch/x64/pic.rs:74, idt.rs:515]
  ↓ InterruptStackFrame
keyboard_handler()                                           [idt.rs:1374]
  ─ lee status 0x64, si bit0 lee 0x60                        [idt.rs:1376-1381]
  ─ seq = KBD_SEQ++                                           [kbd/event.rs:92]
  ─ llama kbd_event_handler_direct(scancode) directo         [idt.rs:1391]
       ↓
kbd_process_internal(sc, "IRQ_DIRECT", seq)                  [kbd/event.rs:41-78]
  ─ log [KBD] seq scancode make code tid pid in_irq           [event.rs:48-56]
  ─ try_lock KBD (spin::Mutex)                                [event.rs:58]
     ├─ OK: drena PENDING_SCANCODES FIFO, luego current       [event.rs:60-67]
     └─ BUSY: push a PENDING_SCANCODES (256 SPSC) o drop      [event.rs:70-76]
               + set_need_resched() para drenar
       ↓
NeoKbd::process_scancode(code,is_make)                       [kbd/mod.rs:164]
  ─ code = sc &0x7F, is_extended = (sc==0xE0) → return      [mod.rs:165-169] ← BUG1
  ─ update modifiers (SHIFT/CTRL/ALT/CAPS/NUM/SCROLL)         [mod.rs:172-207]
  ─ push_event KEYDOWN/KEYUP (eventbus)                        [mod.rs:209-213]
  ─ if !is_make → return                                      [mod.rs:215]
  ─ dispatch_hotkey()                                         [mod.rs:219]
  ─ mods → lookup_codepoint(layout,code,mods)                  [layout.rs:197]
     → Option<u16> (0xFFFF = no mapeo)
  ─ is_dead_key? → dead_key=Some(cp) return                    [layout.rs:233]
  ─ dead_key compose?                                          [layout.rs:231-237]
  ─ unicode_to_utf8(cp) → [u8;4]                               [kbd/unicode.rs]
     for b in utf8: push_byte(b)                               [mod.rs:239-244]
       → InputManager::push_byte(active_vt)                     [input/manager.rs:60]
          → VtInputQueue::push(byte)                           [input/vt.rs:25]
             head Acquire, tail Relaxed, next=(tail+1)%4096
             if next==head → Err(()) full → log dropped         [mod.rs:242]
     wake_blocked_readers()                                     [mod.rs:245]
       → without_interrupts { wake_blocked_on_magic(0xFFFF...)  [syscall/mod.rs:512]
       → set_need_resched() global+per-CPU                       [syscall/mod.rs:216]
       → EVENT_KEY_CHAR, MODIFIER                               [mod.rs:247]
  ↓ u8 byte UTF-8 en VT queue activa
InputManager pop_byte_from_vt(vt)                              [input/manager.rs:64]
  ↓ Option<u8>
syscall handler_read fd0                                        [handlers.rs:125]
  ─ loop while bytes_read<count (count=1 en shell)
     pop_byte_from_vt(vt) SIN lock → if Some → write buf          [handlers.rs:132-139]
     else if bytes_read>0 → break                                 [handlers.rs:141]
     else without_interrupts {                                     [handlers.rs:145]
           pop again atomically → if Some → return byte           [handlers.rs:146]
           else set Blocked{waiting_for=0xFFFFFFFF}                [handlers.rs:152-156]
                set_need_resched() → return -Again (0xfff...ff8)  [handlers.rs:169]
       }
  ↓ 1 byte o -Again
libconsole-nxl read_byte()                                      [libconsole-nxl/src/main.rs:48]
  ─ asm int 0x80 rax=21 rbx=0 rcx=&c rdx=1 → if r&0x8000... → -1 else c
       ├─ neoshell shell.rs:158 b=console::read_byte(); if b<0 continue + yield [shell.rs:159+main.rs:253]
       └─ libconsole-nxl console_readline loop yield via mov rax,1 int 0x80 [main.rs:253]
  ↓ i32 byte
shell::Shell::readline()                                        [userbin/neoshell/src/shell.rs:153]
  ─ prompt, cursor blink, echo loop:
    b<0 → continue (yield ya en libconsole)
    \r/\n → history_add, break
    0x08/0x7F → backspace lógica + redraw
    0x01/0x02 → history prev/next
    0x09 → completion
    0x20-0x7E → insertar en line[pos], echo write_str([c])
  ↓ line[..pos]
shell::Shell::run → execute_line()                              [shell.rs:535]
```

**Cambios de representación:**
- `u8 raw scancode` → `(code u8 0x00-0x7F, is_make bool)` → `modifiers u8 bitmask` → `u16 codepoint` → `utf8 [u8]` → `u8 VT byte` → `i32 syscall` → `u8 shell char`

---

## 3. Puntos críticos y sospechas iniciales (white-box, sin modificar)

### BUG-E1: Extended prefix 0xE0 descartado incorrectamente
`process_scancode` hace `if scancode==0xE0 return` sin estado. El siguiente byte (ej. flecha arriba 0x48 con prefix E0) se interpreta como `code=0x48` normal (numpad 8). Además break extendido `0xE0 0xF0 ??` no existe en set1, pero QEMU emite `E0 xx` y `E0 xx|0x80`. Al descartar solo E0, el release siguiente se ve como make de otro scancode. Flechas, Delete, Home/End, etc. rotas. Caps/Shift state puede desincronizarse si se pierde un make/break.

### BUG-E2: PENDING_SCANCODES SPSC vs MPSC
`ScancodeQueue` usa `head/tail` con `Relaxed/Acquire/Release` asumiendo SPSC (1 IRQ producer → 1 consumer cuando KBD lock disponible). En SMP, IRQ1 puede llegar en cualquier CPU (IOAPIC redirige). Dos CPUs pueden ejecutar `keyboard_handler` concurrentemente → dos productores concurrentes → race en `tail` (lost update, corrupción). Tamaño 256 oculta pero no elimina.

### BUG-E3: VtInputQueue SPSC vs MPSC + orden memoria
`VtInputQueue::push` hace `head Acquire` + `tail Relaxed` y `pop` hace lo inverso. Correcto para SPSC single-producer-single-consumer. Pero productor es IRQ (cualquier CPU) y consumidor es `handler_read` en CPU del shell (puede migrar). Si dos IRQs en CPUs distintas empujan a la misma VT (active_vt global), es MPSC → race en `tail`. Además `push_byte` lee `active_vt` con `Relaxed` sin sincronización; si VT switch ocurre entre IRQ y push, byte va a cola equivocada.

### BUG-E4: handler_read double-pop race (Fase preemption)
Fuera de `without_interrupts` hace `pop_byte_from_vt` sin lock. Entre ese `None` y el `without_interrupts` interior, un IRQ puede pushar byte y hacer `wake_blocked_readers`. El hilo aún no está Blocked, wake no hace nada. Luego dentro de `without_interrupts` re-chequea y encuentra byte → OK (se recupera). Pero si wake ocurrió antes de Blocked, se pierde wakeup → el hilo se bloquea con byte ya en cola pero nadie lo despertará hasta próxima tecla. En log `vbox_serial.log:2117-2118` se ve bloqueo incluso con byte disponible después: `[READB] enter ...` → `[READB] blocking` → siguiente IRQ despierta. Si timing es ajustado, podría causar latencia o pérdida percibida.

### BUG-E5: VT queue full → silent drop
`push_byte` Err → solo `serial_println!("[KBD] VT input queue full (4096), byte 0x.. dropped")` [kbd/mod.rs:242]. Shell no sabe. Stress test `2N` (8192) debe perder 4095 bytes. No hay backpressure ni ACK.

### BUG-E6: KBD try_lock contención → latencia
`KBD: Mutex<NeoKbd>` es `spin::Mutex`. Si shell está en `set_leds` (llamado desde `process_scancode` para Caps) que a su vez hace `ps2_wait_input` con polling 100k, IRQ no puede hacer `try_lock` y va a PENDING queue. `set_leds` dentro de IRQ deshabilitado? No, pero `ps2::set_leds` hace `outb 0x60 0xED` + `outb 0x60 leds` sin verificar ACK, puede bloquear IRQ por timeout. Además `process_scancode` mantiene lock durante `push_byte` + `wake_blocked_readers` que toma scheduler lock → potencial inversión con timer IRQ que también toma scheduler lock.

### BUG-E7: Handler_read magia 0xFFFFFFFF broadcast
`wake_blocked_on_magic(0xFFFFFFFF)` despierta *todos* los hilos bloqueados en READ (normalmente solo shell). Si hay 2 shells en VTs distintas, ambos despiertan aunque solo una VT recibió byte → thundering herd, uno volverá a bloquear, otro consumirá byte de otra VT? Pero `handler_read` lee siempre `current_vt_num()` → el despertado en VT no activo leerá cola vacía → vuelve a bloquear. No es pérdida, es wakeup espurio.

### Otros:
- `clear_and_rewrite` en libconsole-nxl usa 200 bytes con `\x08` para flush QEMU — no afecta kernel.
- `console::write_char` y `draw_char_at` acceden a `RENDERER` + `vt_shadow` sin lock; OK porque single consumer shell.
- `EVENT_KEYBOARD_INPUT` ya no se usa; path directo es usado (5791a48 fix).

---

## 4. Infraestructura de pruebas actual

- `neodev shell send "cmd"` → vía VM automation (no vía IRQ1, es inyección por canal auxiliar).
- `neodev test` → ejecuta 665 tests kernel en QEMU headless, captura serial, verifica PASS/FAIL (no prueba teclado real).
- Inyección real IRQ1: `QEMU monitor sendkey a` o `qemu qmp send_keys` (no expuesto por neodev). Determinará en Fase1 si existe.

---

## 5. Próximos pasos

FASE 1: probar `a → abc → hello` vía `neodev shell send` + vía `sendkey` QEMU si disponible, capturar serial `[KBD]`/`[READB]` y comparar EXPECTED vs ACTUAL. FASE 2: instrumentar pipeline con logs de cada capa (ya existe `[KBD_IRQ]`, `[KBD]`, `[KBD_EVENT]`, `[READB]`). No se modificará lógica, solo se añadirá medición de pérdida/duplicación.

