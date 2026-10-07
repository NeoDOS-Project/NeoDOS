# NeoDOS — Arquitectura de disco (estilo NT)

> **Estado:** propuesta, no implementada.
> **Fecha:** 2026-09-30
> **Decisiones cerradas:** raíz `C:\NeoDOS` · aplicaciones en `C:\Program Files\` (con espacio).
> Este documento describe el diseño objetivo y el plan de migración. Mientras no se
> implemente, el código (no este documento) describe el layout real.

---

## 1. Estado actual

El contenido de `C:` se genera íntegramente en `neodev/src/image.rs::collect_files()`
(repositorio NeoDev, proyecto independiente). El árbol real hoy es:

```text
C:\
├── README.TXT
├── Temp/.empty
├── Programs\                      # 28 .NXE del OS (mezcla shell + utilidades core)
│   ├── neoshell.nxe  neoinit.nxe  cmdtest.nxe
│   ├── cd.nxe  corehelp.nxe  coredir.nxe  coretype.nxe
│   ├── corecls.nxe corecopy.nxe coredel.nxe coreren.nxe
│   ├── coremd.nxe corerd.nxe  drives.nxe  ps.nxe  keyb.nxe
│   └── datetime neomem echo label poweroff colors neokey
│       nxres nxlocale nxverify ping hostname
└── System\
    ├── Tools\                     # System administration .NXE
    │   ├── kill pri fsck progress neotop
    │   ├── dhcpd netcfg netapplier ipconfig cpuinfo neolocale dhcptest
    │   ├── nslookup
    │   └── reboot shtest stresscmd tree ver vol   # movidos aquí por capacidad
    ├── Libraries\                 # fs.nxl math.nxl console.nxl net.nxl
    ├── Keyboard\                  # US.kbd Spanish.kbd
    ├── Locale\<lang>\*.nlt
    ├── Drivers\                   # *.nem BOOT y SYSTEM mezclados
    └── Registry\SYSTEM.hiv
```

A nivel de particiones (`neodev.toml`, GPT): `ESP (100 MB, FAT32) → NeoDOS (100 MB, NE2) → padding`.
Sin partición MSR ni Recovery.

---

## 2. Problemas del layout actual

1. **La división `\Programs` vs `\System\Tools` no es semántica, es de capacidad.**
   El propio builder lo documenta:
   `// Moved out of /Programs to keep its single leaf within capacity.`
   En NT los ejecutables de sistema viven todos en `\Windows\System32`; aquí
   `System\Tools` funciona como desbordamiento, no como categoría.
2. **`\System` es un cajón plano**: binarios, librerías, drivers, hives, teclados y
   locales comparten nivel. NT separa por rol (`System32`, `drivers`, `config`,
   `Fonts`, `INF`, `Globalization`).
3. **Drivers BOOT y SYSTEM mezclados** en `\System\Drivers` (`image.rs`).
4. **Hives con nombre y extensión no-NT**: `\System\Registry\SYSTEM.hiv` frente a
   `\Windows\System32\config\SYSTEM`.
5. **No existen `\Users`, `\Program Files` ni `\ProgramData`**: no hay separación
   entre binarios del OS, aplicaciones instaladas y datos de usuario.
6. **Descubrimiento (PATH) partido**: el shell arranca con `\Programs`, la
   finalización de comandos asume `\Programs` y `res.rs` busca recursos solo en
   `C:\Programs\`.
7. **Incoherencia documental**: `docs/architecture/vision.md` cita
   `C:\System\Config\` para la persistencia del registro, pero el código usa
   `C:\System\Registry\`.

---

## 3. Restricción clave: directorios multi-hoja

El límite de ~28 entradas por directorio **no viene del runtime**, sino del generador
de imágenes:

- **Runtime:** `neodos-kernel/src/fs/btree.rs` implementa un B-tree persistente con
  `NodeType::Internal`/`Leaf`, `split_node()` y `split_internal()`. Tests
  `btree_forced_split` y `btree_stress_insert_500` cubren el caso multi-hoja.
- **Builder:** `neodev/src/image.rs::make_btree_leaf()` emite **una sola hoja** y hace
  `bail!` si no cabe (`multi-block directory support is not implemented`).

Con `DIRENTRY_SIZE = 128` y bloque de 4096 B, una hoja aloja ~28-29 entradas. Un
`System32` estilo NT (decenas de binarios) no cabe hasta que el builder emita
nodos internos y hojas encadenadas. **Es el prerrequisito bloqueante del rediseño.**

---

## 4. Árbol objetivo

```text
C:\
├── NeoDOS\                              # ≈ C:\Windows
│   ├── System32\                        # TODOS los .NXE de sistema
│   │   ├── config\                      # hives: SYSTEM.hiv (+ SOFTWARE/SAM/SECURITY/DEFAULT)
│   │   ├── drivers\                     # .NEM SYSTEM
│   │   │   └── BOOT\                    # .NEM boot-critical (ps2kbd, serial, rtc…)
│   │   ├── Keyboard\                    # US.kbd, Spanish.kbd
│   │   ├── Globalization\<lang>\        # *.nlt
│   │   ├── Fonts\
│   │   ├── INF\
│   │   ├── Logs\                        # WDT_*.dmp / eventos
│   │   └── Temp\
│   ├── Libraries\                       # fs.nxl math.nxl console.nxl net.nxl
│   ├── Tasks\
│   └── Boot\                            # boot.cfg
├── Program Files\<app>\                 # apps instaladas (NXE + NXL + recursos por app)
├── ProgramData\                         # datos/config machine-wide
├── Users\<usuario>\
│   ├── Desktop\ Documents\ Downloads\
│   └── AppData\Local\  AppData\Roaming\
├── System Volume Information\           # snapshots NeoFS (oculto/sistema)
└── $Recycle.Bin\
```

### Correspondencia con NT

| NT | NeoDOS | Contenido |
| --- | --- | --- |
| `\Windows` | `\NeoDOS` | raíz del sistema |
| `\Windows\System32` | `\NeoDOS\System32` | `.NXE` de sistema (hoy `Programs` + `System\Tools`) |
| `\Windows\System32\config` | `\NeoDOS\System32\config` | hives del registro |
| `\Windows\System32\drivers` | `\NeoDOS\System32\drivers` | drivers `.NEM` |
| `\Windows\System32\*.dll` | `\NeoDOS\Libraries` | librerías `.NXL` |
| `\Windows\Fonts` | `\NeoDOS\System32\Fonts` | fuentes |
| `\Windows\INF` | `\NeoDOS\System32\INF` | metadatos de drivers |
| `\Windows\Globalization` | `\NeoDOS\System32\Globalization` | localizaciones `.nlt` |
| `\Program Files` | `\Program Files` | aplicaciones instaladas |
| `\ProgramData` | `\ProgramData` | datos machine-wide |
| `\Users\<u>\AppData` | `\Users\<u>\AppData` | perfil de usuario |

Extensiones nativas: `.NXE` ≈ `.exe`, `.NXL` ≈ `.dll`, `.NEM` ≈ `.sys`.

**Ventajas:** desaparece el split artificial `Programs`/`System\Tools`; encaja con el
Object Manager, que ya es NT-like (`\Device`, `\DosDevices`, `\Registry\Machine\...`);
y `\Users` encaja con la seguridad SAM/SID/ACL ya existente.

---

## 5. Decisiones de diseño

| Decisión | Valor | Motivo |
| --- | --- | --- |
| Raíz del OS | `C:\NeoDOS` | Análogo a `\Windows`, identidad propia, sin choque con el namespace de objetos `\System`. |
| Directorio de apps | `C:\Program Files\` (con espacio) | Fidelidad NT. **Requiere** quoting/`;` correctos en el shell y en el PATH. |
| Hives | `\NeoDOS\System32\config\SYSTEM.hiv` | Equivale a `config\SYSTEM`. Valorar quitar la extensión para máxima fidelidad. |
| Librerías | `\NeoDOS\Libraries\` separado de `System32` | Claridad; en NT las DLL viven en `System32`, pero aquí `.NXL` es una categoría propia. |
| Datos de usuario | `\Users\<usuario>\AppData\{Local,Roaming}` | Alineado con SAM/SID. |
| Particiones (futuro) | `ESP → MSR(16 MB) → NeoDOS → Recovery` | Orden NT. No urgente. |

---

## 6. Impacto de la migración

Las rutas están codificadas como literales en varios subsistemas. Inventario no
exhaustivo:

| Ruta | Consumidor |
| --- | --- |
| `C:\Programs\neoshell.nxe` | `neodos-kernel/src/boot/mod.rs`, `neodos-kernel/src/cm/init.rs`, libneodos |
| `C:\Programs\neoinit.nxe` | `neodos-kernel/src/boot/mod.rs` |
| `C:\Programs` (PATH/recursos) | `userbin/neoshell/src/shell.rs`, `userbin/neoshell/src/completion.rs`, `userbin/corehelp/src/main.rs`, `libneodos/src/res.rs` |
| `C:\System\Registry\*.hiv` | `neodos-kernel/src/cm/init.rs` |
| `C:\System\Libraries\*.nxl` | `neodos-kernel/src/boot/mod.rs`, `libneodos/src/console.rs` |
| `C:\System\Drivers\` | `docs/architecture/overview.md`, libneodos, boot_loader |
| `C:\System\Keyboard\` | `neodos-kernel/src/boot/mod.rs` |
| `C:\System\Locale\` | `libneodos/src/i18n.rs` |
| `C:\System\Tools\dhcpd.nxe`, `netapplier.nxe` | `neodos-kernel` (servicios) |
| `C:\Logs\WDT_*.dmp` | `neodos-kernel` |

**Estrategia:** introducir una constante única de raíz — propuesta
`ND_SYSTEM_ROOT = "C:\\NeoDOS"` — en kernel, `libneodos` y userbin, y eliminar
literales. El `PATH` debe pasar a leerse del registro (NT:
`CurrentControlSet\Control\Session Manager\Environment`) en lugar del `\Programs`
por defecto del shell. `libneodos/src/res.rs` debe buscar recursos **junto al
ejecutable**, no en una ruta global.

> El builder vive en el repositorio **NeoDev** (proyecto independiente), por lo que
> la Fase 3 es un cambio cross-repo.

---

## 7. Plan por fases

### Fase 1 — Directorios multi-hoja (bloqueante)

- Emitir nodos internos + hojas encadenadas en `neodev/src/image.rs::make_btree_leaf()`.
- Tests de imagen con directorios de 50+ entradas.
- **Aceptación:** `C:\NeoDOS\System32` con 50+ entradas se construye y `DIR` lo lista correctamente.

### Fase 2 — Abstracción de raíz y PATH

- Constante `ND_SYSTEM_ROOT` y eliminación de literales en kernel, `libneodos` y userbin.
- `PATH` en el registro; default apuntando a `System32`, `System\Tools` retirado.
- Asegurar quoting para `Program Files` (espacio) antes de escribir en esa ruta.
- Independiente de la Fase 1; ambas desbloquean la Fase 3.

### Fase 3 — Migración del builder y consumidores

- Reescribir `collect_files()` al árbol de §4.
- Ajustar `cm/init.rs`, `i18n.rs`, `res.rs`, `console.rs`, servicios y `main.rs`.
- `SYSTEM.hiv` → `config\SYSTEM.hiv` (valorar sin extensión).

### Fase 4 — Documentación e invariantes

- Actualizar `docs/architecture/overview.md`, `docs/filesystem/overview.md`,
  `docs/filesystem/neofs-v2.md`.
- Corregir `docs/architecture/vision.md` (`C:\System\Config\` → `System32\config`).
- Añadir invariante en `docs/architecture/source-of-truth.md`: *toda ruta de sistema
  cuelga de `C:\NeoDOS`; ningún subsistema codifica literales de layout*.

### Fase 5 — Opcional

- `\Users\<usuario>` + `AppData` ligado a SAM/SID/ACL.
- GPT estilo NT: `ESP → MSR(16 MB) → NeoDOS → Recovery`.

### Orden recomendado

```text
1 (multi-hoja) → 2 (raíz + PATH) → 3 (builder + consumidores) → 4 (docs) → 5 (perfiles/GPT)
```
