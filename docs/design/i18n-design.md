# NeoDOS Internationalization (i18n) — Design Document

> **Status:** Draft v1 — Pre-implementation design review
> **Target Kernel:** v0.54+ (post user commands)
> **Filosofía:** Ligero, rápido, sin dependencias externas, preparado para GUI.
> **Principio:** El kernel NO traduce mensajes. Las aplicaciones traducen vía libneodos.

---

## 1. Auditoría — Estado Actual

### 1.1 ¿Qué existe hoy?

**Nada.** El código actual contiene:

| Componente | Strings | Mé todo de salida |
|------------|---------|-------------------|
| NeoShell | ~21 user-facing | `write_str(b"literal")` → `sys_write` |
| NeoInit | ~17 | `write_str(b"literal")` → `sys_write` |
| corehelp | ~16 + help blocks | `write_str(b"literal")` |
| Otras 30+ bins | ~50 total | `write_str(b"literal")` |
| libneodos | 0 (solo errno numérico) | N/A — el kernel no imprime |

**Total:** ~100 cadenas únicas en ~30 binarios. Cero infraestructura i18n.

### 1.2 Mecanismo de carga de NXLs (relevante para i18n)

```
0x1e00_0000  Slot 0: libneodos.nxl (auto-load, ABI v7)
0x1e04_0000  Slot 1: math.nxl
0x1e08_0000  Slot 2: console.nxl (lazy-load via AtomicU64)
0x1e0c_0000  Slot 3: net.nxl
0x1e10_0000  Slots 4-7: LIBRES ← para locale.nxl
0x1e1f_ffff
```

Patrón de lazy-load existente (desde `console.rs`):
```rust
static BASE: AtomicU64 = AtomicU64::new(0);
fn get_table() -> Option<&'static Table> {
    let base = BASE.load(Ordering::Relaxed);
    if base != 0 { return Some(unsafe { &*... }); }
    match loadlib("C:\\System\\Libraries\\console.nxl") {
        Ok(base) => { BASE.store(base, Ordering::Relaxed); ... }
        Err(_) => None,
    }
}
```

### 1.3 Registry — dónde vive la configuración de locale

Actualmente existe `\Registry\Machine\System\CurrentControlSet\Control` con `WaitForNetwork`.
No existe entrada de locale. La ruta propuesta sigue el estándar NT:

```
\Registry\Machine\System\CurrentControlSet\Control\Locale
  Language        REG_SZ  "en"
  Locale          REG_SZ  "en_US.UTF-8"
  KeyboardLayout  REG_SZ  "us"
```

### 1.4 Cómo se imprimen los mensajes hoy

1. `write_str(b"literal")` en la aplicación
2. → `libneodos::syscall::sys_write(fd, ptr, len)`
3. → `(*get_table().sys_write)(fd, ptr, len)` — salta al NXL slot 0
4. → `libneodos-nxl::io::nxl_sys_write()`
5. → `asm!("int 0x80")` — llama al kernel

No hay formateo, no hay placeholders, no hay tabla de traducción.

---

## 2. Formato de Recursos

### 2.1 Formato NLT (Neodos Language Table)

Formato propio, binario, diseñado para carga rápida y bajo consumo:

```
┌──────────────────────────────────────────────┐
│ Magic: "NLT\0" (4 bytes)                     │
│ Version: u32 LE (= 1)                        │
│ Count: u32 LE (número de entradas)           │
│ ┌─────────── Tabla de claves ───────────┐   │
│ │ key_off[0]: u32 LE                     │   │
│ │ key_off[1]: u32 LE                     │   │
│ │ ...                                    │   │
│ │ key_off[N-1]: u32 LE                   │   │
│ ├─────────── Tabla de valores ──────────┤   │
│ │ val_off[0]: u32 LE                     │   │
│ │ val_off[1]: u32 LE                     │   │
│ │ ...                                    │   │
│ │ val_off[N-1]: u32 LE                   │   │
│ ├─────────── Strings ───────────────────┤   │
│ │ key[0]: bytes...\0                     │   │
│ │ key[1]: bytes...\0                     │   │
│ │ ...                                    │   │
│ │ val[0]: bytes...\0                     │   │
│ │ val[1]: bytes...\0                     │   │
│ │ ...                                    │   │
└──────────────────────────────────────────────┘
```

**Campos:**
- `Magic`: 4 bytes `NLT\0` (0x00544C4E LE)
- `Version`: 1 (u32 LE). Permite evolución del formato.
- `Count`: número de entradas (u32 LE). Máximo 65535.
- `key_off[i]`: offset desde el inicio del archivo hasta el string de la clave i (null-terminated)
- `val_off[i]`: offset desde el inicio del archivo hasta el string del valor i (null-terminated)
- `key[i]`: string UTF-8 null-terminated (ej. `"file.notfound"`)
- `val[i]`: string UTF-8 null-terminado (ej. `"File not found"`)

**Búsqueda O(n)** — lineal sobre `Count` entradas. Para ~100-500 claves por aplicación, una búsqueda lineal sobre un array contiguo de offsets es más rápida que un hash (sin colisiones, sin cálculo de hash, amigable con caché L1).

### 2.2 Justificación del formato

| Decisión | Alternativa | Por qué NLT |
|----------|------------|-------------|
| Sin hash | HashMap requiere ~2KB+ de overhead + cálculo de hash | O(n) con n<500 es ~10-50μs; hash añade complejidad sin beneficio real |
| Sin JSON/XML | Legible pero parseo costoso y verboso | NLT se mapea directo a memoria (`mmap` o `read` plano), sin parseo |
| Sin gettext .mo | .mo es específico de GNU, requiere toolchain externa | NLT es autónomo, el formato cabe en 50 líneas de Rust |
| Offsets, no punteros | Cargar en cualquier dirección de memoria | Los offsets son relativos al inicio del archivo; se pueden cargar en cualquier slot |
| Claves en inglés | `"file.notfound"` como clave única, no el string original | Evita ambigüedad, permite cambiar el inglés también |

**Tamaño estimado por aplicación:**
- 100 claves × (8 bytes de overhead + 20 bytes de clave + 40 bytes de valor) ≈ 7 KB
- Total para ~30 aplicaciones ≈ 210 KB comprimido en disco
- La aplicación solo carga su propio archivo, no el de otras

### 2.3 Convención de nombres de clave

Tesitura jerárquica con puntos:

```
file.notfound       → "File not found"
file.permission     → "Permission denied"
cmd.help.header     → "Available commands:"
cmd.help.footer     → "Type HELP <command> for details"
prompt.suffix       → "> "
error.invalid_drive → "Invalid drive"
error.bad_command   → "Bad command or file name"
error.pipe          → "Pipe error"
status.running      → "running..."
status.done         → "Done"
```

---

## 3. Organización de Archivos en Disco

```
C:\System\Locale\
  ├── en-US\
  │   ├── neoshell.nlt
  │   ├── neoinit.nlt
  │   ├── corehelp.nlt
  │   ├── coredir.nlt
  │   ├── corecopy.nlt
  │   ├── ... (uno por aplicación con strings visibles)
  │   └── locale.nlt        ← traducciones del propio runtime i18n
  ├── es-ES\
  │   ├── neoshell.nlt
  │   ├── neoinit.nlt
  │   └── ...
  ├── ca-ES\
  │   └── ...
  └── default.nlt           ← symlink o fallback built-in
```

### 3.1 Justificación

| Decisión | Por qué |
|----------|---------|
| Un archivo por aplicación | Una aplicación no necesita cargar traducciones de otras. Menos I/O, menos memoria. |
| Un directorio por idioma | Fácil de añadir/quitar idiomas. `ls C:\System\Locale\` lista idiomas disponibles. |
| Archivo por aplicación, no por idioma | Cada `.nlt` es pequeño (~7 KB). Carga completa en RAM. Sin lecturas aleatorias. |
| Sin archivo monolítico | Un solo archivo de 6 MB para todos los idiomas + aplicaciones sería frágil y lento. |

### 3.2 Carga en tiempo de ejecución

1. NeoInit lee `Language = "es"` del Registry
2. La aplicación llama `i18n_load("neoshell")`
3. libneodos construye la ruta: `C:\System\Locale\es-ES\neoshell.nlt`
4. Si no existe, fallback: `C:\System\Locale\es\neoshell.nlt`
5. Si no existe, fallback: `C:\System\Locale\en-US\neoshell.nlt`
6. Si no existe, fallback: todos los `tr!("key")` devuelven la clave literal

---

## 4. API Pública

### 4.1 Core API (libneodos)

```rust
/// Inicializa el subsistema i18n para la aplicación actual.
/// Lee el locale activo del Registry y prepara el loader.
/// Se llama una vez al inicio de la aplicación.
/// Returns: 0 on success, -errno on failure.
pub fn i18n_init() -> Result<(), i64>;

/// Carga el archivo de traducciones para una aplicación.
/// app_name: nombre corto (ej. "neoshell", "neoinit").
/// Busca en C:\System\Locale\{lang}\{app_name}.nlt
/// con cadena de fallback (ver sección 6).
/// Returns: 0 on success, -errno on failure.
pub fn i18n_load(app_name: &str) -> Result<(), i64>;

/// Obtiene la traducción de una clave.
/// Si no encuentra la clave, devuelve la propia clave (nunca panic).
/// El resultado es un &str que vive mientras el .nlt esté cargado.
pub fn i18n_get(key: &str) -> &str;

/// Libera los recursos de traducción de una aplicación.
/// Se llama automáticamente al salir si se registra el cleanup.
pub fn i18n_unload(app_name: &str);

/// Recarga todas las traducciones (útil tras cambiar idioma en caliente).
pub fn i18n_reload_all();

/// Consulta el locale activo actual.
pub fn i18n_active_locale() -> &str;   // ej. "es-ES"

/// Lista los idiomas disponibles en el sistema.
pub fn i18n_available_locales() -> Result<Vec<&str>, i64>;
```

### 4.2 Macro `tr!`

```rust
/// Macro de traducción.
/// Uso: tr!("file.notfound")
/// Expande a: i18n_get("file.notfound")
/// Si i18n no está inicializado, devuelve el string literal.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        if let Some(s) = $crate::i18n::try_get($key) {
            s
        } else {
            $key  // fallback: la propia clave es legible
        }
    };
}
```

### 4.3 Ejemplo de uso en NeoShell

```rust
// Antes:
write_str(b"Bad command or file name\r\n");

// Después:
use libneodos::tr;
write_str(tr!("error.bad_command"));
write_str(b"\r\n");
```

### 4.4 Ejemplo de uso con formato

```rust
// Para mensajes con parámetros (ej. "File X not found"):
let msg = libneodos::i18n::format_str(
    tr!("file.notfound.with_name"),  // clave: "File '{0}' not found"
    &[filename],                      // parámetros
);
write_str(msg.as_bytes());
```

Nota: `format_str` es una función auxiliar que reemplaza `{0}`, `{1}`, etc. en el string traducido. Es lazy (solo formatea cuando se necesita) y usa un buffer de stack de 256 bytes.

---

## 5. Selección de Idioma

### 5.1 Fuente de verdad: Registry

```
\Registry\Machine\System\CurrentControlSet\Control\Locale
  Language  REG_SZ  "es"
  Locale    REG_SZ  "es_ES.UTF-8"
```

### 5.2 Flujo de selección

```
i18n_init()
  → cm_open_key("\Registry\Machine\System\CurrentControlSet\Control\Locale")
  → cm_query_value("Locale") o ("Language")
  → Si no existe: hardcode "en-US"
  → Almacena en static LOCALE: &str (ej. "es-ES")
  → (Opcional) Lee KeyboardLayout y lo aplica
```

### 5.3 Cambio de idioma en caliente

```rust
// 1. Usuario cambia el valor en Registry (via neocfg o API)
// 2. Aplicación llama:
i18n_reload_all();
// 3. Todas las traducciones se recargan desde disco
// 4. La próxima vez que se llame tr!("clave"), se usa el nuevo idioma
// 5. Los mensajes ya mostrados no cambian (solo los futuros)
```

### 5.4 Integración con login (futuro)

Cuando exista el sistema de usuarios (USR-P2), el locale se almacenará también en `\Registry\User\{sid}\Control\Locale`, con prioridad sobre el del sistema:

1. `\Registry\User\{sid}\Control\Locale\Locale` — per-user (mayor prioridad)
2. `\Registry\Machine\System\CurrentControlSet\Control\Locale` — sistema (fallback)
3. Hardcoded `"en-US"` — fallback final

---

## 6. Sistema de Fallback

### 6.1 Cadena de resolución de archivos

```
i18n_load("neoshell")
  → LOCALE = "es-ES"
  → 1. C:\System\Locale\es-ES\neoshell.nlt  (español de España)
  → 2. C:\System\Locale\es\neoshell.nlt     ( español genérico)
  → 3. C:\System\Locale\en-US\neoshell.nlt   (inglés por defecto)
  → 4. C:\System\Locale\default.nlt          (built-in, opcional)
  → 5. No hay archivo → todas las claves devuelven el key literal
```

### 6.2 Cadena de resolución de claves

```
i18n_get("file.notfound")
  → 1. Busca en el .nlt cargado para esta app
  → 2. Busca en locale.nlt (traducciones del runtime)
  → 3. Busca en el .nlt de en-US (si está cargado como fallback)
  → 4. Devuelve "file.notfound" (el propio key)
```

**Nunca panic, nunca null pointer, nunca error fatal.**

### 6.3 Carga de fallback

Cuando `i18n_load("neoshell")` carga es-ES, también carga automáticamente `en-US/neoshell.nlt` si existe. Así las claves no traducidas en es-ES caen en en-US sin I/O adicional en caliente.

```
Carga real:
  es-ES/neoshell.nlt → tabla A (100 claves)
  en-US/neoshell.nlt → tabla B (100 claves, cargada solo si difiere)
Consulta:
  Buscar en A (O(n)). Si no encontrado, buscar en B (O(n)).
  Si no en B, devolver key literal.
```

---

## 7. Caché en Tiempo de Ejecución

### 7.1 Estrategia

| Aspecto | Decisión | Justificación |
|---------|----------|---------------|
| Cuándo se carga | Al llamar `i18n_load()` | La aplicación decide cuándo. Típicamente al inicio. |
| Dónde se almacena | En una `Vec<NltTable>` estática por-app en libneodos | Un NLT cabe en ~7 KB. Múltiples tablas son <50 KB. |
| Liberación | `i18n_unload()` o al salir de la app | Se puede registrar un destructor vía `on_exit`. |
| Re-carga | `i18n_reload_all()` libera y recarga todo | Para cambio de idioma en caliente. |

### 7.2 Estructura en memoria

```rust
pub struct NltTable {
    pub locale: &'static str,     // ej. "es-ES"
    pub app_name: &'static str,   // ej. "neoshell"
    pub keys: &'static [KeyEntry], // apunta directamente a la mmap
}

pub struct KeyEntry {
    pub key: &'static str,   // apunta dentro del .nlt mmap
    pub val: &'static str,   // apunta dentro del .nlt mmap
}
```

**Sin asignaciones dinámicas por consulta.** `i18n_get()` solo hace un scan lineal sobre los slices. Los strings apuntan directamente a la memoria del archivo cargado (zero-copy).

### 7.3 Mecanismo de carga

```rust
fn load_nlt(path: &str) -> Result<NltTable, i64> {
    let fd = sys_ob_open(path, ACCESS_READ)?;
    let size = query_size(fd);
    let mut buf = vec![0u8; size];  // única alloc: el buffer completo
    sys_ob_query_info(fd, ReadContent, &mut buf)?;
    sys_close(fd);

    // Validar magic y version
    let magic = &buf[0..4];
    if magic != b"NLT\0" { return Err(-EINVAL); }
    let count = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;

    // Construir índices sin copiar strings
    // key_offsets: &[u32] apuntando dentro de buf
    // val_offsets: &[u32] apuntando dentro de buf
    // keys[i]: &str apuntando a buf[key_offsets[i]]
    // vals[i]: &str apuntando a buf[val_offsets[i]]

    Ok(NltTable {
        locale: ...,
        app_name: ...,
        entries: Vec::from_raw_parts(entries_ptr, count, count),
        _raw_buf: buf,  // mantiene viva la memoria
    })
}
```

---

## 8. Integración con NeoShell

### 8.1 Cambios en main.rs

**Antes:**
```rust
fn main() -> ! {
    write_str(b"Type HELP for a list of commands.\r\n");
    // ...
    write_str(b"Bad command or file name\r\n");
    // ...
}
```

**Después:**
```rust
fn main() -> ! {
    let _ = libneodos::i18n_init();        // lee locale del Registry
    let _ = libneodos::i18n_load("neoshell");  // carga traducciones
    // ...
    write_str(tr!("prompt.startup_hint"));
    write_str(b"\r\n");
    // ...
    write_str(tr!("error.bad_command"));
    write_str(b"\r\n");
    // ...
}
```

### 8.2 Lista de claves para neoshell

| Clave actual | Clave i18n | Propósito |
|-------------|------------|-----------|
| `b"C:\\"` (fallback CWD) | `prompt.cwd_fallback` | Default CWD en prompt |
| `b"> "` | `prompt.suffix` | Prompt suffix |
| `b"Type HELP for a list of commands.\r\n"` | `prompt.startup_hint` | Mensaje de bienvenida |
| `b"\r\nInvalid drive\r\n"` | `error.invalid_drive` | Drive inválido |
| `b"ob_wait error\r\n"` | `error.ob_wait` | Error de wait |
| `b"cd: directory not found\r\n"` | `error.cd_not_found` | CD falló |
| `b"Bad command or file name\r\n"` | `error.bad_command` | Comando no encontrado |
| `b"\r\nPipe error\r\n"` | `error.pipe` | Error de pipe |
| `b"\r\nInvalid pipe syntax\r\n"` | `error.pipe_syntax` | Sintaxis de pipe |
| `b"\r\nCannot pipe built-in\r\n"` | `error.pipe_builtin` | Built-in en pipe |
| `b"\r\nUsage: CALL batchfile\r\n"` | `error.call_usage` | CALL sin argumento |
| `b"\r\nBatch file not found\r\n"` | `error.call_not_found` | CALL archivo no encontrado |
| `b"\r\nError reading batch\r\n"` | `error.call_read` | CALL error de lectura |
| `b"\r\npowering off...\r\n"` | `status.poweroff` | Apagado |
| `b"Press any key to continue . . .\r\n"` | `prompt.pause` | Pausa batch |
| `b" [VT"` | `prompt.vt_prefix` | Indicador de VT |

### 8.3 Estrategia de migración

1. Añadir `i18n_init()` y `i18n_load("neoshell")` al inicio de `main()`
2. Reemplazar cada `write_str(b"literal")` por `write_str(tr!("clave"))`
3. Mantener los `b"\r\n"` y caracteres de control separados (no se traducen)
4. Si `i18n_load()` falla (no hay archivo .nlt), `tr!("clave")` devuelve `"clave"` — no hay mensajes visibles, pero no crash
5. En una segunda fase, crear los archivos `.nlt` para en-US y es-ES

---

## 9. Integración con Aplicaciones

### 9.1 API común para todas las aplicaciones

Cada aplicación .NXE que muestre mensajes al usuario debería:

```rust
fn main() -> ! {
    let _ = libneodos::i18n_init();
    let _ = libneodos::i18n_load(crate::APP_NAME);
    // ... resto de la lógica usando tr!("...")
}
```

Donde `APP_NAME` es una constante al inicio de cada binario:
```rust
const APP_NAME: &str = "neoinit";  // o "corehelp", "coredir", etc.
```

### 9.2 Aplicaciones objetivo (Fase 1)

| App | Strings | Priority |
|-----|---------|----------|
| NeoShell | 16 | Crítica — primer contacto del usuario |
| NeoInit | 12 | Alta — mensajes de boot |
| corehelp | 14 | Alta — help del sistema |
| coredir | 4 | Media |
| corecopy | 6 | Media |
| kill | 3 | Media |
| ps | 3 | Media |
| label | 3 | Baja |
| fsck | 4 | Baja |
| ndreg | 4 | Baja |
| **Total Fase 1** | **~72** | |

### 9.3 Aplicaciones objetivo (Fase 2)

neologon, neotop, neostat, neotask, neocfg, neofs, neolog, ping, ipconfig, dhcpd

---

## 10. Integración con el Runtime i18n

### 10.1 locale.nxl — Runtime de traducción (opcional)

Si el volumen de datos lo justifica, se puede crear un `locale.nxl` en slot 4 que centralice:
- `locale_load()`, `locale_get()`, `locale_list()` como funciones NXL
- Las aplicaciones llaman a libneodos que delega al NXL

**Decisión:** Empezar sin NXL. Todo el runtime cabe en libneodos como Rust nativo. El NXL solo se justifica si:
- El conjunto de datos crece >100 KB por aplicación
- Se necesita compartir tablas entre procesos
- La carga en caliente requiere aislamiento

### 10.2 libneodos — Módulo i18n

Nuevo archivo: `libneodos/src/i18n.rs`

```
libneodos/src/
  ├── lib.rs          + pub mod i18n;
  ├── i18n.rs         ← NUEVO: 300-400 líneas
  ├── export.rs
  ├── syscall.rs
  └── ...
```

`i18n.rs` contiene:
- `NltTable` struct (formato en memoria)
- `i18n_init()` — lectura de Registry
- `i18n_load()` — carga de archivo .nlt
- `i18n_get()` — lookup lineal
- `i18n_unload()` — liberación
- `i18n_reload_all()` — recarga completa
- `format_str()` — reemplazo de `{0}`, `{1}`
- `try_get()` — usado por `tr!()` macro
- Tests unitarios

---

## 11. Preparación para GUI

### 11.1 Misma API, diferente backend

La API `tr!("clave")` funciona igual en consola y en GUI:

```rust
// Consola:
write_str(tr!("file.notfound"));
write_str(b"\r\n");

// GUI (futuro):
label.set_text(tr!("file.notfound"));  // misma llamada
```

El runtime i18n no sabe si la salida es consola o GUI. Solo devuelve `&str`.

### 11.2 Formato de mensajes con placeholders

Para GUI se necesitarán mensajes con formato más complejo. El sistema de `{0}` placeholders en `format_str()` ya cubre:

```rust
// Clave: "delete.confirm" → "Are you sure you want to delete '{0}'?"
// GUI:
let msg = format_str(tr!("delete.confirm"), &[filename]);
dialog.show_confirm(&msg);
```

### 11.3 Bidireccionalidad (RTL)

El sistema de fallback por locale permite añadir soporte RTL en el futuro:
- `\Registry\Machine\...\Locale\Layout` = `"rtl"`
- La GUI consulta esta clave para espejar el layout
- El runtime i18n no necesita cambios (los strings RTL están en el .nlt)

---

## 12. Herramienta de Desarrollo: `neolocale`

### 12.1 Especificación

Binario .NXE que valida y gestiona archivos .nlt:

```
NEOLOCALE validate <file.nlt>        → verifica formato, duplicados
NEOLOCALE check <dir>                → busca claves faltantes entre idiomas
NEOLOCALE diff <base> <target>       → compara dos archivos .nlt
NEOLOCALE create <app> <locale>      → crea .nlt vacío desde plantilla
NEOLOCALE stats <dir>                → estadísticas de traducción
```

### 12.2 Funciones de validación

```
validate(nlt):
  ✓ Magic "NLT\0"
  ✓ Version == 1
  ✓ Count <= 65535
  ✓ Todos los offsets están dentro del archivo
  ✓ No hay claves duplicadas
  ✓ Todos los strings son UTF-8 válidos
  ✓ Claves siguen convención "dominio.subdominio"
```

---

## 13. Alternativas Consideradas

### 13.1 gettext-like .mo

| Aspecto | .mo | NLT (elegido) |
|---------|-----|---------------|
| Formato | Binario, específico de GNU | Binario propio, 50 líneas de Rust |
| Dependencia | Necesita toolchain gettext | Ninguna |
| Búsqueda | Hash table (compleja) | O(n) lineal (simple, rápida para n<500) |
| Plurales | Soporte nativo | No necesario para v1 (añadir en v2) |
| Contexto | `msgctx` | Se puede simular con clave jerárquica |
| Portabilidad | Dependiente de glibc | 100% Rust, 0 dependencies |

**Decisión:** NLT. gettext es pesado, externo, y no se alinea con la filosofía "no copiar" de NeoDOS.

### 13.2 Archivo plano TOML/INI

| Aspecto | TOML | NLT (elegido) |
|---------|------|---------------|
| Parseo | Necesita parser TOML completo | Mapeo directo a memoria |
| Tamaño | ~50% overhead frente a binario | ~5% overhead (offsets) |
| Velocidad de carga | ~500μs (parseo) | ~10μs (mmap + validación) |
| Editable | Sí, con cualquier editor | No directamente (necesita neolocale) |

**Decisión:** NLT. La velocidad de carga y el mínimo consumo de memoria son críticos para un sistema operativo. La edición se resuelve con `neolocale`.

### 13.3 Strings compilados en el binario

Cada aplicación llevaría sus traducciones compiladas dentro del .NXE:

| Aspecto | Compilado en .NXE | .nlt externo (elegido) |
|---------|-------------------|----------------------|
| Cambiar idioma | Recompilar | Editar archivo en disco |
| Tamaño .NXE | Crece ~10 KB por idioma | No crece |
| Añadir idioma | Recompilar todas las apps | Crear directorio + archivos |
| Carga | Instantánea (en .rodata) | ~10 μs de I/O |

**Decisión:** .nlt externo. La flexibilidad de añadir idiomas sin recompilar es más importante que los ~10 μs de carga. Además, mantener los .NXE pequeños es importante dado el límite de 64 KB.

---

## 14. Rendimiento

### 14.1 Métricas objetivo

| Operación | Objetivo | Estimación |
|-----------|----------|------------|
| `i18n_init()` | <100 μs | 1 lectura de Registry + parseo |
| `i18n_load("app")` | <50 μs | 1 open + 1 read de ~7 KB + validación |
| `i18n_get("clave")` | <1 μs | Scan lineal de ~100 entradas |
| `tr!("clave")` | <1 μs | Idem + expansión de macro |
| `format_str("{0}", ...)` | <5 μs | Replace en buffer de stack |
| Memoria por app cargada | ~7 KB | El archivo .nlt completo |
| Memoria total (todas las apps) | <200 KB | ~30 apps cargadas simultáneamente |

### 14.2 Optimizaciones

1. **Carga lazy:** `i18n_load()` se llama explícitamente. No hay precarga de todas las apps.
2. **Zero-copy:** Los strings apuntan directamente al buffer mmap. No hay copia ni al leer ni al buscar.
3. **Sin hashing:** Búsqueda lineal sobre array contiguo. Para 100 entradas, ~50 comparaciones de media.
4. **Buffer de stack:** `format_str()` usa un array de 256 bytes en la pila. Sin alloc para el 99% de los casos.
5. **Caché de hot path:** La última clave buscada se almacena en un `Cell<(&str, &str)>`. Si se repite (ej. prompt), la segunda búsqueda es O(1).
6. **Enlace estático del runtime:** El código i18n está en libneodos, que ya está en el NXL. No hay llamadas extra.

---

## 15. Tests

### 15.1 Tests unitarios (en `libneodos/src/i18n.rs`)

| Test | Descripción |
|------|-------------|
| `i18n_parse_nlt_valid` | Parsear .nlt válido de 3 entradas |
| `i18n_parse_nlt_magic_invalid` | Magic incorrecto → error |
| `i18n_parse_nlt_truncated` | Archivo truncado → error |
| `i18n_get_exact_match` | Clave exacta devuelve valor correcto |
| `i18n_get_case_sensitive` | Diferente mayúsc/minús no matchea |
| `i18n_get_missing_returns_key` | Clave faltante devuelve la propia clave |
| `i18n_get_empty_table` | Tabla vacía devuelve clave |
| `i18n_get_after_unload` | `unload()` + `get()` devuelve clave |
| `i18n_load_app_not_found` | Archivo inexistente → error, no panic |
| `i18n_fallback_to_enUS` | es-ES faltante → en-US cargado |
| `i18n_fallback_chain_full` | Cadena completa: es-ES → es → en-US → key |
| `i18n_format_str_simple` | `"{0}"` reemplazado correctamente |
| `i18n_format_str_multiple` | `"{0} {1} {0}"` reemplaza ambos |
| `i18n_format_str_no_placeholders` | Sin placeholders devuelve original |
| `i18n_cold_cache_hit` | Primera búsqueda va a tabla |
| `i18n_lru_cache_repeat` | Segunda búsqueda misma clave es O(1) |

### 15.2 Tests de integración (en `auto_test.py`)

| Test | Descripción |
|------|-------------|
| `i18n_neoshell_spanish` | Boot con locale=es, verificar prompt en español |
| `i18n_neoshell_english` | Boot con locale=en, verificar prompt en inglés |
| `i18n_switch_locale_runtime` | Cambiar Registry + reload, verificar nuevo idioma |
| `i18n_missing_locale_fallback` | locale=fr (sin .nlt) → funciona en inglés |

### 15.3 Tests de herramientas

| Test | Descripción |
|------|-------------|
| `neolocale_validate_valid` | Validar .nlt bien formado → OK |
| `neolocale_check_missing` | Detectar clave faltante entre es-ES y en-US |
| `neolocale_create_empty` | Crear .nlt vacío → válido |
| `neolocale_stats_counts` | Estadísticas correctas |

---

## 16. Implementación por Fases

### Fase 1 — Runtime básico (v0.54)

| Paso | Archivos | Descripción |
|------|----------|-------------|
| 1.1 | `libneodos/src/i18n.rs` (nuevo) | `NltTable`, `i18n_get()`, `i18n_load()`, `tr!()`, parseo de .nlt |
| 1.2 | `libneodos/src/lib.rs` | `pub mod i18n;` |
| 1.3 | `libneodos/src/syscall.rs` | Añadir `sys_cm_query_value` si no existe wrapper para leer Registry |
| 1.4 | `libneodos/src/i18n.rs` | `i18n_init()`: leer `\Registry\Machine\...\Control\Locale\Locale` |
| 1.5 | `neodos-kernel/src/cm/mod.rs` | Añadir valor por defecto `Locale` = `"en-US"` en `cm_ensure_default_values()` |

### Fase 2 — Archivos .nlt + migración de NeoShell (v0.54)

| Paso | Archivos | Descripción |
|------|----------|-------------|
| 2.1 | `scripts/create_ne2_image.py` | Añadir `C:\System\Locale\en-US\*.nlt` al disco |
| 2.2 | `scripts/build.sh` | Generar .nlt desde plantillas (o crearlos manualmente) |
| 2.3 | `userbin/neoshell/src/main.rs` | `i18n_init()`, `i18n_load("neoshell")`, reemplazar strings por `tr!(...)` |
| 2.4 | `tools/neolocale/` (nuevo) | Binario de validación y creación de .nlt |

### Fase 3 — Migración de NeoInit + apps core (v0.54)

| Paso | Archivos | Descripción |
|------|----------|-------------|
| 3.1 | `userbin/neoinit/src/main.rs` | `i18n_init()`, `i18n_load("neoinit")`, migrar strings |
| 3.2 | `userbin/corehelp/src/main.rs` | Migrar strings |
| 3.3 | `userbin/coredir/src/main.rs` | Migrar strings |
| 3.4 | `userbin/corecopy/src/main.rs` | Migrar strings |
| 3.5 | Otras apps core | Migrar strings (kill, ps, label, fsck, ndreg) |

### Fase 4 — Segundo idioma + localización (v0.55+)

| Paso | Archivos | Descripción |
|------|----------|-------------|
| 4.1 | `locale/es-ES/*.nlt` | Crear traducciones al español |
| 4.2 | `locale/ca-ES/*.nlt` | Crear traducciones al catalán |
| 4.3 | `userbin/neoshell/` | Añadir comando `LANG` para cambiar locale en caliente |
| 4.4 | `userbin/neocfg/` | Añadir configuración de idioma en UI |

### Fase 5 — Herramientas restantes + GUI prep (v0.56+)

| Paso | Archivos | Descripción |
|------|----------|-------------|
| 5.1 | Apps Fase 2 (neologon, neotop, etc.) | Migrar strings |
| 5.2 | `libneodos/src/i18n.rs` | Añadir `format_str()` con placeholders `{N}` |
| 5.3 | `libneodos/src/i18n.rs` | Añadir caché LRU para hot path |
| 5.4 | Documentación | `docs/i18n.md` — guía para desarrolladores |

---

## 17. Componentes Afectados

| Sub sistema | Cambio | Complejidad |
|-------------|--------|-------------|
| `libneodos/src/i18n.rs` | Nuevo archivo (~350 líneas) | M |
| `libneodos/src/lib.rs` | + `pub mod i18n;` | S |
| `libneodos/src/macros.rs` | + `tr!()` macro | S |
| `userbin/neoshell/src/main.rs` | ~16 strings reemplazados por `tr!()` | M |
| `userbin/neoinit/src/main.rs` | ~12 strings reemplazados | S |
| `userbin/corehelp/` | ~14 strings reemplazados | S |
| Otras 5+ apps core | ~30 strings total | M |
| `neodos-kernel/src/cm/mod.rs` | + valor `Locale` por defecto | S |
| `scripts/create_ne2_image.py` | + archivos .nlt en disco | S |
| `tools/neolocale/` (nuevo) | Binario de validación (~200 líneas) | M |
| `docs/i18n.md` (nuevo) | Documentación para desarrolladores | M |

**No cambian:** Kernel (no traduce), Object Manager, Scheduler, Drivers, VFS, NeoFS, HAL, bootloader.

---

## 18. Riesgos

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|-------------|---------|------------|
| .nlt no se encuentra en disco | Alta (primeros despliegues) | Bajo | `tr!("clave")` devuelve la clave, no panic |
| Formato .nlt cambia en el futuro | Media | Medio | Version field en header permite evolución |
| Carga de archivos falla por permisos | Baja | Bajo | Error silencioso, fallback a key literal |
| Traducciones incompletas | Alta (nuevos idiomas) | Bajo | Fallback a en-US, luego a key |
| Rendimiento de búsqueda lineal con 500+ claves | Baja | Bajo | 500 comparaciones ≈ 0.5 μs en RAM. Si es necesario, hash en v2. |
| Aplicaciones existentes no migran a i18n | Media | Medio | No es crítico: las apps funcionan sin i18n_init(). Solo pierden traducción. |
