# NLT (Neo Language Table) — Referencia Rápida

> Formato binario de internacionalización de NeoDOS. Solo NLTv2.

---

## 1. Formato Fuente (TOML)

Los desarrolladores editan archivos `.toml`. El compilador `nltc` los convierte a `.nlt` binario.

```toml
[meta]
app = "neoshell"
language = "es-ES"

[ids]
IDS_OK = 1001
IDS_CANCEL = 1002

[strings]
IDS_OK = "Aceptar"
IDS_CANCEL = "Cancelar"
```

### Reglas
- `[meta]` requiere `app` y `language` (tag IETF: `en-US`, `es-ES`, etc.)
- `[ids]` asigna IDs numéricos a nombres simbólicos (opcional; auto-asignados si se omite)
- `[strings]` mapea los mismos nombres simbólicos a las traducciones
- Los IDs deben coincidir entre `[ids]` y `[strings]`
- Sin claves duplicadas

---

## 2. Formato Binario NLTv2

```
Offset  Size  Campo
──────  ────  ──────────────────────
0       4     Magic: "NLT2"
4       2     Version: u16 = 2
6       2     HeaderSize: u16 = 32
8       4     LanguageID: u32 LE
12      4     ApplicationID: u32 LE
16      4     StringCount: u32 LE (= N)
20      4     Flags: u32 LE
24      4     Checksum: u32 LE (CRC32)
28      4     Reserved: u32
32      8*N   IndexTable: { id: u32 LE, offset: u32 LE }[N]
32+8*N  ~     StringData: UTF-8 null-terminated
```

- **LanguageID** y **ApplicationID** son tablas predefinidas (ver `nltc --list-langs`)
- **CRC32** se calcula con el campo checksum = 0 durante la verificación
- **IndexTable** ordenada por ID para búsqueda binaria O(log n)
- Solo UTF-8. Sin BOM. Sin padding.

---

## 3. Compilador (`nltc`)

```text
nltc <input.toml> [output.nlt]        Compilar TOML → NLTv2
nltc --check <input.toml>             Validar sintaxis y semántica
nltc --generate-ids <input.toml>      Asignar IDs automáticos
nltc --generate-rust <input.toml>     Generar constantes Rust
nltc --scaffold <app> <language>      Crear plantilla TOML
nltc --list-langs                     Listar idiomas conocidos
nltc --lang-id <tag>                  Mostrar ID numérico de idioma
nltc --app-id <name>                  Mostrar ID numérico de aplicación
nltc --info <file.nlt>                Inspeccionar binario NLT
nltc --generate-all <locale-dir>      Compilar todo un directorio
```

### Integración NeoDev

`neodev build` y `neodev image` compilan automáticamente todos los `.toml` en `data/locale/` a `.nlt` antes de generar la imagen de disco.

---

## 4. Runtime API (`libneodos/src/i18n.rs`)

```rust
pub fn i18n_init();                       // Inicializar (leer Registry)
pub fn i18n_language() -> &'static str;   // Idioma activo (ej. "es-ES")
pub fn i18n_load(app: &str) -> Result;     // Cargar tabla NLTv2
pub fn i18n_get_id(id: u32) -> &'static str;  // Traducir ID → string
pub fn i18n_try_get_id(id: u32) -> Option<&'static str>;  // Opcional
pub fn i18n_unload(app: &str);            // Liberar tabla
pub fn i18n_reload_all();                 // Recargar todo (cambio idioma)
pub fn i18n_active_locale() -> &'static str;  // Locale activo
pub fn i18n_loaded_count() -> usize;      // Diagnóstico
pub fn i18n_is_loaded(app: &str) -> bool; // Consulta
```

### Macro

```rust
tr_id!(IDS_OK)   // → i18n_get_id(IDS_OK)
tr_id!(1001)     // → i18n_get_id(1001)
```

---

## 5. Constantes en Código

Las aplicaciones definen constantes manualmente o las generan con:

```bash
nltc --generate-rust neoshell.toml ids.rs
```

```rust
// ids.rs — Auto-generated
pub const IDS_OK: u32 = 1001;
pub const IDS_CANCEL: u32 = 1002;
```

Uso en aplicación:

```rust
mod ids;
use ids::*;
write_str(tr_id!(IDS_OK));
```

---

## 6. Localización de Archivos

```
C:\System\Locale\
  en-US\
    neoshell.nlt
    neoinit.nlt
    corehelp.nlt
    ...
  es-ES\
    neoshell.nlt
    ...
```

### Cadena de fallback
1. `C:\System\Locale\{lang}\{app}.nlt`
2. `C:\System\Locale\{lang-only}\{app}.nlt` (ej. `es`)
3. `C:\System\Locale\en-US\{app}.nlt`
4. Si nada funciona, `i18n_get_id()` devuelve `"?"`

---

## 7. Registry

```
HKLM\System\CurrentControlSet\Control\Locale
  Language   REG_SZ   "en-US"    (tag IETF del idioma activo)
```

`i18n_init()` lee esta clave. Si no existe, se establece `"en-US"` por defecto en el arranque del kernel (`ensure_language_default()`).

Futuro: `HKCU\Control\Locale\Language` para idioma por usuario.

---

## 8. IDs de Lenguaje Estándar

| ID  | Tag    | Idioma             |
|-----|--------|--------------------|
| 1   | en-US  | English            |
| 2   | es-ES  | Español            |
| 3   | fr-FR  | Français           |
| 4   | de-DE  | Deutsch            |
| 5   | it-IT  | Italiano           |
| 8   | ca-ES  | Català             |
| 12  | ja-JP  | 日本語             |
| 13  | zh-CN  | 简体中文           |
| 14  | ru-RU  | Русский            |

Ver `nltc --list-langs` para la lista completa.

Los IDs 0x8000+ se generan por hash para tags no estándar.

---

## 9. IDs de Aplicación Estándar

| ID  | App          |
|-----|--------------|
| 1   | neoshell     |
| 2   | neoinit      |
| 3   | corehelp     |
| 4   | coredir      |
| 5   | corecopy     |
| 6   | coretype     |
| 7   | neolocale    |
| 8-40 | Otras apps |

Los IDs 0x8000+ se generan por hash.

---

## 10. Herramientas

| Herramienta    | Propósito                          |
|----------------|------------------------------------|
| `nltc`         | Compilador TOML → NLTv2            |
| `neolocale`    | Validación, diff, stats, check     |
| `gen_nlt_toml.py` | Generar TOML desde datos Python |

### flujo de trabajo

```bash
# 1. Crear fuente
nltc --scaffold miapp es-ES > miapp.toml

# 2. Editar miapp.toml con IDs y traducciones
# 3. Generar constantes Rust
nltc --generate-rust miapp.toml src/ids.rs

# 4. Compilar
nltc miapp.toml data/locale/es-ES/miapp.nlt

# 5. Usar en código
write_str(tr_id!(IDS_SALUDO));
```

---

## 11. Herramienta neolocale

```text
neolocale validate <file.nlt>     Validar formato NLTv2
neolocale stats    <file.nlt>     Estadísticas de entradas
neolocale diff     <f1> <f2>      Comparar dos archivos NLT
neolocale check    [dir]          Buscar traducciones faltantes
neolocale create   <app>          Crear scaffold NLTv2 vacío
```

---

## 12. Aplicaciones con NLT

**Todas las 42 aplicaciones User-Bin** tienen NLT completo:

| Categoría | Aplicaciones |
|-----------|-------------|
| Core | corecls, corecopy, coredel, coredir, corerd, coreren, coremd, corehelp, coretype, cd, echo, tree |
| Sistema | neoinit, neoshell, poweroff, reboot, ver, vol, label, drives |
| Procesos | kill, ps, pri, neotop |
| Memoria | neomem |
| Teclado | keyb, neokey |
| Red | dhcpd, dhcptest, ipconfig, netcfg, ping |
| Drivers | loadnem, ndreg |
| Utilidades | colors, cpuinfo, datetime, fsck, nxlocale, nxres, nxverify, progress |
| Tests | cmdtest, shtest, stresscmd |

Cada una con traducciones completas a en-US, es-ES, ca-ES.

## 13. Norma de desarrollo

**No se permiten cadenas visibles hardcodeadas en User-Bin nuevos o existentes.**
Todo mensaje visible debe añadirse al sistema NLT y traducirse simultáneamente a:
- en-US
- es-ES
- ca-ES

## 14. Añadir un nuevo idioma

1. Crear `data/locale/{locale}/` con los archivos `.toml` para cada app
2. Ejecutar `nltc --generate-all data/locale/{locale}`
3. El sistema NLT cargará automáticamente los archivos `.nlt` desde `C:\System\Locale\{locale}\`
4. Si el idioma no está en la lista de IDs conocidos, se usará un hash CRC32

## 15. Añadir una nueva clave

1. Añadir la entrada en `[ids]` y `[strings]` en los tres archivos `.toml` (`en-US`, `es-ES`, `ca-ES`)
2. Recompilar: `nltc --generate-all data/locale/{locale}`
3. En el código: definir constante `const IDS_NUEVA: u32 = N;` y usar `tr_id!(IDS_NUEVA)`
```
