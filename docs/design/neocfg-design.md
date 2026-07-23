# NeoCfg — Panel de Control de NeoDOS

> **Versión:** v0.1
> **Estado:** Diseño completo
> **Versión de NeoDOS:** v0.49+ (depende de PM-PHASE1+2 para Power, i18n runtime para Locale)
> **Precedencia:** Este documento es la especificación. No se implementa código hasta aprobación.

---

## 1. Research

### 1.1 Auditoría de subsistemas existentes

| Subsistema | Estado actual | APIs públicas disponibles | Documento de diseño |
| --- | --- | --- | --- |
| **Object Manager** | ✅ Completo (ObType 0–20, RAX 60–66) | `ob_open`, `ob_create`, `ob_query_info`, `ob_set_info`, `ob_enum`, `ob_wait`, `ob_destroy` | `docs/objects.md` |
| **Registry (Cm)** | ✅ Completo (RAX 67–76, cell-based hive) | `cm_open_key`, `cm_create_key`, `cm_query_value`, `cm_set_value`, `cm_enum_key`, `cm_enum_value`, `cm_delete_key`, `cm_flush_key` | `docs/registry.md` |
| **Service Manager** | ✅ Completo (ObType::Service=20, RAX 77) | `sys_ob_service` con START/STOP/RESTART/QUERY_STATUS/SET_CONFIG | `docs/syscalls.md` |
| **Keyboard layout** | ✅ Completo (ObInfoClass::KeyboardLayout=14, ObSetInfoClass::KeyboardLayout=5) | `ob_query_info(KeyboardLayout)`, `ob_set_info(KeyboardLayout)` | `docs/objects.md` |
| **System info** (Version, Memory, CPU, DateTime, Drives) | ✅ Completo via `\Global\Info\*` objects | `ob_open` + `ob_query_info` con clases 7–11 | `docs/objects.md` |
| **Power Manager** | ❌ No implementado (diseño en `docs/power-manager.md`) | Propuesto: ObType::PowerManager=21, info classes 32–34 y 37–42 | `docs/power-manager.md` |
| **i18n/Locale** | ❌ No implementado (diseño en `docs/design/i18n-design.md`) | Propuesto: `i18n.rs` en libneodos, formato NLT, fallback chain | `docs/design/i18n-design.md` |
| **Network** | ❌ Parcial (TCP/UDP sockets via ObType::Socket=18, info classes 17–20, 23) | `ob_socket_*` wrappers, `ipconfig.nxe`, `dhcpd.nxe` | — |
| **Users/Groups/Security** | ❌ Parcial (SAM database, Token/ACL, pero sin sesiones ni grupos completos) | USR-P1a+1b+1c+1d en roadmap v0.51 | `docs/security.md` |
| **Storage/NeoFS** | ✅ Parcial (NeoFS v2, FSCK, volume label) | `ob_query_info(VolumeLabel=16)`, `ob_set_info(SetVolumeLabel=9)`, `sys_fsck` | `docs/filesystem.md` |

### 1.2 Patrón de aplicaciones Ring 3 existentes

Todas las 38 aplicaciones .NXE en `userbin/` siguen el mismo patrón:

```rust
#![no_std]
#![no_main]

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Leer args (opcional)
    // 2. Usar libneodos::syscall, libneodos::io, libneodos::console
    // 3. libneodos::syscall::exit(0)
}
```

Dependen de `libneodos`, target `x86_64-unknown-none`, linker script `user.ld`, y se incluyen en la imagen vía `scripts/create_ne2_image.py`.

### 1.3 APIs específicas para cada módulo de NeoCfg

| Módulo | APIs existentes | APIs necesarias (no existen) |
| --- | --- | --- |
| System | `ob_query_info(Version=8)`, `ob_query_info(Memory=10)`, `ob_query_info(CpuInfo=7)`, `ob_query_info(DateTime=9)`, `ob_open(\Global\Info\Drives)` + `ob_query_info(Drives=11)`, `ob_enum(\Ob\Process)` | Ninguna |
| Power | — | Power Manager: `ob_set_info(PowerShutdown=37)`, `ob_set_info(PowerReboot=38)`, `ob_set_info(PowerSetPlan=41)`, `ob_query_info(PowerPlanInfo=32)`, `ob_query_info(PowerStatus=33)` |
| Locale | — | i18n runtime: `i18n_init()`, `i18n_load()`, `i18n_get()`, `i18n_available_locales()` |
| Keyboard | `ob_set_info(fd, KeyboardLayout=5, &[layout])`, `ob_query_info(fd, KeyboardLayout=14, &mut buf)` | Ninguna |
| About | `ob_query_info(Version=8)` (vía `\Global\Info\Version`) | Ninguna |

---

## 2. Problem Analysis

### 2.1 Limitación actual

NeoDOS carece de una **interfaz de configuración unificada**. Actualmente:

1. **Configuración dispersa**: cada aspecto se configura desde una herramienta diferente:
   - `KEYB US/SP` para el teclado (no hay `LANG` o `POWERCFG`)
   - `POWEROFF` para apagar (sin opción de plan de energía o reinicio)
   - Comandos de shell aislados (`PRI`, `KILL`, `VOL`, `LABEL`, `KEYB`) sin interfaz común
   - `neoinit` lee configuración del Registry directamente via `sys_cm_*`
   - `NDREG` para drivers, `neotop` para procesos — herramientas independientes

2. **Sin panel de control**: no existe un punto de entrada único donde un administrador pueda:
   - Ver información del sistema
   - Cambiar el plan de energía
   - Modificar el idioma/teclado
   - Gestionar servicios

3. **Internacionalización ausente**: todos los mensajes de todas las aplicaciones están hardcodeados en inglés. No hay infraestructura para traducción.

4. **Sin preparación para GUI**: cada herramienta de consola existente gestiona su propia UI de forma ad-hoc. Una futura interfaz gráfica requeriría reescribir cada una.

### 2.2 Por qué las abstracciones existentes no resuelven el problema

- **NeoShell** es un intérprete de comandos genérico. Su propósito es ejecutar comandos, no agrupar configuración en flujos guiados (menús, wizard).
- **NeoReg** es un editor del Registry. Expone la complejidad interna (claves, valores, tipos). NeoCfg debe ocultar esa complejidad.
- **Los comandos individuales** (KEYB, VOL, PRI, etc.) operan cada uno sobre un subsistema. No hay coordinación entre ellos, ni interfaz consistente.
- **libneodos** proporciona las APIs de bajo nivel (ob_open, ob_set_info) pero no ofrece wrappers de alto nivel para "configurar teclado" o "ver estado del sistema".

NeoCfg llena este vacío: **una aplicación Ring 3 que consume las APIs públicas de libneodos y las presenta en una interfaz de menús coherente**, preparada para que una futura GUI reutilice exactamente la misma lógica.

---

## 3. Solution Design

### 3.1 Principios arquitectónicos

1. **Solo APIs públicas.** NeoCfg nunca accede al kernel directamente ni al Registry vía Cm. Toda operación usa libneodos.
2. **Separación UI/lógica.** Cada módulo expone un trait común `CfgModule`; la UI (menús) y la lógica (llamadas a libneodos) están separadas.
3. **Internacionalización nativa.** Desde el diseño, todos los textos visibles se cargan vía `i18n_get()`/`tr!()`. Ningún string literal en el código fuente.
4. **Preparado para GUI.** La lógica de los módulos (en `modules/`) es reutilizable por una futura GUI sin cambios. Solo la capa de presentación en `ui/` cambiaría.
5. **Modular por subsistema.** Cada módulo es independiente. Añadir uno nuevo no requiere modificar los existentes.

### 3.2 Arquitectura general

```text
NeoCfg (.NXE user binary)
│
├── main.rs                 ← Entry point, módulo de navegación principal
├── lib.rs                  ← CfgModule trait, tipos compartidos
│
├── ui/
│   ├── menu.rs             ← Renderizado de menús (título, opciones, navegación)
│   └── dialog.rs           ← Diálogos: input, confirmación, mensaje
│
├── modules/
│   ├── system.rs           ← Información del sistema (solo lectura)
│   ├── power.rs            ← Planes de energía (vía Power Manager API)
│   ├── locale.rs           ← Idioma del sistema (vía i18n runtime)
│   ├── keyboard.rs         ← Distribución de teclado (vía Ob API)
│   ├── about.rs            ← Versiones del sistema (solo lectura)
│   └── mod.rs              ← Registry de módulos (CfgModule trait impls)
│
├── i18n/                   ← Archivos .nlt por idioma (no código)
│   ├── en-US/
│   │   └── neocfg.nlt
│   └── es-ES/
│       └── neocfg.nlt
│
└── Cargo.toml              ← Dependencia: libneodos
```

### 3.3 `CfgModule` trait

```rust
/// Cada módulo de NeoCfg implementa este trait.
pub trait CfgModule {
    /// Nombre corto del módulo (clave i18n, ej. "module.system.name").
    fn name(&self) -> &'static str;

    /// Descripción breve (clave i18n, ej. "module.system.desc").
    fn description(&self) -> &'static str;

    /// Renderiza la interfaz del módulo y maneja la interacción.
    /// Retorna cuando el usuario sale (Esc o selección de salida).
    fn run(&self, ui: &mut MenuContext) -> CfgResult;
}
```

Cada módulo se registra en un `static MODULES: &[&dyn CfgModule]` en `modules/mod.rs`.

### 3.4 Menú principal

```text
=========================================
          tr!("neocfg.title")
          NeoCfg v0.1
=========================================

1.  tr!("module.system.name")     ← System
2.  tr!("module.power.name")      ← Power
3.  tr!("module.locale.name")     ← Locale
4.  tr!("module.keyboard.name")   ← Keyboard
5.  tr!("module.about.name")      ← About

A.  tr!("neocfg.about")

Q.  tr!("neocfg.exit")

=========================================
tr!("neocfg.select_hint") >
```

### 3.5 Navegación

| Tecla | Acción |
| --- | --- |
| `1`–`9`, `A` | Seleccionar módulo (por índice) |
| `Q`, `Esc` | Salir de NeoCfg / Volver al menú anterior |
| `↑`/`↓`, `Tab`/`Shift+Tab` | Navegar entre opciones (en diálogos list) |
| `Enter` | Confirmar selección |
| Dentro de un módulo: `Esc` | Volver al menú principal |

Implementación: usar `console::read_byte()` para captura de teclas individuales. No requiere readline.

### 3.6 Módulo System (solo lectura)

```text
===== tr!("module.system.title") =====

tr!("system.kernel_version"):  NeoDOS v0.49.0
tr!("system.build_date"):      2026-07-11
tr!("system.uptime"):          1d 3h 42m
tr!("system.cpu"):             Intel QEMU (fam 6, model 2)
tr!("system.cores"):           4
tr!("system.memory_total"):    1024 MB
tr!("system.memory_used"):     342 MB
tr!("system.memory_free"):     682 MB
tr!("system.drives"):
  C:  NE2   [NeoDOS]    256.0 MB  45% used
  A:  FAT32 [ESP]        64.0 MB  12% used
tr!("system.processes"):      7
tr!("system.services"):       4  (2 running)

[Esc] tr!("neocfg.back")
```

**Implementación**: llamadas secuenciales a `ob_query_info`:

- Version → `ob_open("\Global\Info\Version")` + `ob_query_info(Version=8)`
- Memory → `ob_open("\Global\Info\Memory")` + `ob_query_info(Memory=10)` → `MemInfo`
- CPU → `ob_open("\Global\Info\CpuInfo")` + `ob_query_info(CpuInfo=7)` → `CpuInfoFull`
- DateTime → `ob_open("\Global\Info\DateTime")` + `ob_query_info(DateTime=9)` → `DateTime` (para build date)
- Drives → `ob_open("\Global\Info\Drives")` + `ob_query_info(Drives=11)` → `[DriveInfo]`
- Processes → `ob_enum("\Ob\Process")` → count entries
- Services → `ob_enum("\Service")` + `ob_query_info(ServiceStatus=31)` por servicio

### 3.7 Módulo Power

Dependencia: **Power Manager implementado** (PM-PHASE1+2 de `docs/power-manager.md`).

```text
===== tr!("module.power.title") =====

tr!("power.active_plan"):     tr!("power.plan.balanced")
tr!("power.state"):           tr!("power.state.active")

tr!("power.actions"):
  1. tr!("power.set_plan")
  2. tr!("power.restore_defaults")
  3. tr!("power.shutdown")
  4. tr!("power.reboot")

[Esc] tr!("neocfg.back")
> 1

===== tr!("power.select_plan") =====
1. tr!("power.plan.balanced")       [*]
2. tr!("power.plan.performance")
3. tr!("power.plan.powersaver")
```

**Implementación** (vía Power Manager API, una vez implementado):

- `ob_open("\Device\PowerManager")` → cache fd en static
- Query: `ob_query_info(fd, PowerPlanInfo=32)` → `PowerPlanInfo`
- Set plan: `ob_set_info(fd, PowerSetPlan=41, &plan_index)`
- Shutdown: `ob_set_info(fd, PowerShutdown=37, &[])` (no retorna)
- Reboot: `ob_set_info(fd, PowerReboot=38, &[])` (no retorna)
- Defaults: `ob_set_info(fd, PowerSetPlan=41, &0u32)` (Balanced)

**Hasta que Power Manager exista**, este módulo muestra un mensaje informativo:

```text
tr!("power.not_available")
```

### 3.8 Módulo Locale

Dependencia: **i18n runtime implementado** (Fase 1–2 de `docs/design/i18n-design.md`).

```text
===== tr!("module.locale.title") =====

tr!("locale.current"):        es-ES
tr!("locale.available"):
  1. en-US
  2. es-ES  [*]
  3. ca-ES

tr!("locale.actions"):
  1. tr!("locale.change")
  2. tr!("locale.set_default")

[Esc] tr!("neocfg.back")
```

**Implementación** (vía i18n runtime, una vez implementado):

- `i18n_active_locale()` → muestra locale actual
- `i18n_available_locales()` → lista del directorio `C:\System\Locale\`
- Cambio: escribir `\Registry\Machine\System\CurrentControlSet\Control\Locale\Language` vía `cm_set_value`, luego `i18n_reload_all()`

**Hasta que i18n runtime exista**, este módulo muestra:

```text
tr!("locale.not_available")
```

### 3.9 Módulo Keyboard

Sin dependencias externas. Implementable inmediatamente con la API Ob existente.

```text
===== tr!("module.keyboard.title") =====

tr!("keyboard.current"):      tr!("keyboard.layout.sp")

tr!("keyboard.available"):
  1. tr!("keyboard.layout.us")      0 — US
  2. tr!("keyboard.layout.sp")      1 — Spanish  [*]

tr!("keyboard.actions"):
  1. tr!("keyboard.change")

[Esc] tr!("neocfg.back")
```

**Implementación** (vía Ob API existente, mismo patrón que `userbin/keyb/`):

```rust
fn get_layout() -> u8 {
    let fd = sys_ob_open("\\Global\\Info\\Keyboard", ob_access::READ)?;
    let mut buf = [0u8; 1];
    sys_ob_query_info(fd, ObInfoClass::KeyboardLayout, &mut buf)?;
    sys_close(fd)?;
    buf[0]
}

fn set_layout(layout: u8) {
    let fd = sys_ob_open("\\Global\\Info\\Keyboard", ob_access::WRITE)?;
    sys_ob_set_info(fd, ObSetInfoClass::KeyboardLayout, &[layout])?;
    sys_close(fd)?;
}
```

### 3.10 Módulo About

Sin dependencias externas. Implementable inmediatamente.

```text
===== tr!("module.about.title") =====

tr!("about.neodos"):          NeoDOS v0.49.0
tr!("about.kernel"):          neodos-kernel v0.49.0
tr!("about.abi"):             v7
tr!("about.arch"):            x86_64
tr!("about.neofs"):           NE2 v2
tr!("about.libneodos"):       v7 (ABI table)
tr!("about.build"):           2026-07-11

[Esc] tr!("neocfg.back")
```

**Implementación**:

- Version → `ob_open("\Global\Info\Version")` + `ob_query_info(Version=8)` → string del kernel
- Valores fijos compilados: ABI, arch, NeoFS version
- `libneodos::export::ABI_VERSION` para la versión de libneodos

### 3.11 Nuevos tipos/structs

**En `userbin/neocfg/src/lib.rs`:**

```rust
/// Resultado de una operación de módulo.
pub type CfgResult = Result<(), CfgError>;

pub enum CfgError {
    ModuleNotAvailable,     // El módulo requiere subsistema no implementado
    PermissionDenied,
    IoError(i64),
    UserCancelled,
}

/// Contexto de UI compartido entre módulos.
pub struct MenuContext {
    pub console: &'static dyn ConsoleAbiTable,
    pub should_exit: bool,
}

/// Claves i18n del módulo (resueltas vía tr!() en tiempo de compilación).
/// NO son un enum real — las claves son strings literales,
/// pero se documentan aquí para trazabilidad.
pub mod i18n_keys {
    // Navegación
    pub const TITLE: &str = "neocfg.title";
    pub const EXIT: &str = "neocfg.exit";
    pub const BACK: &str = "neocfg.back";
    pub const SELECT_HINT: &str = "neocfg.select_hint";
    pub const ABOUT: &str = "neocfg.about";

    // System
    pub const SYS_KERNEL_VERSION: &str = "system.kernel_version";
    pub const SYS_BUILD_DATE: &str = "system.build_date";
    pub const SYS_UPTIME: &str = "system.uptime";
    pub const SYS_CPU: &str = "system.cpu";
    pub const SYS_CORES: &str = "system.cores";
    pub const SYS_MEM_TOTAL: &str = "system.memory_total";
    pub const SYS_MEM_USED: &str = "system.memory_used";
    pub const SYS_MEM_FREE: &str = "system.memory_free";
    pub const SYS_DRIVES: &str = "system.drives";
    pub const SYS_PROCESSES: &str = "system.processes";
    pub const SYS_SERVICES: &str = "system.services";

    // Power
    pub const PWR_ACTIVE_PLAN: &str = "power.active_plan";
    pub const PWR_STATE: &str = "power.state";
    pub const PWR_SET_PLAN: &str = "power.set_plan";
    pub const PWR_RESTORE: &str = "power.restore_defaults";
    pub const PWR_SHUTDOWN: &str = "power.shutdown";
    pub const PWR_REBOOT: &str = "power.reboot";
    pub const PWR_NOT_AVAILABLE: &str = "power.not_available";

    // Locale
    pub const LOC_CURRENT: &str = "locale.current";
    pub const LOC_AVAILABLE: &str = "locale.available";
    pub const LOC_CHANGE: &str = "locale.change";
    pub const LOC_DEFAULT: &str = "locale.set_default";
    pub const LOC_NOT_AVAILABLE: &str = "locale.not_available";

    // Keyboard
    pub const KBD_CURRENT: &str = "keyboard.current";
    pub const KBD_AVAILABLE: &str = "keyboard.available";
    pub const KBD_CHANGE: &str = "keyboard.change";

    // About
    pub const ABOUT_NEODOS: &str = "about.neodos";
    pub const ABOUT_KERNEL: &str = "about.kernel";
    pub const ABOUT_ABI: &str = "about.abi";
    pub const ABOUT_ARCH: &str = "about.arch";
    pub const ABOUT_NEOFS: &str = "about.neofs";
    pub const ABOUT_LIBNEODOS: &str = "about.libneodos";
    pub const ABOUT_BUILD: &str = "about.build";
}
```

### 3.12 Dependencia de subsistemas no implementados

| Módulo | Dependencia | Estado en roadmap | Acción en v0.1 |
| --- | --- | --- | --- |
| System | Ninguna | ✅ Listo | Implementar completo |
| Keyboard | Ninguna | ✅ Listo | Implementar completo |
| About | Ninguna | ✅ Listo | Implementar completo |
| Power | Power Manager (ObType=21, info classes 32–34, 37–42) | PM-PHASE1: **HIGH**, PM-PHASE2: MEDIUM (v0.51) | Mostrar `power.not_available` + stub |
| Locale | i18n runtime (libneodos/src/i18n.rs, NLT format) | i18n-design.md: Fase 1 v0.54 | Mostrar `locale.not_available` + stub |

Los stubs no son simples placeholders: contienen la lógica de navegación, el menú, y el texto informativo. Cuando los subsistemas se implementen, solo se reemplaza el cuerpo de la función `run()`.

### 3.13 No se requieren nuevos ObType, syscalls ni info classes

NeoCfg no extiende el kernel. Es una aplicación Ring 3 que **consume** APIs existentes:

| Operación | API | RAX o clase |
| --- | --- | --- |
| Abrir información del sistema | `ob_open(\Global\Info\*)` + `ob_query_info` | RAX 60, 62 |
| Leer versión | `ob_query_info(Version=8)` | Clase 8 |
| Leer memoria | `ob_query_info(Memory=10)` | Clase 10 |
| Leer CPU | `ob_query_info(CpuInfo=7)` | Clase 7 |
| Leer unidades | `ob_query_info(Drives=11)` | Clase 11 |
| Enumerar procesos | `ob_enum(\Ob\Process)` | RAX 64 |
| Enumerar servicios | `ob_enum(\Service)` + `ob_query_info(ServiceStatus=31)` | RAX 64, clase 31 |
| Cambiar teclado | `ob_set_info(KeyboardLayout=5)` | Clase 5 |
| Leer teclado | `ob_query_info(KeyboardLayout=14)` | Clase 14 |
| Abrir Power Manager (futuro) | `ob_open(\Device\PowerManager)` | RAX 60 |
| Consultar plan (futuro) | `ob_query_info(PowerPlanInfo=32)` | Clase 32 |
| Cambiar plan (futuro) | `ob_set_info(PowerSetPlan=41)` | Clase 41 |
| Apagar (futuro) | `ob_set_info(PowerShutdown=37)` | Clase 37 |
| Reiniciar (futuro) | `ob_set_info(PowerReboot=38)` | Clase 38 |
| Leer Registry para locale (futuro) | `cm_open_key` + `cm_query_value` | RAX 67, 69 |
| Escribir Registry para locale (futuro) | `cm_set_value` | RAX 70 |

### 3.14 Archivos nuevos

| Ruta | Propósito | Líneas estimadas |
| --- | --- | --- |
| `userbin/neocfg/Cargo.toml` | Dependencia de libneodos, target config | 15 |
| `userbin/neocfg/src/main.rs` | Entry point, inicialización, bucle principal | 80 |
| `userbin/neocfg/src/lib.rs` | `CfgModule` trait, `CfgResult`, `MenuContext`, claves i18n | 60 |
| `userbin/neocfg/src/ui/menu.rs` | Renderizado de menú (título, opciones, selección) | 120 |
| `userbin/neocfg/src/ui/dialog.rs` | Diálogos: input, confirmación, mensaje informativo | 100 |
| `userbin/neocfg/src/modules/mod.rs` | Registry de módulos (`static MODULES`) | 30 |
| `userbin/neocfg/src/modules/system.rs` | Módulo System (información del sistema) | 100 |
| `userbin/neocfg/src/modules/power.rs` | Módulo Power (planes de energía, stub + ready) | 80 |
| `userbin/neocfg/src/modules/locale.rs` | Módulo Locale (idioma, stub) | 60 |
| `userbin/neocfg/src/modules/keyboard.rs` | Módulo Keyboard (distribución de teclado) | 80 |
| `userbin/neocfg/src/modules/about.rs` | Módulo About (versiones) | 60 |
| `userbin/neocfg/user.ld` | Linker script para .NXE | 10 |
| `userbin/neocfg/.cargo/config.toml` | Target config | 5 |
| `userbin/neocfg/rust-toolchain.toml` | Toolchain config | 3 |
| `userbin/neocfg/i18n/en-US/neocfg.nlt` | Traducciones inglés (generado) | 50+ entradas |
| `userbin/neocfg/i18n/es-ES/neocfg.nlt` | Traducciones español (generado) | 50+ entradas |

**Total estimado: ~700 líneas de Rust + archivos de configuración + traducciones.**

### 3.15 Cambios a archivos existentes

| Archivo | Cambio |
| --- | --- |
| `scripts/build.sh` | Añadir `neocfg` al loop de build de user binaries (línea 82) |
| `scripts/create_ne2_image.py` | Añadir `'neocfg'` a la lista de binarios (línea 226) |
| `roadmap/improvements.md` | Mover ADM-5 (`neocfg`) a `completed` cuando se implemente |
| `docs/userland/shell.md` | Añadir neocfg a la tabla de binarios (sección userbin/.NXE) |
| `docs/design/neocfg-design.md` (nuevo) | Este documento o un resumen |

---

## 4. Alternatives

### Alternative A: Extender NeoShell con subcomandos de configuración

**Descripción**: En lugar de crear un binario separado, añadir subcomandos a NeoShell: `CONFIG SYSTEM`, `CONFIG POWER`, `CONFIG KEYBOARD`.

**Rechazada porque**:

1. **Acoplamiento**: NeoShell se convertiría en un monolito de configuración, violando SRP. Cada nuevo módulo requeriría modificar NeoShell.
2. **Sin menús**: NeoShell es un REPL, no ofrece menús guiados. Habría que implementar un sistema de UI dentro de NeoShell.
3. **Reutilización GUI**: La futura interfaz gráfica no podría reutilizar la lógica incrustada en NeoShell.
4. **Complejidad de mantenimiento**: NeoShell ya tiene ~15 módulos internos (completion, pipeline, redir, env, etc.). Añadir configuración multiplica la complejidad.
5. **No hay precedente en NT**: Windows tiene `control.exe` separado de `cmd.exe`.

### Alternative B: Herramientas independientes por subsistema (ej. `powercfg.nxe`, `langcfg.nxe`, `kbdcfg.nxe`)

**Descripción**: Crear un binario .NXE separado para cada área de configuración, como ya se hace con `keyb.nxe`, `ipconfig.nxe`, `ndreg.nxe`.

**Rechazada porque**:

1. **Fragmentación de la UX**: El usuario debe recordar múltiples comandos. No hay un punto de entrada único.
2. **Duplicación de UI**: Cada herramienta implementa su propia interfaz de usuario. NeoCfg centraliza la UI en `ui/menu.rs` y `ui/dialog.rs`, reutilizables.
3. **Dificultad de descubrimiento**: Un usuario nuevo no sabe qué herramientas existen ni qué hace cada una. NeoCfg ofrece un listado completo.
4. **Alineación con la visión**: El roadmap (`ADM-5+6: neocfg + neofs`) ya prevé una herramienta de administración unificada.
5. **No excluye herramientas independientes**: `keyb.nxe`, `ipconfig.nxe` pueden coexistir con NeoCfg, que las invoca o emula su funcionalidad.

### Alternative C: Aplicación GUI desde el inicio

**Descripción**: Saltar la fase de consola e implementar NeoCfg directamente como una aplicación gráfica con framebuffer/modo VESA.

**Rechazada porque**:

1. **NeoDOS no tiene subsistema gráfico**. No hay ventanas, widgets, ni event loop gráfico. Implementar GUI desde cero para NeoCfg sería desproporcionado.
2. **La filosofía de NeoDOS prioriza interfaz de consola en la serie 0.x** (establecido en `ARCHITECTURAL_VISION.md`).
3. **La capa de presentación es reemplazable**: la arquitectura propuesta separa UI de lógica. La GUI futura reutilizará los módulos de `modules/` y solo reemplazará `ui/`.
4. **Time-to-market**: una versión de consola se implementa en días; una GUI requeriría meses de infraestructura previa.

---

## 5. Affected Components

| Subsistema | Impacto | Detalles |
| --- | --- | --- |
| **userbin/neocfg/** | **NUEVO** — Crear proyecto .NXE | 10+ archivos nuevos, ~700 líneas |
| **scripts/build.sh** | Bajo | Añadir `neocfg` al loop de build |
| **scripts/create_ne2_image.py** | Bajo | Añadir `'neocfg'` a la lista de binarios |
| **libneodos** | Ninguno | NeoCfg consume APIs existentes. No requiere cambios. |
| **Kernel** | Ninguno | NeoCfg no extiende el kernel. No requiere cambios. |
| **Object Manager** | Ninguno | NeoCfg usa Ob API via libneodos. Sin nuevos ObTypes. |
| **Registry (Cm)** | Ninguno | NeoCfg no accede al Registry directamente. Los módulos que lo necesiten (Locale) usarán `cm_set_value` vía libneodos. |
| **Power Manager** | **Dependencia futura** | El módulo Power requiere Power Manager implementado (PM-PHASE1+2). Hasta entonces, stub. |
| **i18n runtime** | **Dependencia futura** | El módulo Locale requiere i18n runtime. Hasta entonces, stub. |
| **Keyboard subsystem** | Ninguno | Ya implementado. NeoCfg lo consume. |
| **NeoShell** | Ninguno | NeoCfg es un .NXE independiente, invocable como `NEOCFG` desde NeoShell. Coexiste. |
| **Documentación** | Bajo | Actualizar `docs/userland/shell.md`. Nuevo `docs/design/neocfg-design.md` (este documento). |

---

## 6. API Contract

NeoCfg no define nuevas APIs kernel. Consume APIs existentes de libneodos. A continuación, el contrato de cada operación que NeoCfg realiza, con los wrappers que expondría (algunos ya existen, otros se añadirían a libneodos como helpers).

### 6.1 Helpers de libneodos (a añadir en versiones futuras)

```rust
/// Obtiene la versión del kernel como string.
/// fd interno: ob_open("\Global\Info\Version") + ob_query_info(Version)
pub fn sys_version() -> Result<[u8; 64], i64>;

/// Obtiene información de memoria del sistema.
/// fd interno: ob_open("\Global\Info\Memory") + ob_query_info(Memory)
pub fn sys_mem_info() -> Result<MemInfo, i64>;

/// Obtiene información completa de CPU.
/// fd interno: ob_open("\Global\Info\CpuInfo") + ob_query_info(CpuInfo)
pub fn sys_cpu_info() -> Result<CpuInfoFull, i64>;

/// Obtiene la lista de unidades montadas.
/// fd interno: ob_open("\Global\Info\Drives") + ob_query_info(Drives)
pub fn sys_drive_list() -> Result<[DriveInfo; 26], i64>;

/// Obtiene la fecha/hora del sistema.
/// fd interno: ob_open("\Global\Info\DateTime") + ob_query_info(DateTime)
pub fn sys_date_time() -> Result<DateTime, i64>;

/// Obtiene información de un servicio por fd de Ob service object.
pub fn sys_service_status(fd: u8) -> Result<ServiceStatus, i64>;
```

### 6.2 Funciones internas de NeoCfg (no públicas)

```rust
// === Módulo System ===
fn show_system_info(ui: &mut MenuContext) -> CfgResult;

// === Módulo Keyboard ===
fn get_current_layout() -> Result<u8, i64>;
fn set_layout(layout: u8) -> Result<(), i64>;
fn show_keyboard_menu(ui: &mut MenuContext) -> CfgResult;

// === Módulo About ===
fn show_about(ui: &mut MenuContext) -> CfgResult;

// === Módulo Power (stub + ready) ===
fn show_power_menu(ui: &mut MenuContext) -> CfgResult;

// === Módulo Locale (stub) ===
fn show_locale_menu(ui: &mut MenuContext) -> CfgResult;
```

### 6.3 Wrappers de Power Manager (futuro, en libneodos)

```rust
/// Abre el Power Manager y cachea el fd.
/// Llamada una vez al entrar al módulo Power.
pub fn power_open() -> Result<u8, i64>;

/// Obtiene el plan activo.
pub fn power_get_active_plan(fd: u8) -> Result<PowerPlanInfo, i64>;

/// Cambia el plan activo (0=Balanced, 1=Performance, 2=PowerSaver).
pub fn power_set_active_plan(fd: u8, plan: u32) -> Result<(), i64>;

/// Apagado coordinado del sistema. No retorna.
pub fn power_shutdown(fd: u8) -> !;

/// Reinicio coordinado del sistema. No retorna.
pub fn power_reboot(fd: u8) -> !;
```

### 6.4 Wrappers de i18n (futuro, en libneodos)

Tal como se especifica en `docs/design/i18n-design.md`:

```rust
pub fn i18n_init() -> Result<(), i64>;
pub fn i18n_load(app_name: &str) -> Result<(), i64>;
pub fn i18n_get(key: &str) -> &str;
pub fn i18n_active_locale() -> &str;
pub fn i18n_available_locales() -> Result<Vec<&str>, i64>;
pub fn i18n_reload_all();
```

---

## 7. Test Plan

### 7.1 Tests de navegación (invariante: el menú principal muestra todos los módulos registrados)

| # | Test | Expected |
| --- | --- | --- |
| 1 | NeoCfg inicia y muestra menú principal con módulos System, Power, Locale, Keyboard, About | 5 opciones numeradas visibles |
| 2 | Pulsar `Q` o `Esc` en menú principal → NeoCfg termina con código 0 | `sys_exit(0)` |
| 3 | Pulsar `1` (System) → se ejecuta módulo System → `Esc` vuelve al menú principal | Navegación correcta |

### 7.2 Tests del módulo System (invariante: la información del sistema es correcta y no modifica estado)

| # | Test | Expected |
| --- | --- | --- |
| 4 | System muestra versión del kernel | Coincide con `ob_query_info(Version)` |
| 5 | System muestra uso de memoria | `used_kib + free_kib <= total_kib` (coherencia) |
| 6 | System muestra unidades montadas | Lista al menos `C:` presente |
| 7 | System no modifica ningún estado del sistema (solo lectura) | No se invoca `ob_set_info` en todo el módulo |

### 7.3 Tests del módulo Keyboard (invariante: cambiar layout cambia efectivamente el layout activo)

| # | Test | Expected |
| --- | --- | --- |
| 8 | Keyboard muestra layout actual | Coincide con `ob_query_info(KeyboardLayout)` |
| 9 | Seleccionar "US" → layout cambia a US | `ob_query_info(KeyboardLayout)` retorna 0 |
| 10 | Seleccionar "Spanish" → layout cambia a Spanish | `ob_query_info(KeyboardLayout)` retorna 1 |
| 11 | Keyboard no modifica nada excepto `ob_set_info(KeyboardLayout)` | Solo se invoca la clase 5 |

### 7.4 Tests del módulo About (invariante: los datos de versión son consistentes)

| # | Test | Expected |
| --- | --- | --- |
| 12 | About muestra versión de NeoDOS | Contiene "v0." + números |
| 13 | About muestra arquitectura "x86_64" | Campo arch == "x86_64" |
| 14 | About muestra versión de NeoFS "NE2 v2" | Campo neofs contiene "NE2" |

### 7.5 Tests de módulos stub (invariante: los stubs muestran mensaje y no crashean)

| # | Test | Expected |
| --- | --- | --- |
| 15 | Power muestra mensaje "not available" cuando Power Manager no existe | El stub no crashea, muestra texto informativo |
| 16 | Locale muestra mensaje "not available" cuando i18n runtime no existe | El stub no crashea, muestra texto informativo |
| 17 | `Esc` desde stub → vuelve al menú principal | Navegación correcta |

### 7.6 Tests de internacionalización (invariante: todos los textos visibles pasan por `tr!()`)

| # | Test | Expected |
| --- | --- | --- |
| 18 | `tr!("neocfg.title")` devuelve string no vacío | `&str` con contenido |
| 19 | `tr!("nonexistent.key")` devuelve `"nonexistent.key"` (fallback seguro) | Sin panic, sin crash |
| 20 | Enumerar todas las claves i18n de neocfg → cada una tiene entrada en `en-US/neocfg.nlt` | No hay claves huérfanas |

### 7.7 Tests de integración (invariante: el binario se construye y ejecuta correctamente)

| # | Test | Expected |
| --- | --- | --- |
| 21 | `cargo build --release` en userbin/neocfg/ → `neocfg.nxe` generado | Exit code 0, archivo existe |
| 22 | El binario se incluye en la imagen de disco vía `create_ne2_image.py` | `'neocfg'` en la lista |
| 23 | `NEOCFG` desde NeoShell → lanza NeoCfg | Proceso se ejecuta sin error |
| 24 | NeoCfg usa solo libneodos (verificar imports) | No `core::` raw syscalls, solo `libneodos::*` |

---

## 8. Implementation Plan

### Step 1: Project scaffolding (0.5 day)

**Archivos:** todos en `userbin/neocfg/`

1. Crear estructura de directorios:

```text
userbin/neocfg/
├── Cargo.toml
   ├── rust-toolchain.toml
   ├── user.ld
   ├── .cargo/config.toml
   ├── src/
   │   ├── main.rs
   │   ├── lib.rs
   │   ├── ui/
   │   │   ├── mod.rs
   │   │   ├── menu.rs
   │   │   └── dialog.rs
   │   └── modules/
   │       ├── mod.rs
   │       ├── system.rs
   │       ├── power.rs
   │       ├── locale.rs
   │       ├── keyboard.rs
   │       └── about.rs
   └── i18n/
       ├── en-US/
       └── es-ES/
   ```

1. `Cargo.toml`: `libneodos = { path = "../../libneodos" }`
1. `main.rs`: entry point con `_start()`, inicialización, bucle de menú principal
1. `lib.rs`: `CfgModule` trait, `MenuContext`, tipos compartidos

### Step 2: UI framework (0.5 day)

**Archivos:** `src/ui/menu.rs`, `src/ui/dialog.rs`, `src/ui/mod.rs`

1. `menu.rs`: función `render_menu(title: &str, options: &[MenuOption]) -> Option<usize>`
   - Imprimir título centrado, línea separadora
   - Opciones numeradas con nombres traducidos
   - Capturar tecla vía `console::read_byte()`
   - Manejar `1`–`9`, `A`, `Q`, `Esc`
   - Retornar índice seleccionado o `None` (salir)
2. `dialog.rs`:
   - `show_message(title, lines)`: muestra texto informativo, espera tecla
   - `show_confirm(title, prompt) -> bool`: confirmación Sí/No
   - `show_input(title, prompt, buf)`: entrada de texto (usar `console::readline_buf`)
   - `show_error(title, message)`: mensaje de error + tecla para continuar

### Step 3: Module registry (0.25 day)

**Archivos:** `src/modules/mod.rs`

1. Definir `pub const MODULES: &[&dyn CfgModule] = &[...]` con los 5 módulos
2. Cada módulo es un `struct` vacío que implementa `CfgModule`
3. Los stubs (Power, Locale) implementan `run()` mostrando mensaje informativo

### Step 4: Module About (0.25 day)

**Archivos:** `src/modules/about.rs`

1. `ob_open("\Global\Info\Version")` → `ob_query_info(Version)` → print version string
2. Valores fijos compilados: `env!("CARGO_PKG_VERSION")` para libneodos, constantes para ABI v7, arch x86_64, NE2 v2
3. Esperar tecla vía `console::read_byte()`, retornar

### Step 5: Module System (0.5 day)

**Archivos:** `src/modules/system.rs`

1. `ob_open("\Global\Info\Version")` → `ob_query_info(Version)` → print
2. `ob_open("\Global\Info\Memory")` → `ob_query_info(Memory)` → parse `MemInfo` → print
3. `ob_open("\Global\Info\CpuInfo")` → `ob_query_info(CpuInfo)` → parse `CpuInfoFull` → print
4. `ob_open("\Global\Info\DateTime")` → `ob_query_info(DateTime)` → print date
5. `ob_open("\Global\Info\Drives")` → `ob_query_info(Drives)` → iterate drives → print
6. `ob_enum("\Ob\Process")` → count → print
7. `ob_enum("\Service")` → iterate → `ob_query_info(ServiceStatus)` por servicio → print count + running
8. Esperar tecla, retornar

### Step 6: Module Keyboard (0.25 day)

**Archivos:** `src/modules/keyboard.rs`

1. `ob_open("\Global\Info\Keyboard")` → `ob_query_info(KeyboardLayout)` → get current
2. Mostrar menú con layouts disponibles (US=0, Spanish=1)
3. `ob_set_info(KeyboardLayout, &[layout])` si el usuario selecciona cambio
4. Esperar tecla, retornar

### Step 7: Module Power stub (0.25 day)

**Archivos:** `src/modules/power.rs`

1. Intentar `ob_open("\Device\PowerManager")`
2. Si falla (Not Found) → mostrar `tr!("power.not_available")`, retornar
3. Si existe (futuro) → implementar menú completo con planes, shutdown, reboot
4. Esperar tecla, retornar

### Step 8: Module Locale stub (0.25 day)

**Archivos:** `src/modules/locale.rs`

1. Mostrar `tr!("locale.not_available")`
2. Esperar tecla, retornar

### Step 9: Internationalization (1 day)

**Archivos:** `i18n/en-US/neocfg.nlt`, `i18n/es-ES/neocfg.nlt`

1. Crear archivos .nlt manualmente (formato NLT: magic + count + key_offsets + val_offsets + strings)
2. ~50 claves i18n para todos los textos visibles
3. Integrar `i18n_init()` + `i18n_load("neocfg")` en `main.rs` (con fallback si no existe)
4. Reemplazar todos los `write_str(b"...")` por `write_str(tr!("clave"))`

### Step 10: Build integration (0.25 day)

**Archivos:** `scripts/build.sh`, `scripts/create_ne2_image.py`

1. Añadir `neocfg` al loop de build en `scripts/build.sh` (línea ~82)
2. Añadir `'neocfg'` a la lista de binarios en `scripts/create_ne2_image.py` (línea ~226)
3. Verificar: `cargo build` en `neodos-kernel/` → genera `userbin/neocfg/target/x86_64-unknown-none/release/neocfg`

### Step 11: Tests (0.5 day)

**Archivos:** tests embebidos en cada módulo o test manual

1. Tests unitarios de UI: navegación, renderizado (simular input buffer)
2. Tests de módulos: verificar que las llamadas a libneodos son correctas
3. Compilar y verificar: `cargo build` + `python3 scripts/auto_test.py`
4. Prueba manual en QEMU: ejecutar `NEOCFG`, navegar menús, verificar System/Keyboard/About

### Step 12: Documentation (0.5 day)

**Archivos:** `docs/design/neocfg-design.md` (este documento), `docs/userland/shell.md`, `roadmap/improvements.md`

1. Añadir entrada `neocfg` a la tabla de binarios en `docs/userland/shell.md`
2. Sincronizar con `scripts/sync-roadmap.sh sync` para marcar ADM-5 completado

### Total estimated effort: ~5 days

---

## Appendix A: i18n keys for NeoCfg

```text
# Navigation
neocfg.title            = "NeoCfg — Control Panel"
neocfg.exit             = "Exit"
neocfg.back             = "Back"
neocfg.select_hint      = "Select an option"
neocfg.about            = "About NeoCfg"

# Module names
module.system.name      = "System"
module.system.desc      = "View system information"
module.power.name       = "Power"
module.power.desc       = "Manage power plans"
module.locale.name      = "Locale"
module.locale.desc      = "Configure system language"
module.keyboard.name    = "Keyboard"
module.keyboard.desc    = "Change keyboard layout"
module.about.name       = "About"
module.about.desc       = "System version information"

# System module
system.title            = "===== System Information ====="
system.kernel_version   = "Kernel version"
system.build_date       = "Build date"
system.uptime           = "Uptime"
system.cpu              = "CPU"
system.cores            = "CPU cores"
system.memory_total     = "Total memory"
system.memory_used      = "Used memory"
system.memory_free      = "Free memory"
system.drives           = "Drives"
system.processes        = "Processes"
system.services         = "Services"

# Power module
power.title             = "===== Power Management ====="
power.active_plan       = "Active plan"
power.state             = "System state"
power.plan.balanced     = "Balanced"
power.plan.performance  = "High Performance"
power.plan.powersaver   = "Power Saver"
power.state.active      = "Active"
power.set_plan          = "Change power plan"
power.restore_defaults  = "Restore defaults"
power.shutdown          = "Shut down"
power.reboot            = "Reboot"
power.not_available     = "Power Manager is not available on this system"

# Locale module
locale.title            = "===== System Language ====="
locale.current          = "Current language"
locale.available        = "Available languages"
locale.change           = "Change language"
locale.set_default      = "Set as default"
locale.not_available    = "Internationalization is not available on this system"

# Keyboard module
keyboard.title          = "===== Keyboard Layout ====="
keyboard.current        = "Current layout"
keyboard.available      = "Available layouts"
keyboard.change         = "Change layout"
keyboard.layout.us      = "US"
keyboard.layout.sp      = "Spanish"

# About module
about.title             = "===== About NeoDOS ====="
about.neodos            = "NeoDOS version"
about.kernel            = "Kernel version"
about.abi               = "ABI version"
about.arch              = "Architecture"
about.neofs             = "NeoFS version"
about.libneodos         = "libneodos version"
about.build             = "Build date"
```

---

*Este documento constituye la especificación de diseño de NeoCfg v0.1.*
*No se implementará código hasta la aprobación del ARB.*
*Las dependencias externas (Power Manager, i18n runtime) se trackean en `roadmap/improvements.md`.*
