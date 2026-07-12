# NeoKBD — Keyboard Manager de NeoDOS

> **Versión:** v0.1
> **Estado:** Diseño completo
> **Versión de NeoDOS:** v0.50+
> **Dependencias:** Event Bus (existente), Registry/Cm (existente), Input Manager (existente), Ob (existente)

---

## 1. Research

### 1.1 Auditoría del flujo actual de teclado

El flujo de eventos desde el hardware hasta el espacio de usuario tiene 7 etapas:

```text
PS/2 IRQ (IDT handler)
  → scancode raw (inb 0x60)
  → EVENT_KEYBOARD_INPUT (type 1) al Event Bus
  → ps2kbd NEM driver (driver_on_event)
  → process_scancode()
  → translate_scancode() + layout tables + dead keys
  → hst_push_input_byte(byte)
  → input::push_byte(byte) → VT queue
  → sys_read (fd=0, en proceso usuario)
  → console NXL → libneodos → neoshell
```

### 1.2 Archivos clave existentes

| Archivo | Rol actual | Líneas |
| --- | --- | --- |
| `drivers/ps2kbd/src/lib.rs` | NEM driver: scancode→byte, layout, dead keys, modifiers, UTF-8 encode | 277 |
| `drivers/ps2kbd/build.rs` | Genera `kbd_layout.rs` desde archivos `.klc` | ~200 |
| `drivers/ps2kbd/layouts/KBDUS.klc` | Layout US (inglés) | ~100 |
| `drivers/ps2kbd/layouts/KBDSP.klc` | Layout SP (español) | ~100 |
| `neodos-kernel/src/arch/x64/idt.rs:698-756` | IRQ handler: lee scancode, check Ctrl+Alt+Del, Alt+F#, push EVENT_KEYBOARD_INPUT | 58 |
| `neodos-kernel/src/drivers/nem/hst.rs:78-82` | `hst_push_input_byte()`: capability check → input::push_byte | 4 |
| `neodos-kernel/src/eventbus/mod.rs` | `EVENT_KEYBOARD_INPUT=1`, `EVENT_KEYB_LAYOUT=9` | 548 |
| `neodos-kernel/src/input/manager.rs` | `InputManager`: 4 VT queues, dispatch | 78 |
| `neodos-kernel/src/input/vt.rs` | `VtInputQueue`: ring buffer 4096 bytes, lock-free SPSC | 66 |
| `neodos-kernel/src/syscall/handlers.rs:441-507` | `handler_read()`: stdin → VT queue pop, bloqueo | 66 |
| `neodos-kernel/src/syscall/ob.rs:1277-1283` | `ob_query_info(KeyboardLayout)`: lee KEYBOARD_LAYOUT | 6 |
| `neodos-kernel/src/syscall/ob.rs:1921-1935` | `ob_set_info(KeyboardLayout)`: escribe KEYBOARD_LAYOUT, push EVENT_KEYB_LAYOUT | 15 |
| `neodos-kernel/src/syscall/mod.rs:231` | `KEYBOARD_LAYOUT: AtomicU8 = 1` (default Spanish) | 1 |
| `neodos-kernel/src/object/types.rs` | `ObInfoClass::KeyboardLayout=14`, `ObSetInfoClass::KeyboardLayout=5` | definiciones |
| `neodos-kernel/src/main.rs:246-248` | Crea `\Global\Info\Keyboard` (ObType::Key, native_id=9) | 3 |
| `neodos-kernel/src/drivers/boot_loader/mod.rs:165-172` | Registra PS2KBD para EVENT_KEYBOARD_INPUT + EVENT_KEYB_LAYOUT | 8 |
| `libconsole-nxl/src/main.rs` | `read_byte()` → `sys_read(0)` → devuelve byte | 60 |
| `libneodos/src/console.rs` | `read_byte()` via NXL export table | 20 |
| `userbin/neoshell/src/shell.rs` | `readline()` → `console::read_byte()` → build line | ~200 |
| `userbin/keyb/src/main.rs` | KEYB utility: set layout via Ob API | 126 |

### 1.3 Limitaciones identificadas

| # | Limitación | Causa raíz | Impacto |
| --- | --- | --- | --- |
| L1 | Solo 2 layouts (US, SP), índice `u8` | `KEYBOARD_LAYOUT: AtomicU8`, layout compilado en driver NEM | No se pueden añadir layouts sin recompilar el driver |
| L2 | Sin nombres de layout | El layout se identifica solo por número (0/1) | KEYB muestra "0" o "1", no "US"/"Spanish" |
| L3 | Sin persistencia | `KEYBOARD_LAYOUT` es volátil, no se guarda en Registry | Al reiniciar, siempre Spanish |
| L4 | Bytes especiales en stream | Flechas → 0x01/0x02, Backspace → 0x08, Tab → 0x09 | Imposible distinguir entre flecha arriba real y byte 0x01 |
| L5 | Dead keys limitadas a `u8` | `DEAD_KEY: AtomicU8` guarda solo la tecla muerta como byte | No puede componer caracteres >U+00FF (ej. Č, Ğ, ń) |
| L6 | Sin API de estado de teclas | El NEM driver mantiene MODS internamente, no expuesto | User space no puede consultar Caps/Num/Scroll state |
| L7 | Auto-repeat no configurable | Usa repeat hardware PS/2 (siempre activo, tasa fija) | No hay API para cambiar RepeatDelay/RepeatRate |
| L8 | Sin hotkeys configurables | Ctrl+Alt+Del hardcodeado en IDT handler, Alt+F1-F4 hardcodeado | No se pueden añadir hotkeys sin modificar IDT |
| L9 | LED management inline | `set_leds()` en PS/2 driver, solo se llama 3 veces en boot | User space no puede controlar LEDs |
| L10 | ObType incorrecto | Keyboard es `ObType::Key` (tipo Registry) con `native_id=9` | Abuso del sistema de tipos, no hay un ObType propio |
| L11 | Sin información de teclado vía Ob | `ObInfoClass::KeyboardLayout=14` devuelve solo 1 byte | No se puede obtener nombre, número de layouts, capacidades |
| L12 | Sin i18n en layout names | Los nombres "US"/"SP" están hardcodeados en KEYB binary | Preparar para `tr!("keyboard.layout.us")` |

### 1.4 Por qué las abstracciones existentes no resuelven el problema

- **El NEM driver ps2kbd** es un componente aislado con capacidades limitadas. No tiene acceso al Registry, no puede ser consultado vía Ob, y sus tablas de layout están compiladas estáticamente.
- **`KEYBOARD_LAYOUT: AtomicU8`** es un único entero atómico. No hay estructura para metadata, nombres, ni más de 256 layouts.
- **`ObInfoClass::KeyboardLayout=14`** retorna un solo `u8`. No hay variante para consultar nombre, capacidades, o información extendida.
- **`input::push_byte(byte)`** es un canal de un solo byte. No transporta eventos estructurados (key down/up, modificadores).
- **Input Manager** gestiona colas de terminal virtual, pero no tiene semántica de teclado. Solo maneja bytes.
- **Registry (Cm)** no tiene ninguna clave de configuración de teclado. No hay `\Registry\Machine\System\Keyboard`.
- **libneodos** no tiene wrappers de teclado más allá de `KEYB` que usa `ob_set_info(KeyboardLayout)`.

Se necesita un **nuevo subsistema de kernel** que:

1. Centralice la gestión de layouts (carga dinámica, nombres, metadatos)
2. Exponga un Ob nativo (`ObType::KeyboardDevice`) en el namespace
3. Proporcione consultas ricas vía `ob_query_info` (nombre del layout, lista de layouts, estado de LEDs, capacidades)
4. Soporte configuración vía `ob_set_info` (layout, repeat, LEDs, hotkeys)
5. Persista la configuración en Registry automáticamente
6. Mantenga el estado de modificadores centralizado y consultable
7. Emita eventos estructurados (KeyDown/KeyUp/Char) además de bytes

---

## 2. Problem Analysis

### 2.1 Estado actual

El teclado en NeoDOS funciona, pero con limitaciones severas:

- La ps2kbd NEM driver es monolítica: mezcla detección de hardware, traducción scancode→carácter, gestión de dead keys, composición Unicode, y push de bytes en ~277 líneas.
- El kernel no tiene visibilidad de la semántica del teclado. Solo recibe bytes de la cola VT. No sabe qué teclas están pulsadas, qué layout está activo (más allá del `AtomicU8`), ni el estado de los LEDs.
- El user space (neoshell, keyb) no puede consultar información del teclado más allá de "layout es 0 o 1".
- Añadir un nuevo layout requiere: crear .klc, modificar build.rs, recompilar driver, reiniciar.
- No hay preparación para la futura GUI: la GUI necesitará eventos KeyDown/KeyUp, no bytes en una cola.

### 2.2 Por qué no basta con parchar lo existente

- **Refactorizar ps2kbd** para añadir más layouts empeoraría el acoplamiento. El driver no debería ser el gestor de layouts.
- **Añadir más `ObInfoClass` variantes** al Ob actual (`ObType::Key` con `native_id=9`) es posible, pero forzar un tipo semánticamente incorrecto (`Key = Registry key`) para representar un dispositivo de entrada es arquitectónicamente incorrecto.
- **Añadir campos a `KEYBOARD_LAYOUT`** (ej. `AtomicU16` o struct) parchearía L1 pero no resolvería L2-L12.

Se necesita un **nuevo subsistema de kernel con un ObType propio**, no un parche sobre la abstracción incorrecta.

---

## 3. Solution Design

### 3.1 Arquitectura general

```text
PS/2 IRQ (IDT handler)
  │
  ▼
EVENT_KEYBOARD_INPUT (type 1) ────────────────────┐
  │ scancode + make/break                          │
  ▼                                                │
ps2kbd NEM driver (SIMPLIFICADO)                   │
  │ solo modifica state (make/break → modifiers)   │
  │ emite eventos estructurados                    ▼
  └──────────────────────────────────┐    NeoKBD (kernel)
                                     │    src/kbd/
                                     │    ┌──────────────────┐
                                     │    │ Event Processor  │ ← recibe EVENT_KEYBOARD_INPUT
                                     │    ├──────────────────┤
                                     │    │ Layout Engine    │ ← carga .kbd de disco
                                     │    ├──────────────────┤
                                     │    │ Unicode Mapper   │ ← scancode → key → Unicode
                                     │    ├──────────────────┤
                                     │    │ Dead Key Engine  │ ← compose dead keys + Unicode
                                     │    ├──────────────────┤
                                     │    │ Hotkey Dispatcher│ ← Ctrl+C, Ctrl+Alt+Del, etc.
                                     │    ├──────────────────┤
                                     │    │ Auto Repeat      │ ← timer-driven repeat
                                     │    ├──────────────────┤
                                     │    │ Config Manager   │ ← Registry persistencia
                                     │    └──────────────────┘
                                     │          │
                                     │          ├── input::push_byte(byte) → VT queue (legacy)
                                     │          ├── EVENT_KEYDOWN / EVENT_KEYUP (nuevo)
                                     │          └── Ob API (consulta/configuración)
                                     ▼
                              \Device\Keyboard
                              ObType::KeyboardDevice(22)
```

### 3.2 Principios de diseño

1. **Hardware aislado del layout.** El driver PS/2 emite solo eventos físicos (scancode + make/break). NeoKBD traduce.
2. **Layouts como datos, no código.** Archivos `.kbd` binarios cargados desde `C:\System\Keyboard\`. Sin compilación.
3. **Unicode nativo.** Toda la representación interna es Unicode (u32 codepoint). La conversión a UTF-8 ocurre al entregar al user space.
4. **Eventos estructurados.** KeyDown, KeyUp, CharacterInput, ModifierChanged son eventos de primera clase.
5. **Ob nativo.** `ObType::KeyboardDevice(22)` con entrada propia en `\Device\Keyboard`.
6. **Registry como fuente de verdad.** Toda configuración persistente via Cm syscalls.

### 3.3 Nuevo ObType

```rust
// En src/object/types.rs
KeyboardDevice = 22,
```

### 3.4 Nuevos archivos en kernel

| Ruta | Propósito | Líneas estimadas |
| --- | --- | --- |
| `src/kbd/mod.rs` | `NeoKbd` struct, `KBD` global singleton, `kbd_init()` | 150 |
| `src/kbd/layout.rs` | Layout engine: `KbdLayout`, `KeyEntry`, carga de `.kbd`, lookup scancode→key→unicode | 250 |
| `src/kbd/unicode.rs` | Unicode mapper: combinación dead keys, UTF-8 encode, compose tables | 120 |
| `src/kbd/hotkey.rs` | Hotkey dispatcher: registro, matching, callback | 100 |
| `src/kbd/repeat.rs` | Auto-repeat: timer management, repeat rate/delay | 80 |
| `src/kbd/config.rs` | Config Manager: load/save to Registry, defaults | 120 |
| `src/kbd/event.rs` | Event types, subscriber for EVENT_KEYBOARD_INPUT | 80 |

**Total estimado: ~900 líneas.**

### 3.5 Cambios a archivos existentes

| Archivo | Cambio |
| --- | --- |
| `src/object/types.rs` | Añadir `KeyboardDevice = 22` a `ObType`, `ob_type_list` |
| `src/object/types.rs` | Añadir `KeyboardInfo = 35`, `KeyboardCaps = 36` a `ObInfoClass` |
| `src/object/types.rs` | Añadir `KeyboardSetLayout = 43`, `KeyboardSetRepeatDelay = 44`, `KeyboardSetRepeatRate = 45`, `KeyboardSetLeds = 46`, `KeyboardSetModifier = 47` a `ObSetInfoClass` |
| `src/syscall/ob.rs` | Añadir dispatch para info classes 35–36 (query) y 43–47 (set) sobre `\Device\Keyboard` |
| `src/syscall/mod.rs` | Eliminar `KEYBOARD_LAYOUT: AtomicU8` (reemplazado por NeoKBD) |
| `src/eventbus/mod.rs` | Añadir `EVENT_KEYDOWN = 27`, `EVENT_KEYUP = 28`, `EVENT_KEY_CHAR = 29`, `EVENT_KBD_MODIFIER = 30`, `EVENT_KBD_REPEAT = 31` |
| `src/main.rs` | Añadir PHASE 3.875: `kbd::kbd_init()` (después de input init, antes de driver loader) |
| `src/drivers/boot_loader/mod.rs:165-172` | Mantener registro de PS2KBD para EVENT_KEYBOARD_INPUT (NeoKBD será handler adicional o reemplazo) |
| `src/cm/mod.rs` | Añadir defaults Registry: `cm_ensure_default_values()` crear `\Registry\Machine\System\Keyboard\*` |
| `drivers/ps2kbd/src/lib.rs` | **Simplificar**: eliminar layout tables, dead keys, UTF-8 encode. Mantener solo modifier tracking + scancode→event. Emitir eventos estructurados via nueva HST. |
| `libneodos/src/keyboard.rs` (nuevo) | Wrappers: `kbd_get_layout()`, `kbd_set_layout()`, `kbd_list_layouts()`, `kbd_get_repeat()`, `kbd_set_repeat()`, `kbd_get_state()`, `kbd_set_leds()` |
| `libneodos/src/lib.rs` | Añadir `pub mod keyboard;` |
| `userbin/keyb/src/main.rs` | Actualizar para usar nuevas APIs libneodos |
| `userbin/neocfg/` | Módulo Keyboard actualizado para usar `kbd_list_layouts()`, `kbd_get_layout()` |
| `docs/syscalls.md` | Documentar nuevos info classes |
| `docs/objects.md` | Documentar ObType::KeyboardDevice |

### 3.6 Formato de layout `.kbd`

Archivo binario para carga dinámica desde `C:\System\Keyboard\*.kbd` (no compilado en el driver).

```text
┌──────────────────────────────────────────────┐
│ Magic: "KBD\0" (4 bytes)                     │
│ Version: u32 LE = 1                          │
│ Name: [32] UTF-8 null-terminated             │
│   ej. "US", "Spanish", "French"              │
│ LangTag: [16] UTF-8 null-terminated          │
│   ej. "en-US", "es-ES", "fr-FR"             │
│ ScancodeCount: u32 LE (max 256)              │
│ ┌─── KeyTable[ScancodeCount] ────────────┐  │
│ │  KeyEntry:                               │  │
│ │    normal: u16 (codepoint o 0xFFFF si no)│  │
│ │    shift: u16                            │  │
│ │    altgr: u16                            │  │
│ │    ctrl: u16                             │  │
│ │    flags: u8  (bit 0=dead_normal         │  │
│ │                 bit 1=dead_shift         │  │
│ │                 bit 2=dead_altgr         │  │
│ │                 bit 3=virt_key)          │  │
│ │    _pad: [7]                             │  │  (aligned to 16 bytes)
│ └─────────────────────────────────────────┘  │
│ ComposeCount: u32 LE                         │
│ ┌─── ComposeTable[ComposeCount] ─────────┐   │
│ │  ComposeEntry: { dead: u16, base: u16,  │   │
│ │                  result: u16 }           │   │
│ └─────────────────────────────────────────┘   │
└──────────────────────────────────────────────┘
```

Tamaño por layout (~256 scancodes): `16 + 32 + 16 + 4 + (256×16) + 4 + (N×6)` ≈ 4.2 KB + compose tables.

### 3.7 Registry structure

```text
\Registry\Machine\System\Keyboard
├── Layout (REG_SZ) = "Spanish"        # nombre del layout activo
├── RepeatDelay (REG_DWORD) = 500       # ms antes de repetir (250-1000)
├── RepeatRate (REG_DWORD) = 30         # caracteres/segundo (2-60)
├── NumLockOnBoot (REG_DWORD) = 1       # estado NumLock al arrancar
├── CapsLockOnBoot (REG_DWORD) = 0      # estado CapsLock al arrancar
├── ScrollLockOnBoot (REG_DWORD) = 0    # estado ScrollLock al arrancar
└── Hotkeys\                            # (futuro)
    └── (hive para hotkeys configurables)
```

### 3.8 Nuevos tipos/structs

```rust
// src/kbd/mod.rs
pub struct NeoKbd {
    state: KbdState,
    config: KbdConfig,
    active_layout: KbdLayoutRef,
    layouts: Vec<KbdLayoutRef>,     // layouts cargados desde disco
    modifiers: ModifierState,
    dead_key: Option<u16>,
    repeat: RepeatState,
    hotkeys: HotkeyRegistry,
}

#[repr(C)]
pub struct KbdState {
    pub modifiers: u8,      // bitmask: Shf, Ctrl, Alt, AltGr, Caps, Num, Scr
    pub leds: u8,           // bitmask: CapsLock, NumLock, ScrollLock
    pub active_layout_index: u32,
}

#[repr(C)]
pub struct KbdCaps {
    pub max_layouts: u32,
    pub supports_repeat_config: bool,
    pub supports_led_control: bool,
    pub supports_hotkeys: bool,
    pub num_layouts: u32,
}

#[repr(C)]
pub struct KbdLayoutInfo {
    pub index: u32,
    pub name: [u8; 32],       // "Spanish"
    pub lang_tag: [u8; 16],   // "es-ES"
    pub scancode_count: u32,
    pub compose_count: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KeyEntry {
    pub normal: u16,        // codepoint or 0xFFFF = none
    pub shift: u16,
    pub altgr: u16,
    pub ctrl: u16,
    pub flags: u8,          // bit 0 = dead_normal, bit 1 = dead_shift, etc.
    _pad: [u8; 7],
}

pub struct ModifierState {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub altgr: bool,
    pub caps: bool,
    pub num: bool,
    pub scroll: bool,
}
```

### 3.9 Nuevos Info Classes

#### ObInfoClass (query)

| Clase | Nombre | Descripción | Tamaño buf |
| --- | --- | --- | --- |
| 35 | `KeyboardInfo` | Estado actual: modificadores + layout activo + LEDs | 12 bytes (`KbdState`) |
| 36 | `KeyboardCaps` | Capacidades del teclado y gestor | 20 bytes (`KbdCaps`) |
| 37 | `KeyboardLayouts` | Lista de layouts disponibles | Array de `KbdLayoutInfo` |

#### ObSetInfoClass (set)

| Clase | Nombre | Descripción | buf |
| --- | --- | --- | --- |
| 43 | `KeyboardSetLayout` | Cambiar layout por nombre | `[u8; 32]` nombre |
| 44 | `KeyboardSetRepeatDelay` | Tiempo antes de repetir (ms) | `u32` (250-1000) |
| 45 | `KeyboardSetRepeatRate` | Velocidad de repetición (cps) | `u32` (2-60) |
| 46 | `KeyboardSetLeds` | Forzar estado LEDs | `u8` bitmask |
| 47 | `KeyboardSetModifier` | Forzar estado modificador (debug) | `u8` bitmask |

### 3.10 Nuevos eventos Event Bus

| Constante | Valor | Descripción | data0 | data1 |
| --- | --- | --- | --- | --- |
| `EVENT_KEYDOWN` | 27 | Tecla presionada | scancode (u8) | modifiers (u8) |
| `EVENT_KEYUP` | 28 | Tecla liberada | scancode (u8) | modifiers (u8) |
| `EVENT_KEY_CHAR` | 29 | Carácter Unicode generado | codepoint (u32) | scancode (u8) |
| `EVENT_KBD_MODIFIER` | 30 | Cambio de modificador | modifiers new (u8) | modifiers old (u8) |
| `EVENT_KBD_REPEAT` | 31 | Auto-repeat generó carácter | codepoint (u32) | repeat count |

(Vafor 27–31, después de 26 que es el último planificado para Power Manager en `docs/power-manager.md`.)

### 3.11 API pública (libneodos)

```rust
// libneodos/src/keyboard.rs (NUEVO)

/// Obtiene el layout activo (nombre, ej. "Spanish")
pub fn kbd_get_layout() -> Result<[u8; 32], i64>;

/// Cambia el layout activo por nombre
pub fn kbd_set_layout(name: &str) -> Result<(), i64>;

/// Lista los layouts instalados en el sistema
pub fn kbd_list_layouts() -> Result<Vec<KbdLayoutInfo>, i64>;

/// Obtiene configuración de auto-repeat
pub fn kbd_get_repeat() -> Result<(u32, u32), i64>;  // (delay_ms, rate_cps)

/// Configura auto-repeat
pub fn kbd_set_repeat(delay_ms: u32, rate_cps: u32) -> Result<(), i64>;

/// Obtiene el estado actual del teclado (modificadores + LEDs)
pub fn kbd_get_state() -> Result<KbdState, i64>;

/// Fuerza el estado de los LEDs
pub fn kbd_set_leds(leds: u8) -> Result<(), i64>;

// Tipos de datos exportados
#[repr(C)]
pub struct KbdState {
    pub modifiers: u8,
    pub leds: u8,
    pub active_layout_index: u32,
}

#[repr(C)]
pub struct KbdLayoutInfo {
    pub index: u32,
    pub name: [u8; 32],
    pub lang_tag: [u8; 16],
    pub scancode_count: u32,
    pub compose_count: u32,
}

// Constantes de modificadores
pub const KBD_SHIFT: u8      = 0x01;
pub const KBD_CTRL: u8       = 0x02;
pub const KBD_ALT: u8        = 0x04;
pub const KBD_ALTGR: u8      = 0x08;
pub const KBD_CAPS: u8       = 0x10;
pub const KBD_NUMLOCK: u8    = 0x20;
pub const KBD_SCROLLLOCK: u8 = 0x40;
```

### 3.12 Integración con flujo existente

El cambio es **progresivo**:

**Fase 1 (NeoKBD como superposición):**

- NeoKBD se registra como handler adicional de `EVENT_KEYBOARD_INPUT`
- El ps2kbd NEM driver sigue traduciendo y pushing bytes como hoy
- NeoKBD procesa los mismos eventos, mantiene estado, expone API vía Ob, pero NO interfiere con la salida
- Las aplicaciones legacy (neoshell, KEYB) siguen funcionando igual
- Las nuevas APIs permiten consultar layout por nombre, listar layouts, configurar repeat

**Fase 2 (NeoKBD como gestor principal):**

- El ps2kbd NEM driver se simplifica: elimina layout tables, dead keys, UTF-8 encode
- ps2kbd solo trackea modificadores y emite eventos (scancode + make/break)
- NeoKBD recibe el evento, traduce, gestiona dead keys, compone Unicode
- NeoKBD llama a `input::push_byte()` con los bytes UTF-8 resultantes (compatible hacia atrás)
- Las nuevas APIs (`kbd_get_state()`, `kbd_set_leds()`) funcionan desde Fase 1

**El flujo de bytes hacia VT queue se mantiene inalterado.** NeoShell y todas las aplicaciones existentes no necesitan cambios.

### 3.13 Múltiples dispositivos de entrada (futuro)

El diseño prepara para múltiples teclados:

```rust
pub struct KeyboardDevice {
    pub device_id: u32,         // Event Bus device_id (3=ps2kbd, 4=usbkbd, ...)
    pub instance: KbdInstance,  // Estado independiente por dispositivo
    pub active: bool,
}

pub struct KbdManager {
    pub devices: Vec<KeyboardDevice>,
    pub global_config: KbdConfig,    // Config compartida
}
```

Cada dispositivo tiene su propio estado de modificadores, dead key pendiente, y layout asignado (por defecto, el global). El `\Device\Keyboard` Ob object expone operaciones que afectan al dispositivo activo o al global.

---

## 4. Alternatives

### Alternative A: Mantener toda la lógica en el NEM driver ps2kbd, extender HST

**Descripción:** En lugar de crear un subsistema de kernel, extender el NEM driver ps2kbd para que exponga funciones vía la HST bridge. Añadir nuevas HST calls: `hst_get_layout()`, `hst_set_layout()`, `hst_get_state()`, `hst_load_layout()`. El kernel delegaría consultas vía Ob al driver.

**Rechazada porque:**

1. **Los NEM drivers son aislados y de capacidades limitadas.** Un driver no debería ser el repositorio central de configuración del sistema. No tiene acceso natural al Registry (tendría que ser vía HST calls).
2. **Disponibilidad.** Si el driver ps2kbd falla (Faulted state), se pierde toda la API de teclado, incluyendo consultas de solo lectura.
3. **Latencia de consulta.** Cada `hst_*` call requiere validación de capacidades (`check_cap(CAP_INPUT)`), cambio de dominio (kernel→driver→kernel), que añade ~1-5μs por llamada.
4. **Carga de layouts.** El driver NEM tiene un espacio aislado de 1 MB en `DRIVER_ISO`. Cargar múltiples layouts (cada uno ~4 KB) fragmentaría su memoria. El kernel tiene el heap global para estos datos.
5. **Consistencia de estado.** El driver mantiene SU estado atómico (`MODS`, `LAYOUT`, `DEAD_KEY`). Si el kernel necesita consultarlo, hay latencia y posible incoherencia. Con estado en kernel, es inmediato y atómico.
6. **Precedente en el diseño de NeoDOS.** Los objetos del sistema que representan recursos globales (Drivers, Services, Network) son ObType nativos en el kernel, no drivers. El teclado debe seguir el mismo patrón.

### Alternative B: Mantener el modelo actual, solo añadir más Ob info classes sobre `\Global\Info\Keyboard` (ObType::Key)

**Descripción:** Sin nuevo subsistema. Seguir usando `ObType::Key` con `native_id=9` para el teclado. Añadir nuevos `ObInfoClass` variants para consultar más datos (nombre del layout, lista de layouts, estado). Seguir compilando layouts en ps2kbd.

**Rechazada porque:**

1. **Abuso del sistema de tipos.** `ObType::Key` es semánticamente "Registry key". Usarlo para representar un dispositivo de entrada es incorrecto y confunde el mantenimiento. Las comprobaciones en `ob.rs` verifican `obj.obj_type != ObType::Key` para validar el handle — añadir más clases sobre este tipo mezcla dominios.
2. **Escalabilidad.** No se pueden cargar layouts dinámicamente porque el driver NEM los compila estáticamente. Seguiría siendo necesario recompilar el driver para cada layout nuevo.
3. **Sin gestión de modificadores.** El driver mantiene `MODS` internamente. No hay forma de consultarlo desde user space sin añadir HST calls (ver Alternative A).
4. **Deuda técnica.** El hack actual (`ObType::Key` + `native_id=9`) es una solución temporal. Pasar a v1.0 con esta deuda es arriesgado.

---

## 5. Affected Components

| Subsistema | Impacto | Detalles |
| --- | --- | --- |
| **Object Manager** | Medio | Nuevo `ObType::KeyboardDevice(22)`. Nuevo objeto `\Device\Keyboard` en namespace. 2 nuevos query info classes, 5 nuevos set info classes. |
| **Event Bus** | Bajo | 5 nuevos tipos de evento (27–31). No rompe frozen ABI (los tipos nuevos están fuera del rango 0–15). |
| **Input Manager** | Bajo | NeoKBD llama a `input::push_byte()` igual que ps2kbd. Sin cambios en VT queues. |
| **Registry (Cm)** | Bajo | Nuevas claves por defecto en `cm_ensure_default_values()`. |
| **Syscall dispatch (`ob.rs`)** | Medio | Añadir match arms en `handler_ob_query_info` para clases 35–37 y en `handler_ob_set_info` para clases 43–47. Validar ObType::KeyboardDevice. |
| **Syscall globals** | Bajo | Eliminar `KEYBOARD_LAYOUT: AtomicU8`. Reemplazar por `KBD` global. |
| **Boot sequence** | Bajo | Nueva PHASE 3.875: `kbd::kbd_init()`. Después de input init (3.7), antes de driver loader (3.85). |
| **ps2kbd NEM driver** | Alto (Fase 2) | Simplificar: eliminar ~150 líneas de lógica de layout/dead keys/UTF-8. Mantener solo tracking de modificadores y push de eventos. |
| **ps2kbd build.rs** | Alto (Fase 2) | Eliminar generación de kbd_layout.rs. Los layouts ahora son archivos `.kbd`. |
| **Boot loader** | Bajo | Mantener registro de PS2KBD para EVENT_KEYBOARD_INPUT (no cambiar binding). |
| **libneodos** | Medio | Nuevo módulo `keyboard.rs`. 7 nuevas funciones públicas. |
| **userbin/keyb** | Medio | Refactorizar para usar `kbd_list_layouts()`, `kbd_get_layout()` (nombres, no índices). |
| **userbin/neocfg** | Bajo | El módulo Keyboard ya usa nombres de layout (diseñado en `neocfg-design.md`). Sin cambios. |
| **Documentation** | Medio | Actualizar `docs/objects.md`, `docs/syscalls.md`, nuevo `docs/keyboard.md`. |

**No cambian:** Scheduler, VFS, Memory, HAL, Drivers (excepto ps2kbd), ABI freeze (los eventos 27–31 están fuera del rango 0–15 congelado).

---

## 6. API Contract

### 6.1 `ob_query_info(KeyboardInfo = 35)` en `\Device\Keyboard`

- **Args:** `fd` = handle a `\Device\Keyboard`, `class` = 35, `buf` = `&mut KbdState` (12 bytes), `size` ≥ 12
- **Returns:** Bytes escritos (12) en éxito, `-Fault` si buffer < 12
- **Error codes:** `-BadF` si fd no es KeyboardDevice, `-Inval` si class incorrecto
- **Preconditions:** Handle obtenido vía `ob_open("\Device\Keyboard", READ)`. No requiere admin.
- **Side effects:** Ninguno. Solo lectura.

```rust
#[repr(C)]
pub struct KbdState {
    pub modifiers: u8,    // bitmask KBD_SHIFT|KBD_CTRL|KBD_ALT|KBD_ALTGR|KBD_CAPS|KBD_NUMLOCK|KBD_SCROLLLOCK
    pub leds: u8,         // bitmask KBD_CAPS|KBD_NUMLOCK|KBD_SCROLLLOCK
    pub active_layout_index: u32,
}
```

### 6.2 `ob_query_info(KeyboardCaps = 36)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 36, `buf` = `&mut KbdCaps` (20 bytes), `size` ≥ 20
- **Returns:** 20 en éxito, `-Fault` si buffer < 20
- **Error codes:** `-BadF`, `-Inval`
- **Preconditions:** No requiere admin.

```rust
#[repr(C)]
pub struct KbdCaps {
    pub max_layouts: u32,
    pub supports_repeat_config: bool,
    pub supports_led_control: bool,
    pub supports_hotkeys: bool,
    pub num_layouts: u32,
    pub _pad: [u8; 3],
}
```

### 6.3 `ob_enum(KeyboardLayouts = 37)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 37, `buf` = `&mut [KbdLayoutInfo]`, `size` = buf len
- **Returns:** Número de layouts escritos, `-Fault` si buffer demasiado pequeño
- **Nota:** Usa el mecanismo de `ob_query_info` con semántica de array: escribe tantos `KbdLayoutInfo` como quepan en el buffer.
- **Preconditions:** No requiere admin.

### 6.4 `ob_set_info(KeyboardSetLayout = 43)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 43, `buf` = nombre del layout (`[u8; 32]` null-terminated), `size` = len del nombre (1-32)
- **Returns:** 0 en éxito, `-NoEnt` si el nombre no corresponde a ningún layout cargado, `-Inval` si nombre vacío
- **Preconditions:** Handle obtenido vía `ob_open("\Device\Keyboard", WRITE)`. No requiere admin (por defecto; futuro: requerir admin para cambio global).
- **Side effects:** Cambia `active_layout` en NeoKBD. Dispara `EVENT_KEYB_LAYOUT` (type 9) al Event Bus. Persiste en Registry `\Registry\Machine\System\Keyboard\Layout`.

### 6.5 `ob_set_info(KeyboardSetRepeatDelay = 44)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 44, `buf` = `&u32` delay en ms, `size` = 4
- **Returns:** 0 en éxito, `-Inval` si delay < 100 o > 2000
- **Preconditions:** Handle con WRITE. No requiere admin.
- **Side effects:** Actualiza `KbdConfig.repeat_delay`. Persiste en Registry `RepeatDelay`.

### 6.6 `ob_set_info(KeyboardSetRepeatRate = 45)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 45, `buf` = `&u32` rate en caracteres/segundo, `size` = 4
- **Returns:** 0 en éxito, `-Inval` si rate < 1 o > 60
- **Preconditions:** Handle con WRITE.
- **Side effects:** Actualiza `KbdConfig.repeat_rate`. Persiste en Registry `RepeatRate`.

### 6.7 `ob_set_info(KeyboardSetLeds = 46)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 46, `buf` = `&u8` bitmask (bits: 0=CapsLock, 1=NumLock, 2=ScrollLock), `size` = 1
- **Returns:** 0 en éxito, `-Inval` si bits inválidos
- **Preconditions:** Handle con WRITE. No requiere admin (per-user).
- **Side effects:** Actualiza LEDs vía PS/2 command. Actualiza `KbdState.leds`. No persiste automáticamente (el estado de LEDs es transitorio).

### 6.8 `ob_set_info(KeyboardSetModifier = 47)` en `\Device\Keyboard`

- **Args:** `fd`, `class` = 47, `buf` = `&u8` modifier bitmask, `size` = 1
- **Returns:** 0 en éxito, `-Inval` si bits inválidos
- **Preconditions:** Handle con WRITE. **Requiere admin** (forzar modificadores puede evadir安全检查).
- **Side effects:** Actualiza `ModifierState` en NeoKBD. Dispara `EVENT_KBD_MODIFIER`.

### 6.9 API de libneodos (contrato de alto nivel)

```rust
// kbd_get_layout()
//   fd = ob_open("\Device\Keyboard", READ)
//   buf = [0u8; 37]
//   ob_query_info(fd, KeyboardInfo, &mut info)
//   ob_query_info con class 37 para obtener KbdLayoutInfo[index]
//   retorna info.name

// kbd_set_layout(name)
//   fd = ob_open("\Device\Keyboard", WRITE)
//   ob_set_info(fd, KeyboardSetLayout, name.as_bytes())

// kbd_list_layouts()
//   fd = ob_open("\Device\Keyboard", READ)
//   ob_query_info(fd, KeyboardLayouts, &mut buf)  // array de KbdLayoutInfo

// kbd_get_repeat()
//   fd = ob_open("\Device\Keyboard", READ)
//   ob_query_info(fd, KeyboardInfo, &mut info)  // layout index
//   (lectura de config interna, futura info class)

// kbd_set_repeat(delay, rate)
//   fd = ob_open("\Device\Keyboard", WRITE)
//   ob_set_info(fd, KeyboardSetRepeatDelay, &delay)
//   ob_set_info(fd, KeyboardSetRepeatRate, &rate)

// kbd_get_state()
//   fd = ob_open("\Device\Keyboard", READ)
//   ob_query_info(fd, KeyboardInfo, &mut KbdState)

// kbd_set_leds(leds)
//   fd = ob_open("\Device\Keyboard", WRITE)
//   ob_set_info(fd, KeyboardSetLeds, &leds)
```

### 6.10 Nuevos eventos Event Bus

```rust
// EVENT_KEYDOWN (27) - tecla presionada
//   data0: scancode (u8), bits superiores reservados
//   data1: modifiers en el momento (u8)

// EVENT_KEYUP (28) - tecla liberada
//   data0: scancode (u8)
//   data1: modifiers (u8)

// EVENT_KEY_CHAR (29) - carácter Unicode generado
//   data0: codepoint Unicode (u32)
//   data1: scancode (u8)

// EVENT_KBD_MODIFIER (30) - cambio de modificador
//   data0: nuevo estado modificadores (u8)
//   data1: viejo estado (u8)

// EVENT_KBD_REPEAT (31) - auto-repeat
//   data0: codepoint (u32)
//   data1: contador de repeticiones (u32)
```

---

## 7. Test Plan

### 7.1 Inicialización y objeto Ob (invariante: NeoKBD se inicializa correctamente y es accesible vía Ob)

| # | Test | Expected |
| --- | --- | --- |
| 1 | `kbd_init()` completa sin error después de PHASE 3.875 | `KBD` global en estado `KbdState { modifiers: 0, leds: 0, active_layout_index: 0 }` |
| 2 | `\Device\Keyboard` existe en namespace Ob | `ob_lookup_path("\Device\Keyboard")` retorna ObId válido |
| 3 | `ob_open("\Device\Keyboard", READ)` en user space retorna fd ≥ 3 | Handle válido, ObType::KeyboardDevice |
| 4 | `ob_query_info(fd, KeyboardInfo)` retorna `KbdState` con layout por defecto | `active_layout_index == 0`, layout cargado desde Registry |

### 7.2 Carga de layouts (invariante: los layouts se cargan desde disco y son consultables)

| # | Test | Expected |
| --- | --- | --- |
| 5 | `C:\System\Keyboard\Spanish.kbd` se carga en init | Layout "Spanish" disponible, lang_tag "es-ES" |
| 6 | `C:\System\Keyboard\US.kbd` se carga en init | Layout "US" disponible, lang_tag "en-US" |
| 7 | `ob_query_info(fd, KeyboardLayouts, buf)` lista ambos layouts | 2 entries: "Spanish" (index 0) y "US" (index 1) |
| 8 | Archivo `.kbd` con magic inválido → ignorado, no crash | Layout no se carga, resto del sistema funciona |
| 9 | `C:\System\Keyboard\` vacío → `kbd_init()` con layout built-in mínimo | Layout por defecto "US" compilado en kernel como fallback |

### 7.3 Cambio de layout (invariante: cambiar layout actualiza el estado y persiste en Registry)

| # | Test | Expected |
| --- | --- | --- |
| 10 | `ob_set_info(fd, KeyboardSetLayout, "US")` → layout activo cambia a US | `KbdState.active_layout_index == 1` |
| 11 | `ob_set_info(fd, KeyboardSetLayout, "Spanish")` → layout activo vuelve a Spanish | `KbdState.active_layout_index == 0` |
| 12 | `ob_set_info(fd, KeyboardSetLayout, "Nonexistent")` retorna `-NoEnt` | Layout activo no cambia |
| 13 | Tras cambio, `Registry\Machine\System\Keyboard\Layout` se actualiza | `cm_query_value("Layout")` == nuevo nombre |
| 14 | Cambio de layout dispara `EVENT_KEYB_LAYOUT` | Handler registrado recibe event con layout index |

### 7.4 Traducción scancode→Unicode (invariante: NeoKBD traduce correctamente usando el layout activo)

| # | Test | Expected |
| --- | --- | --- |
| 15 | Scancode 0x1E (A) con layout US → `input::push_byte(b'a')` | Byte 'a' (0x61) en VT queue |
| 16 | Scancode 0x1E (A) con layout US + Shift → `input::push_byte(b'A')` | Byte 'A' (0x41) en VT queue |
| 17 | Scancode 0x27 (Ñ) con layout Spanish → `input::push_byte(0xC3)` + `input::push_byte(0xB1)` | UTF-8 de U+00F1 (ñ) en VT queue |
| 18 | Scancode 0x0C (´) + luego 0x1E (a) con layout Spanish → carácter compuesto á | `input::push_byte(0xC3)` + `input::push_byte(0xA1)` = UTF-8 de U+00E1 |

### 7.5 Modificadores (invariante: el estado de modificadores es correcto y consultable)

| # | Test | Expected |
| --- | --- | --- |
| 19 | Scancode 0x2A (Shift make) → `KbdState.modifiers` bit KBD_SHIFT activo | `modifiers & KBD_SHIFT != 0` |
| 20 | Scancode 0xAA (Shift break) → `KbdState.modifiers` bit KBD_SHIFT inactivo | `modifiers & KBD_SHIFT == 0` |
| 21 | Scancode 0x3A (CapsLock make) → toggle Caps bit | `modifiers & KBD_CAPS` cambia de 0→1 o 1→0 |
| 22 | `ob_set_info(fd, KeyboardSetModifier, &KBD_CAPS)` fuerza CapsLock | `modifiers & KBD_CAPS != 0` |
| 23 | `ob_set_info(fd, KeyboardSetModifier, &KBD_CTRL)` requiere admin | `-Perm` si token no admin |

### 7.6 Dead keys y composición (invariante: dead keys componen correctamente)

| # | Test | Expected |
| --- | --- | --- |
| 24 | Dead key ´ (U+00B4) seguido de a → compone á (U+00E1) | Carácter U+00E1 emitido |
| 25 | Dead key ¨ (U+00A8) seguido de o → compone ö (U+00F6) | Carácter U+00F6 emitido |
| 26 | Dead key ´ seguido de otra dead key ´ → compone ´´ (no hay combinación) → "?" | 0x3F emitido |
| 27 | Dead key sin continuación (sin segunda tecla) → nada emitido, dead key reset | No hay salida, `dead_key == None` |

### 7.7 Auto-repeat (invariante: auto-repeat configurable funciona correctamente)

| # | Test | Expected |
| --- | --- | --- |
| 28 | Tecla mantenida > delay configurado → repite a la tasa configurada | N repeticiones en N/rate segundos |
| 29 | `ob_set_info(fd, KeyboardSetRepeatDelay, 1000)` → delay 1 segundo | `KbdConfig.repeat_delay == 1000` |
| 30 | `ob_set_info(fd, KeyboardSetRepeatDelay, 50)` retorna `-Inval` | delay no cambia |
| 31 | Tecla liberada durante auto-repeat → repeticiones cesan inmediatamente | No más `EVENT_KBD_REPEAT` |

### 7.8 LEDs (invariante: control de LEDs funciona y estado es consultable)

| # | Test | Expected |
| --- | --- | --- |
| 32 | CapsLock ON → LED CapsLock encendido vía PS/2 command 0xED | `set_leds(0x04)` llamado |
| 33 | `ob_set_info(fd, KeyboardSetLeds, &0x02)` → NumLock LED toggle | LED NumLock cambia, `KbdState.leds & 0x02` coincide |
| 34 | `ob_query_info(fd, KeyboardInfo)` → `leds` refleja estado actual | `KbdState.leds` bits correctos |

### 7.9 Persistencia en Registry (invariante: configuración persiste entre reinicios)

| # | Test | Expected |
| --- | --- | --- |
| 35 | Cambiar layout a "US", reiniciar → layout activo "US" | Registry leído en kbd_init() |
| 36 | RepeatDelay=750, RepeatRate=15, reiniciar → valores recuperados | cm_query_value confirma |
| 37 | Registry vacío (primer arranque) → defaults aplicados | Layout="Spanish", Delay=500, Rate=30 |

### 7.10 Integración con user space (invariante: aplicaciones existentes no se rompen)

| # | Test | Expected |
| --- | --- | --- |
| 38 | KEYB US desde NeoShell → layout cambia a US | `kbd_get_layout()` retorna "US" |
| 39 | KEYB SP desde NeoShell → layout cambia a Spanish | `kbd_get_layout()` retorna "Spanish" |
| 40 | Ctrl+Alt+Del → poweroff (hotkey manejado por NeoKBD) | Sistema se apaga |
| 41 | Alt+F2 en NeoShell → VT switch a VT2 | `active_vt()` retorna 1 |
| 42 | Escribir en NeoShell con layout Spanish → ñ aparece correctamente | Carácter UTF-8 válido en shell |

### 7.11 Tests de carga de archivos .kbd

| # | Test | Expected |
| --- | --- | --- |
| 43 | Cargar archivo `.kbd` con 256 scancodes completos | Todos los scancodes mapeados |
| 44 | Cargar archivo `.kbd` con compose table de 54 entradas (Spanish) | Dead keys + compose funcionan |
| 45 | Cargar archivo `.kbd` malformado (tamaño insuficiente) | Error `-Inval`, no crash |

---

## 8. Implementation Plan

### Step 1: Kernel module scaffolding (1 day)

**Archivos:** `src/kbd/mod.rs`, `src/kbd/layout.rs`

1. Crear `src/kbd/mod.rs`:
   - struct `NeoKbd` con state, config, layouts vec, modifiers, dead_key
   - `pub static KBD: NeoKbd` (o `Mutex<NeoKbd>`)
   - `pub fn kbd_init()`: escanea `C:\System\Keyboard\*.kbd`, carga layouts, aplica defaults
   - `pub fn kbd_event(scancode: u8)`: procesa un scancode raw (make → lookup composición → push byte)
2. Crear `src/kbd/layout.rs`:
   - `KbdLayout` struct con nombre, lang_tag, `[KeyEntry; 256]`, `Vec<ComposeEntry>`
   - `load_kbd(path: &str) -> Result<KbdLayout, ()>`: parsea archivo `.kbd` binario
   - `lookup(scancode, mods) -> Option<u16>`: retorna codepoint para scancode + modifiers
3. Añadir `pub mod kbd;` a `src/lib.rs`
4. Añadir PHASE 3.875 en `main.rs` (después de `input::init()`, antes de driver loader):

```rust
// PHASE 3.875: Keyboard Manager (NeoKBD)
kbd::kbd_init();
```

### Step 2: Registry defaults (0.25 day)

**Archivos:** `src/cm/mod.rs`

1. Añadir en `cm_ensure_default_values()`:

```rust
// \Registry\Machine\System\Keyboard
//   Layout = "Spanish" (REG_SZ)
//   RepeatDelay = 500 (REG_DWORD)
//   RepeatRate = 30 (REG_DWORD)
//   NumLockOnBoot = 1 (REG_DWORD)
//   CapsLockOnBoot = 0 (REG_DWORD)
```

### Step 3: ObType and namespace (0.5 day)

**Archivos:** `src/object/types.rs`, `src/kbd/mod.rs`

1. Añadir `KeyboardDevice = 22` a `ObType` enum + `to_str()` match arm
2. Añadir `KeyboardInfo = 35`, `KeyboardCaps = 36` a `ObInfoClass`
3. Añadir `KeyboardSetLayout = 43`, `KeyboardSetRepeatDelay = 44`, `KeyboardSetRepeatRate = 45`, `KeyboardSetLeds = 46`, `KeyboardSetModifier = 47` a `ObSetInfoClass`
4. En `kbd_init()`: crear `\Device\Keyboard` objeto Ob:

```rust
let kbd_id = object::ob_create_object(ObType::KeyboardDevice, "Keyboard", 0, 0, None);
object::namespace::ob_insert_object("\\Device\\Keyboard", kbd_id);
```

### Step 4: Syscall dispatch (1 day)

**Archivos:** `src/syscall/ob.rs`

1. En `handler_ob_query_info`:
   - Añadir match arm para clase 35 (KeyboardInfo): escribir `KbdState`
   - Añadir match arm para clase 36 (KeyboardCaps): escribir `KbdCaps`
   - Validar que el fd apunte a ObType::KeyboardDevice
2. En `handler_ob_set_info`:
   - Añadir match arm para clase 43 (KeyboardSetLayout): cambiar layout por nombre
   - Añadir match arm para clase 44 (KeyboardSetRepeatDelay): configurar delay
   - Añadir match arm para clase 45 (KeyboardSetRepeatRate): configurar rate
   - Añadir match arm para clase 46 (KeyboardSetLeds): forzar LEDs
   - Añadir match arm para clase 47 (KeyboardSetModifier): forzar modificador (admin check)

### Step 5: Event Bus integration (0.5 day)

**Archivos:** `src/eventbus/mod.rs`, `src/kbd/event.rs`, `src/kbd/hotkey.rs`

1. Añadir constantes `EVENT_KEYDOWN=27`, `EVENT_KEYUP=28`, `EVENT_KEY_CHAR=29`, `EVENT_KBD_MODIFIER=30`, `EVENT_KBD_REPEAT=31`
2. Crear `src/kbd/event.rs`:
   - Registrar NeoKBD como handler de `EVENT_KEYBOARD_INPUT`
   - `kbd_event_handler()`: recibe evento, llama a `kbd_process_scancode()`
3. Crear `src/kbd/hotkey.rs`:
   - Hotkey: Ctrl+Alt+Del → sys_poweroff (reemplaza el chequeo en IDT handler)
   - Hotkey: Alt+F1-F4 → VT switch (reemplaza el chequeo en IDT handler)
4. Simplificar `keyboard_handler` en `idt.rs`: eliminar Ctrl+Alt+Del y Alt+F# check (NeoKBD los maneja vía eventos)

### Step 6: Unicode mapper + dead keys (0.5 day)

**Archivos:** `src/kbd/unicode.rs`

1. `scancode_to_unicode(scancode, mods, layout) -> Option<u32>`:
   - Lookup en layout por scancode + modifiers
   - Si dead key → almacenar en `dead_key`, retornar None
   - Si dead_key + base → compose vía `ComposeEntry` lookup
   - Si no compose match → retornar `Some(b'?')`
2. `unicode_to_utf8(codepoint: u32) -> [u8; 4]`:
   - Convertir codepoint a UTF-8 sequence (0-3 continuation bytes)
   - Encadenar llamadas a `input::push_byte()` por cada byte

### Step 7: Auto-repeat (0.5 day)

**Archivos:** `src/kbd/repeat.rs`

1. Cuando una tecla se mantiene presionada (KeyDown sin KeyUp), iniciar timer:
   - Esperar `repeat_delay` ms
   - Emitir carácter cada `1000/repeat_rate` ms
2. Al recibir KeyUp: cancelar timer
3. Configuración leída desde `KbdConfig` (cargado de Registry en init, actualizable vía Ob)

### Step 8: Simplificar ps2kbd NEM driver (Fase 2, 1 day)

**Archivos:** `drivers/ps2kbd/src/lib.rs`, `drivers/ps2kbd/build.rs`, `drivers/ps2kbd/layouts/`

1. Eliminar de `lib.rs`:
   - `klc_layout` module include
   - `translate_scancode()` (reemplazar por evento raw)
   - `encode_utf8_first()`
   - `LAYOUT` atomic
   - `DEAD_KEY` atomic
   - `OUTPUT_PENDING0/1` atomics
2. Simplificar `process_scancode()`:
   - Actualizar modificadores (make → set, break → clear, toggle para Caps/Num)
   - Emitir evento vía nueva función `hst_push_key_event(scancode, make_break)`
   - No llamar a `hst_push_input_byte()` directamente
3. Añadir nueva HST call `hst_push_key_event(scancode: u8, is_make: u8)`:
   - Llama a `kbd::kbd_event(scancode, is_make)`
   - NeoKBD hace el resto (traducción, composición, push bytes)

### Step 9: libneodos wrappers (0.5 day)

**Archivos:** `libneodos/src/keyboard.rs` (nuevo), `libneodos/src/lib.rs`

1. `keyboard.rs`: implementar las 7 funciones públicas usando `ob_open("\Device\Keyboard")` + `ob_query_info`/`ob_set_info`
2. `lib.rs`: añadir `pub mod keyboard;`
3. Tipos `KbdState`, `KbdLayoutInfo` exportados

### Step 10: Actualizar userbin/keyb (0.25 day)

**Archivos:** `userbin/keyb/src/main.rs`

1. Cambiar de `KEYB US`/`KEYB SP` (índices) a `KEYB "US"`/`KEYB "Spanish"` (nombres)
2. `KEYB /L`: listar layouts disponibles (usa `kbd_list_layouts()`)
3. `KEYB /I`: mostrar información del layout actual (nombre, lang_tag)
4. Mantener compatibilidad hacia atrás: `KEYB 0` → "US", `KEYB 1` → "Spanish"

### Step 11: Tests de integración (0.5 day)

**Archivos:** Tests kernel en `src/kbd/mod.rs` (bloque `#[test_case]`)

1. Implementar tests de las secciones 7.1 a 7.11
2. Tests de carga de archivos `.kbd` de ejemplo (US, Spanish)
3. Tests de traducción scancode→Unicode para ambos layouts
4. Verificar: `cargo build` + `python3 scripts/auto_test.py`

### Step 12: Documentación (0.5 day)

**Archivos:** `docs/objects.md`, `docs/syscalls.md`, `docs/keyboard.md` (nuevo)

1. `docs/objects.md`: documentar `ObType::KeyboardDevice(22)`, `\Device\Keyboard`
2. `docs/syscalls.md`: documentar info classes 35–37, 43–47
3. `docs/keyboard.md`: guía de arquitectura, cómo añadir layouts, API, integración con NeoCfg

### Total estimated effort: ~7.5 days (Fase 1: 5.5 días, Fase 2: 2 días)

---

## Appendix A: Archivos .kbd soportados inicialmente

Los layouts se generan desde los mismos archivos `.klc` existentes, pero convertidos a formato `.kbd` con una herramienta (`tools/kbdcompile/`):

| Archivo | Layout | LangTag | Scancodes | Compose |
| --- | --- | --- | --- | --- |
| `US.kbd` | US | en-US | 256 | 0 |
| `Spanish.kbd` | Spanish | es-ES | 256 | 54 |

La herramienta `kbdcompile` toma un `.klc` y produce un `.kbd` binario. Se invoca en el build de la imagen, no en runtime.

---

## Appendix B: Mapa de migración ps2kbd → NeoKBD

```text
Fase 1                          Fase 2 (target)
══════════════════════════       ══════════════════════════
ps2kbd: layout + dead keys      ps2kbd: solo raw scancodes
ps2kbd: hst_push_input_byte()   ps2kbd: hst_push_key_event()
NeoKBD: API + estado (consulta) NeoKBD: API + estado + TRADUCCIÓN
NeoKBD: NO traduce (solo API)   NeoKBD: layout engine + dead keys
KEYBOARD_LAYOUT eliminado       KEYBOARD_LAYOUT eliminado
layout via Registry + .kbd      layout via Registry + .kbd
2 layouts (US, SP)              N layouts (carga dinámica)
ObType::KeyboardDevice          ObType::KeyboardDevice
\Global\Info\Keyboard (legado)  \Global\Info\Keyboard (eliminar)
```

La Fase 1 no rompe nada existente. La Fase 2 elimina el driver NEM legacy de traducción.

---

*Este documento constituye la especificación de diseño de NeoKBD v0.1.*
*No se implementará código hasta la aprobación del ARB.*
*Ver `docs/IMPROVEMENTS.md` para tracking del roadmap.*
