# Auditoría Arquitectónica: Componentes Candidatos a Repositorios Independientes

> **Versión del documento:** v1.0 (2026-07-18)
> **Proyecto:** NeoDOS v0.50-dev
> **Alcance:** Árbol completo del repositorio `neodos/`
> **Objetivo:** Identificar componentes separables, justificar cada decisión y proponer una hoja de ruta de migración.

---

## 1. Resumen Ejecutivo

NeoDOS ha alcanzado una madurez estructural que permite evaluar la separación de ciertos componentes en repositorios independientes. El análisis cubre **45 directorios y >81,000 líneas de Rust**, evaluando acoplamiento, dependencias, ciclos de versiones y potencial de reutilización.

**Conclusiones principales:**

| Decisión | Componentes |
|----------|-------------|
| Ya separado | `NeoDev` |
| Migrar inmediatamente | `NeoDOS-LSP`, `NeoMCP`, `NeoTools` (host tools) |
| Migrar post-v1.0 (API estable) | `NeoDrivers`, `NeoTranslations`, `NeoDocs` |
| Mantener en NeoDOS | Kernel, bootloader, libneodos, NXLs, userbin, scripts de build |
| No recomendable separar | Tests unitarios, skills, configuración AI |

---

## 2. Análisis por Componente

### 2.1 Kernel (`neodos-kernel/`)

| Atributo | Valor |
|----------|-------|
| Líneas | ~52,945 (65.4% del total) |
| Módulos | 49 (arch, scheduler, memory, fs, net, security, etc.) |
| Dependencias externas | log, x86_64, lazy_static, spin, linked_list_allocator |
| Dependientes | bootloader (BootInfo ABI), libneodos (syscall ABI), NXLs (NXL ABI), drivers (NEM ABI) |

**Criterios:**
- **Responsabilidad definida:** Sí — núcleo del SO.
- **Puede evolucionar independientemente:** No — es el centro de todas las dependencias.
- **Ciclo de versiones propio:** Sí (v0.49.0).
- **Reutilizable por otros proyectos:** No — es el SO mismo.
- **Dependencias del kernel:** N/A (es el kernel).
- **Publicable como Open Source independiente:** Es el proyecto principal.

**Veredicto: MANTENER EN NEODOS.** El kernel es el componente central. Separarlo no tiene sentido arquitectónico.

---

### 2.2 Bootloader (`neodos-bootloader/`)

| Atributo | Valor |
|----------|-------|
| Versión | v0.11.0 |
| Target | x86_64-unknown-uefi |
| Líneas | 3 archivos (main.rs, file.rs, memory.rs) |
| Dependencias | uefi 0.37, log |

**Criterios:**
- **Responsabilidad definida:** Sí — cargar el kernel en memoria y pasarle BootInfo.
- **Puede evolucionar independientemente:** Limitado — el BootInfo ABI está atado a la versión del kernel.
- **Ciclo de versiones propio:** Sí, pero debe sincronizarse con el kernel.
- **Reutilizable por otros proyectos:** No — diseñado exclusivamente para NeoDOS.
- **Dependencias del kernel:** Alta — BootInfo ABI, direcciones de carga, protocolo de arranque.

**Veredicto: MANTENER EN NEODOS.** El BootInfo ABI cambia con cada versión del kernel. Tenerlo en repo separado añadiría complejidad de sincronización sin beneficio real.

---

### 2.3 libneodos (syscall wrappers)

| Atributo | Valor |
|----------|-------|
| Versión | v0.2.1 |
| Target | x86_64-unknown-none |
| Archivos | 13 (syscall.rs, io.rs, fs.rs, mem.rs, args.rs, console.rs, keyboard.rs, i18n.rs, res.rs, seh.rs, macros.rs, export.rs, lib.rs) |

**Criterios:**
- **Responsabilidad definida:** Sí — biblioteca estándar de usuario (wrappers de syscall).
- **Puede evolucionar independientemente:** No — cada syscall wrapper debe coincidir exactamente con el SSDT del kernel.
- **Ciclo de versiones propio:** Técnicamente sí, pero cualquier cambio requiere kernel sincronizado.
- **Reutilizable por otros proyectos:** No — específico de NeoDOS.
- **Dependencias del kernel:** Absolutas — cada función es una llamada a syscall del kernel.

**Veredicto: MANTENER EN NEODOS.** La ABI de syscalls (SSDT, RAX 0-59) debe estar en lockstep con el kernel. Separarlo crearía riesgos de versiones incompatibles. **Reevaluar post-v1.0 cuando la ABI esté congelada.**

---

### 2.4 NXL Libraries (`libneodos-nxl/`, `libmath-nxl/`, `libconsole-nxl/`, `libnet-nxl/`)

| Atributo | Valor |
|----------|-------|
| Naturaleza | DLLs cargadas por el kernel en direcciones fijas |
| Versiones | v0.2.1, v0.1.0, v0.1.0, v0.1.0 |
| Targets | x86_64-unknown-none con linker scripts propios |

**Criterios:**
- **Responsabilidad definida:** Sí — bibliotecas compartidas Ring 3.
- **Puede evolucionar independientemente:** No — cargadas por el kernel NXL loader en direcciones fijas.
- **Ciclo de versiones propio:** Técnicamente sí, pero deben compilarse contra la misma ABI que el kernel espera.
- **Reutilizable por otros proyectos:** No.
- **Dependencias del kernel:** Absolutas (formato NXL, export table, direcciones de carga).

**Veredicto: MANTENER EN NEODOS.** Están intrínsecamente ligadas al kernel NXL loader.

---

### 2.5 libnet (`libnet/`)

Wrapper de red que depende de `libneodos` y `libnet-nxl`. Misma justificación que libneodos.

**Veredicto: MANTENER EN NEODOS.**

---

### 2.6 User Binaries (`userbin/`)

| Atributo | Valor |
|----------|-------|
| Cantidad | 34 binarios .NXE |
| Líneas | ~15,000 |
| Dependencias | libneodos (todos), libnet (los de red) |

**Criterios:**
- **Responsabilidad definida:** Parcial — mezcla herramientas core (copy, del, dir), utilidades de red (ipconfig, ping), herramientas de sistema (ps, kill, neotop), y el shell (neoshell).
- **Puede evolucionar independientemente:** No — todos dependen de libneodos, que a su vez depende del kernel.
- **Ciclo de versiones propio:** No práctico — cada binario compila contra una versión específica de libneodos.
- **Reutilizable por otros proyectos:** No.
- **Dependencias del kernel:** Altas (vía libneodos syscall wrappers).

**Veredicto: MANTENER EN NEODOS.** Todos los binarios dependen de libneodos. Separarlos añadiría complejidad de compilación sin beneficio. Sin embargo, el roadmap (M2.3 v0.58) planea refactorizar las herramientas — evaluar separación de utilidades de red (ipconfig, ping, nslookup, dhcptest) como **NeoTools** post-v1.0 si se desacoplan de libneodos vía API pública.

---

### 2.7 Drivers NEM (`drivers/`)

| Atributo | Valor |
|----------|-------|
| Cantidad | 10 drivers |
| Lista | acpi, ahci, ata, e1000, pci, ps2kbd, ps2mouse, rtc, serial, virtio-blk |
| Formato | NEM v3 (independiente, crate-type = ["lib"]) |
| Dependencias | Ninguna de Rust (standalone no_std) |
| ABI | Versionada (actual v8) |

**Evaluación detallada:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — cada driver maneja un dispositivo específico |
| Puede evolucionar independientemente | Sí — ya compilan independientemente con su propio Cargo.toml |
| Ciclo de versiones propio | Sí — cada driver podría versionarse independientemente |
| Reutilizable por otros proyectos | Potencialmente — cualquier SO que soporte NEM ABI |
| Dependencias del kernel | Solo la tabla `hst_*` del kernel (host services). ABI versionada. |
| Publicable como Open Source independiente | Sí — drivers de dispositivos estándar (PCI, ATA, AHCI, e1000) |

**Ventajas de separar:**
- Ciclo de releases independiente (un driver puede actualizarse sin tocar el kernel)
- Comunidad puede contribuir drivers sin acceso al kernel completo
- CI más rápida (solo compilar el driver modificado)
- Posible reutilización cross-platform

**Inconvenientes de separar:**
- Complejidad de compatibilidad — driver compilado contra NEM ABI v8 debe funcionar con kernel v8
- Necesidad de un proceso de certificación cross-repo
- Riesgo de versiones incompatibles si la ABI cambia
- Pipeline de build más complejo (images necesitan recolectar drivers de otro repo)

**Veredicto: MANTENER EN NEODOS hasta v1.0.** La NEM ABI aún está en desarrollo activo (v8, no congelada). Separar antes de la congelación crearía una pesadilla de mantenimiento. **Migrar cuando exista una API estable (post-v1.0 freeze).**

---

### 2.8 Tools (`tools/`)

| Tool | Versión | Dependencias | Líneas | Independencia |
|------|---------|-------------|--------|---------------|
| `nxeinfo` | v0.1.0 | serde_json | ~200 | Alta — solo lee archivos .NXE |
| `nxpkg` | v0.1.0 | ninguna | ~300 | Alta — empaqueta .NXP |
| `nxdump` | v0.1.0 | ninguna | ~400 | Alta — dump de formatos |
| `nltc` | v0.1.0 | serde, serde_json, toml | ~500 | Alta — compila TOML → NLT |
| `kbdcompile` | v0.1.0 | ninguna | ~200 | Alta — compila KLC → KBD |
| `nem-pack.py` | - | Python 3 | ~100 | Alta — empaqueta NEM |

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — cada tool tiene un propósito único |
| Puede evolucionar independientemente | Sí — sin dependencias del kernel |
| Ciclo de versiones propio | Sí |
| Reutilizable por otros proyectos | Sí — nxdump y nxeinfo pueden analizar binarios de cualquier SO |
| Dependencias del kernel | Ninguna — son herramientas host estándar |
| Publicable como Open Source independiente | Sí |

**Veredicto: MIGRAR INMEDIATAMENTE.** Las host tools son el candidato más claro. No tienen dependencias del kernel, tienen ciclos de desarrollo independientes, y son útiles incluso sin el kernel. Propuesta:

- **NeoTools**: nxeinfo, nxpkg, nxdump (herramientas de análisis de binarios)
- **NeoLangTool**: nltc + kbdcompile (compiladores de formatos del SO)
- Alternativa: agrupar todo como `NeoTools` con workspace de Cargo

---

### 2.9 LSP Server (`neodos-lsp/`)

| Atributo | Valor |
|----------|-------|
| Versión | v0.1.0 |
| Edición | 2024 |
| Dependencias | serde, serde_json, lsp-types, lsp-server, dashmap, rayon, walkdir, log, env_logger, crossbeam, parking_lot, ignore, globset, url |
| Dependencias del kernel | Ninguna (analiza código fuente, no ejecuta el SO) |

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — servidor LSP para desarrollo NeoDOS |
| Puede evolucionar independientemente | Sí — proyecto Rust estándar con 15+ dependencias host |
| Ciclo de versiones propio | Sí |
| Reutilizable por otros proyectos | Limitado — contiene conocimiento específico de NeoDOS (syscalls, NEM, boot phases) |
| Dependencias del kernel | Ninguna |
| Publicable como Open Source independiente | Sí |

**Veredicto: MIGRAR INMEDIATAMENTE.** Es un proyecto Rust completamente estándar, con muchas dependencias host que no tienen nada que ver con el kernel. Su desarrollo (features del LSP, bugs del indexador) es ortogonal al desarrollo del SO. Propuesta: **NeoDOS-LSP**.

---

### 2.10 Scripts (`scripts/`)

| Script | Naturaleza | Independencia |
|--------|-----------|---------------|
| `check_deps.py` | Validación arquitectónica | Media — analiza código del kernel, pero es standalone |
| `crash_analyzer.py` | Análisis post-mortem | Alta — analiza dumps |
| `gen_nlt_toml.py` | Generación de traducciones | Alta |
| `gen_nlt.py` | Generación de traducciones | Alta |
| `gen_system_hiv.py` | Generación de Registry | Baja — conoce estructura del Registry del kernel |
| `mcp_server/` | Servidor MCP | Alta — Python independiente |
| `mcp-server.sh` | Lanzador | Alta |
| `setup-network.sh` | Configuración de red QEMU | Baja — específico del setup de desarrollo |

**Veredicto: SEPARACIÓN SELECTIVA.**

| Componente | Decisión | Razón |
|------------|----------|-------|
| `scripts/mcp_server/` | **Migrar inmediatamente** | Servidor Python independiente, sin dependencias del kernel |
| `scripts/check_deps.py` | **Mantener** | Validación arquitectónica atada a la estructura del kernel |
| `scripts/crash_analyzer.py` | **Mantener** | Conoce formatos internos del crash dump del kernel |
| `scripts/gen_nlt*.py` | **Migrar con NeoTranslations** | Atados al formato NLT pero no al kernel |
| `scripts/gen_system_hiv.py` | **Mantener** | Conoce la estructura interna del Registry |
| `scripts/setup-network.sh` | **Mantener** | Específico del entorno de desarrollo |

---

### 2.11 Data (`data/`)

| Componente | Contenido | Independencia |
|------------|-----------|---------------|
| `data/locale/{en-US,es-ES,ca-ES}/` | 47 archivos TOML + 47 NLT compilados por locale (282 archivos total) | Alta — datos de traducción puros |
| `data/keyboard/` | KLC sources + KBD compilados | Alta — datos de layouts de teclado |

**Veredicto: MIGRAR CUANDO EXISTA API ESTABLE.** Los datos de traducción son el candidato ideal para un repositorio comunitario. Cualquier persona puede contribuir traducciones sin conocer el kernel. Sin embargo, el formato NLT aún está en desarrollo (I18N-P7..P12 planean compresión, UTF-16, pluralización, etc.). Propuesta: **NeoTranslations** (post-v1.0, cuando el formato NLT esté congelado).

---

### 2.12 Docs (`docs/`)

| Atributo | Valor |
|----------|-------|
| Archivos | 46 documentos Markdown |
| Contenido | Arquitectura, subsistemas, formatos, guías, roadmap, especificaciones |

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — documentación del proyecto |
| Puede evolucionar independientemente | Técnicamente sí, pero referencias cruzadas con el código |
| Ciclo de versiones propio | Posible (versionado de documentación) |
| Reutilizable por otros proyectos | No — específica de NeoDOS |
| Dependencias del kernel | Baja — documenta el kernel pero no depende de él |

**Ventajas de separar:**
- Comunidad puede contribuir docs sin clonar el kernel
- CI de docs independiente (markdownlint, enlaces rotos)
- Posible publicación en GitHub Pages

**Inconvenientes:**
- La documentación técnica referencia código fuente directamente (líneas, nombres de funciones)
- Riesgo de desincronización entre docs y código
- Los PRs que cambian API necesitarían PRs simultáneos en dos repos

**Veredicto: MANTENER EN NEODOS hasta v1.0.** La documentación está demasiado entretejida con el código en esta fase de desarrollo activo. Separarla ahora crearía riesgos de desactualización. **Evaluar NeoDocs post-v1.0** cuando la API esté estable y los documentos dejen de cambiar frecuentemente.

---

### 2.13 Tests

| Componente | Ubicación | Naturaleza |
|------------|-----------|------------|
| Tests unitarios kernel | `neodos-kernel/src/testing.rs` (656+ tests) | En el kernel |
| Tests de integración | `neodev test` (vía neodev, ya separado) | Herramienta externa |
| Tests de validación | `scripts/check_deps.py` | Script Python |

**Veredicto: NO RECOMENDABLE SEPARAR.** Los tests unitarios deben vivir con el código que prueban. Los tests de integración ya están en NeoDev (separado). Un repositorio `NeoTest` separado añadiría complejidad de sincronización sin beneficio real.

---

### 2.14 Skills (`skills/`)

18 checklists AI especializados. Atados a `AGENTS.md` que vive en la raíz del proyecto.

**Veredicto: MANTENER EN NEODOS.** Los skills referencian la estructura del código directamente. Separarlos rompería los workflows de AI.

---

### 2.15 Preferences, .opencode, opencode.json

Configuración del entorno de desarrollo AI.

**Veredicto: MANTENER EN NEODOS.** Configuración específica del proyecto.

---

### 2.16 Imágenes de disco y binarios precompilados

`disk_image.img`, `disk_image.vdi`, `kernel.elf`, `bootloader.efi`, `*.nxl`, `*.nxe`, net.nxl, console.nxl.

**Veredicto: MANTENER EN NEODOS (en .gitignore).** Son artefactos de build, no código fuente.

---

## 3. Candidatos Específicos

### 3.1 NeoDev

**Estado actual: YA SEPARADO.** `NeoDev` v0.2.0 es un repositorio independiente en `github.com/NeoDOS-Project/NeoDev`. El código en `neodev/` dentro del workspace raíz (`/home/amartinper/rust-os/neodev/`) ya no forma parte del tree principal de NeoDOS.

**No requiere acción.**

---

### 3.2 NeoSDK

**Propuesta:** Biblioteca de desarrollo para aplicaciones NeoDOS.

**Componentes evaluados:**

| Componente | ¿Separable? | Razón |
|------------|-------------|-------|
| `libneodos` | No | Syscall ABI lockstep con kernel |
| `libnet` | No | Depende de libneodos + kernel net ABI |
| `libneodos-nxl` | No | Cargado por kernel NXL loader |
| `libnet-nxl` | No | Cargado por kernel NXL loader |

**Análisis:** Un SDK tiene sentido cuando hay una API estable y aplicaciones de terceros. Actualmente:
- La ABI de syscalls (SSDT) no está congelada (cambia entre versiones v0.x)
- No hay aplicaciones de terceros
- libneodos es pequeño y simple (~13 archivos)
- El empaquetado .NXE aún está en desarrollo (M2.1, v0.56)

**Veredicto: NO CREAR NeoSDK POR AHORA.** Mantener libneodos en NeoDOS. Evaluar la creación de un SDK público **post-v1.0** cuando:
1. La ABI de syscalls esté congelada
2. Existan aplicaciones de terceros
3. Se pueda publicar libneodos como crate independiente en crates.io

---

### 3.3 NeoDrivers

**Propuesta:** Repositorio independiente para drivers NEM.

**Evaluación de drivers específicos:**

| Driver | Madurez | Dependencias | ¿Separable ahora? |
|--------|---------|-------------|-------------------|
| `e1000.nem` | Media (funcional) | NEM ABI v8 | Potencialmente, pero ABI no congelada |
| `acpi.nem` | Media | NEM ABI v8 | Igual |
| `ahci.nem` | Alta (794 líneas) | NEM ABI v8 | Igual |
| `ata.nem` | Alta (616 líneas) | NEM ABI v8 | Igual |
| `pci.nem` | Alta (407 líneas) | NEM ABI v8 | Igual |
| `ps2kbd.nem` | Alta (268 líneas) | NEM ABI v8 | Igual |
| `virtio-blk.nem` | Media | NEM ABI v8 | Igual |

**Ventajas:**
- La NEM ABI está versionada (v8) y permite compatibilidad hacia atrás
- Cada driver ya compila independientemente
- Drivers de dispositivos estándar tienen valor educativo propio
- Comunidad podría contribuir drivers para hardware específico

**Inconvenientes:**
- La ABI NEM aún no está congelada (objetivo: v1.0)
- El pipeline de build necesita coordinar versiones driver↔kernel
- El kernel tiene stubs de arranque para algunos drivers (ata, ahci, ps2kbd, pci, rtc) que deben migrarse también
- La certificación de drivers (7 estados) es un proceso kernel-side

**Veredicto: MANTENER EN NEODOS hasta v1.0.** Documentar la NEM ABI congelada en v1.0 y entonces evaluar la separación. Mientras tanto, mantener el directorio `drivers/` pero empezar a tratarlos como componentes cuasi-independientes: CI individual, changelog propio, y revisión separada.

---

### 3.4 NeoTools

**Propuesta:** Repositorio independiente para herramientas de análisis y manipulación de binarios.

**Componentes:**
- `nxeinfo` — inspector de binarios .NXE
- `nxpkg` — empaquetador .NXP
- `nxdump` — dump de ELF/NXE/NEM

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — herramientas de análisis de formatos del SO |
| Puede evolucionar independientemente | Sí — proyectos Rust estándar sin deps del kernel |
| Ciclo de versiones propio | Sí |
| Reutilizable por otros proyectos | Sí — nxdump y nxeinfo pueden analizar formatos genéricos |
| Dependencias del kernel | Ninguna |
| Publicable como Open Source independiente | Sí |

**Veredicto: MIGRAR INMEDIATAMENTE.** Crear repositorio `NeoTools` con workspace de Cargo conteniendo los tres crates. La CI debe compilar cada tool y validar contra formatos de ejemplo.

**Quedan fuera de NeoTools:**
- `nltc` → migrar con `NeoTranslations` (post-v1.0)
- `kbdcompile` → migrar con `NeoTranslations` (post-v1.0)
- `nem-pack.py` → mantener en NeoDOS (parte del pipeline de build de drivers)

---

### 3.5 NeoDocs

**Propuesta:** Repositorio independiente para documentación.

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí |
| Puede evolucionar independientemente | Técnicamente sí |
| Ciclo de versiones propio | Posible |
| Dependencias del kernel | Baja (referencia el código pero no depende de él) |

**Ventajas:**
- Contribuciones de documentación sin clonar el kernel
- CI específica (markdownlint, enlaces, spelling)
- GitHub Pages automático

**Inconvenientes:**
- La documentación técnica referencia líneas y funciones del código
- Riesgo alto de desincronización durante desarrollo activo
- Los PRs de API breaking necesitan PRs simultáneos en docs

**Veredicto: MANTENER EN NEODOS hasta v1.0.** Cuando la API esté estable y la documentación deje de cambiar frecuentemente, considerar migrar a `NeoDocs`.

---

### 3.6 NeoTest

**Propuesta:** Repositorio independiente para framework de pruebas.

**Evaluación:**
- Tests unitarios: deben vivir con el código (656 tests en kernel)
- Tests de integración: ya están en NeoDev (separado)
- check_deps.py: validación arquitectónica, debe vivir con el código
- Benchmarks: no existen aún (planeados en M3.2, v0.63)

**Veredicto: NO RECOMENDABLE SEPARAR.** Los tests unitarios y de integración deben vivir con el componente que prueban. NeoDev ya maneja los tests de integración a nivel de sistema.

---

### 3.7 NeoLive

**Propuesta:** Repositorio independiente para el Live Environment.

**Evaluación:** No existe un Live Environment como componente diferenciado. Actualmente NeoDOS arranca desde imágenes de disco generadas por NeoDev.

**Veredicto: NO RELEVANTE ACTUALMENTE.** Evaluar cuando exista un instalador (M2.4, v0.59) y un entorno live diferenciado.

---

### 3.8 NeoMCP

**Propuesta:** Repositorio independiente para el servidor MCP.

**Componente:** `scripts/mcp_server/` — servidor Python para integración AI.

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí |
| Puede evolucionar independientemente | Sí — Python puro |
| Ciclo de versiones propio | Sí |
| Reutilizable por otros proyectos | Potencialmente — MCP es un protocolo estándar |
| Dependencias del kernel | Ninguna |
| Publicable como Open Source independiente | Sí |

**Veredicto: MIGRAR INMEDIATAMENTE.** Servidor Python independiente que puede evolucionar a su propio ritmo. Propuesta: **NeoMCP**.

---

### 3.9 NeoDOS-LSP

**Propuesta:** Repositorio independiente para el servidor LSP.

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — editor integration |
| Puede evolucionar independientemente | Sí — Rust estándar con 15+ deps host |
| Ciclo de versiones propio | Sí |
| Reutilizable por otros proyectos | Parcialmente (contiene lógica NeoDOS-specific) |
| Dependencias del kernel | Ninguna |
| Publicable como Open Source independiente | Sí |

**Veredicto: MIGRAR INMEDIATAMENTE.** Proyecto completamente independiente con muchas dependencias host. Propuesta: **NeoDOS-LSP**.

---

### 3.10 NeoTranslations

**Propuesta:** Repositorio independiente para datos de localización y herramientas de traducción.

**Componentes:**
- `data/locale/` — archivos fuente TOML + NLT compilados
- `data/keyboard/` — layouts de teclado
- `tools/nltc/` — compilador NLT
- `tools/kbdcompile/` — compilador de teclados
- `scripts/gen_nlt*.py` — utilidades de generación

**Evaluación:**

| Criterio | Respuesta |
|----------|-----------|
| Responsabilidad claramente definida | Sí — i18n/L10n |
| Puede evolucionar independientemente | Sí |
| Ciclo de versiones propio | Sí |
| Reutilizable por otros proyectos | No — formato NLT es específico de NeoDOS |
| Dependencias del kernel | Baja — el runtime NLT está en libneodos, pero los datos son independientes |

**Ventajas:**
- Traductores pueden contribuir sin entorno de desarrollo Rust
- CI independiente (validar sintaxis TOML, compilar NLT, verificar cobertura)
- Las traducciones tienen cadencia distinta al kernel

**Inconvenientes:**
- Formato NLT aún en evolución (I18N-P7..P12 planean cambios)
- El runtime NLT está en libneodos (kernel)

**Veredicto: MIGRAR CUANDO EXISTA API ESTABLE (NLT congelado).** El formato NLT aún tiene cambios planeados (compresión, UTF-16, pluralización). Una vez congelado (post-v1.0), migrar a **NeoTranslations**.

---

## 4. Dependencias

### 4.1 Mapa de Dependencias Actual

```
neodos-kernel ─────────────────────────────────────────┐
    │                                                   │
    ├── (BootInfo ABI) ──► neodos-bootloader           │
    ├── (syscall ABI)  ──► libneodos ◄── userbin/*     │
    │                                    ◄── libnet    │
    │                                    ◄── libnet-nxl│
    ├── (NXL ABI) ──────► libneodos-nxl                │
    │                     libmath-nxl                  │
    │                     libconsole-nxl               │
    │                     libnet-nxl                   │
    ├── (NEM ABI) ──────► drivers/* (10)               │
    │                                                   │
    ├── (host tools, sin dependencias del kernel)       │
    │     ├── tools/nxeinfo                             │
    │     ├── tools/nxpkg                               │
    │     ├── tools/nxdump                              │
    │     ├── tools/nltc                                │
    │     ├── tools/kbdcompile                          │
    │     └── tools/nem-pack.py                         │
    │                                                   │
    ├── neodos-lsp (sin dependencias del kernel)        │
    │                                                   │
    ├── scripts/ (mix: build scripts atados al kernel)  │
    │     └── scripts/mcp_server/ (independiente)       │
    │                                                   │
    └── data/ (locale + keyboard, independiente)        │
```

### 4.2 Mapa de Dependencias Propuesto

```
NeoDOS (core)
├── neodos-kernel
├── neodos-bootloader
├── libneodos + libneodos-nxl + libnet + libnet-nxl
├── libmath-nxl + libconsole-nxl
├── userbin/* (34 binarios)
├── drivers/* (10 NEM)
├── scripts/check_deps.py
├── scripts/gen_system_hiv.py
├── scripts/setup-network.sh
├── skills/*
├── .opencode/
└── opencode.json

NeoDev (YA SEPARADO)
└── neodev (build, run, test, image, VMM)

NeoTools (MIGRAR INMEDIATAMENTE)
├── tools/nxeinfo
├── tools/nxpkg
└── tools/nxdump

NeoDOS-LSP (MIGRAR INMEDIATAMENTE)
└── neodos-lsp

NeoMCP (MIGRAR INMEDIATAMENTE)
└── scripts/mcp_server/

NeoDrivers (MIGRAR POST-V1.0)
└── drivers/*

NeoTranslations (MIGRAR POST-V1.0)
├── data/locale/*
├── data/keyboard/*
├── tools/nltc
├── tools/kbdcompile
└── scripts/gen_nlt*.py

NeoDocs (EVALUAR POST-V1.0)
└── docs/*
```

**No hay dependencias circulares** en la estructura propuesta. Las dependencias son unidireccionales:
- NeoTools/NeoDOS-LSP/NeoMCP no dependen de NeoDOS
- NeoDrivers dependen de la NEM ABI (versión congelada, artefacto publicado)
- NeoTranslations es independiente (consumido por NeoDOS en tiempo de build)

---

## 5. APIs Públicas Necesarias

### 5.1 APIs que ya existen

| API | Estado | Uso |
|-----|--------|-----|
| Syscall ABI (SSDT, RAX 0-59) | Versionada (v8), no congelada | libneodos → kernel |
| NEM ABI (host services table) | Versionada (v8), no congelada | drivers → kernel |
| NXE format | Documentado, estable | userbin, tools |
| NXP format | Documentado, estable | tools/nxpkg |
| NLT format | Documentado, en evolución | data/locale, tools/nltc |

### 5.2 APIs que deberían crearse

| API Propuesta | Propósito | Para | Prioridad |
|---------------|-----------|------|-----------|
| `kernel ABI manifest` | Archivo JSON/YAML declarando versiones de ABI, syscalls, ObInfoClass, etc. | Todas las herramientas externas | Alta |
| `NEM driver manifest` | Metadatos del driver (versión ABI requerida, capacidades, hardware soportado) | NeoDrivers | Alta |
| `Syscall ABI crate` | Publicar `libneodos` como crate en crates.io para desarrollo de aplicaciones | NeoSDK futuro | Media (post-v1.0) |

### 5.3 Acceso directo a estructuras internas detectado

| Componente | Acceso directo | Propuesta |
|------------|---------------|-----------|
| `userbin/*` | Llama syscalls directamente (no vía trait) | Crear wrapper trait en libneodos para tests |
| `libnet` | Usa libneodos internals (syscall numbers) | Ya está abstraído vía wrappers |
| `drivers/*` | Usa tabla `hst_*` con índices fijos | Documentar como ABI pública versionada |

---

## 6. Organización Propuesta

```
NeoDOS-Project/ (GitHub Organization)
│
├── NeoDOS (core OS)
│   ├── neodos-kernel/         # Kernel (52,945 líneas)
│   ├── neodos-bootloader/     # UEFI bootloader
│   ├── libneodos/             # Syscall wrappers
│   ├── libneodos-nxl/         # Core NXL DLL
│   ├── libmath-nxl/           # Math NXL DLL
│   ├── libconsole-nxl/        # Console NXL DLL
│   ├── libnet-nxl/            # Network NXL DLL
│   ├── libnet/                # Network wrapper
│   ├── userbin/               # 34 user-mode .NXE binaries
│   ├── drivers/               # 10 NEM drivers (hasta post-v1.0)
│   ├── scripts/               # Build scripts + validación
│   ├── tools/                 # (solo lo que no se migre)
│   ├── data/                  # (hasta post-v1.0)
│   ├── docs/                  # (hasta post-v1.0)
│   ├── skills/                # AI skills
│   └── .opencode/             # AI config
│
├── NeoDev (YA SEPARADO)
│   └── Build/run/test/image toolchain
│
├── NeoTools (MIGRAR INMEDIATAMENTE)
│   ├── nxeinfo/               # NXE binary inspector
│   ├── nxpkg/                 # NXP package tool
│   └── nxdump/                # ELF/NXE/NEM dumper
│
├── NeoDOS-LSP (MIGRAR INMEDIATAMENTE)
│   └── LSP server for NeoDOS development
│
├── NeoMCP (MIGRAR INMEDIATAMENTE)
│   └── Python MCP server
│
├── NeoDrivers (MIGRAR POST-V1.0)
│   └── Standalone NEM drivers
│
└── NeoTranslations (MIGRAR POST-V1.0)
    ├── data/locale/
    ├── data/keyboard/
    ├── tools/nltc/
    ├── tools/kbdcompile/
    └── scripts/gen_nlt*.py
```

---

## 7. Plan de Migración

### 7.1 Migrar Inmediatamente

| Repositorio | Prioridad | Complejidad | Dependencias | Beneficios | Riesgos |
|-------------|-----------|-------------|--------------|------------|---------|
| **NeoTools** | ALTA | Baja | Ninguna | CI rápida, releases independientes, contribuciones focales | Bajo — herramientas standalone |
| **NeoDOS-LSP** | ALTA | Baja | Ninguna | CI independiente, feature development desacoplado | Bajo — sin deps del kernel |
| **NeoMCP** | MEDIA | Baja | Ninguna | Separación clara AI/kernel | Bajo — Python independiente |

**Orden recomendado:**
1. NeoTools (impacto inmediato, cero riesgo)
2. NeoDOS-LSP (independencia total, dependencies host)
3. NeoMCP (Python, ciclo diferente)

### 7.2 Migrar Cuando Exista API Estable (post-v1.0)

| Repositorio | Prioridad | Complejidad | Dependencias | Beneficios | Riesgos |
|-------------|-----------|-------------|--------------|------------|---------|
| **NeoDrivers** | ALTA | Media | NEM ABI congelada v1.0 | Community drivers, releases independientes | Medio — compatibilidad ABI, pipeline build |
| **NeoTranslations** | MEDIA | Media | NLT format congelado | Contribuciones comunitarias sin Rust | Medio — sincronización con runtime NLT |

**Orden recomendado:**
1. NeoDrivers (mayor impacto, drivers de hardware estándar)
2. NeoTranslations (comunidad, traducciones)

### 7.3 Evaluar Después de v1.0

| Repositorio | Razón para esperar |
|-------------|-------------------|
| **NeoDocs** | Documentación referencias código activo. Esperar a que la API se estabilice. |
| **NeoSDK** | No hay aplicaciones de terceros. Esperar a que exista demanda. |
| **NeoLive** | No existe como componente diferenciado. Evaluar cuando exista instalador. |

### 7.4 Nunca Separar

| Componente | Razón |
|-------------|-------|
| `neodos-kernel` | Es el proyecto principal |
| `neodos-bootloader` | BootInfo ABI atado al kernel |
| `libneodos` + `libneodos-nxl` | Syscall/NXL ABI en lockstep |
| `libnet` + `libnet-nxl` | Dependen de libneodos + kernel net |
| `libmath-nxl` + `libconsole-nxl` | Cargados por kernel NXL loader |
| `userbin/*` | Todos dependen de libneodos |
| `scripts/check_deps.py` | Validación arquitectónica del kernel |
| `scripts/gen_system_hiv.py` | Conoce estructura interna del Registry |
| `skills/*` | Atados a AGENTS.md |
| `.opencode/` + `opencode.json` | Configuración del proyecto |

---

## 8. Tabla de Clasificación Completa

| Componente | Categoría | Justificación |
|------------|-----------|---------------|
| `neodos-kernel/` | Mantener en NeoDOS | Core OS, centro de todas las dependencias |
| `neodos-bootloader/` | Mantener en NeoDOS | BootInfo ABI atado al kernel |
| `neodos-lsp/` | **Migrar inmediatamente** | Standalone, 15+ deps host, sin deps del kernel |
| `libneodos/` | Mantener en NeoDOS | Syscall ABI lockstep con kernel |
| `libneodos-nxl/` | Mantener en NeoDOS | Cargado por kernel NXL loader |
| `libmath-nxl/` | Mantener en NeoDOS | Cargado por kernel a dirección fija |
| `libconsole-nxl/` | Mantener en NeoDOS | Cargado por kernel a dirección fija |
| `libnet-nxl/` | Mantener en NeoDOS | Atado a kernel networking ABI |
| `libnet/` | Mantener en NeoDOS | Depende de libneodos + libnet-nxl |
| `userbin/` (34 binarios) | Mantener en NeoDOS | Todos dependen de libneodos |
| `drivers/` (10 NEM) | **Migrar post-v1.0** | NEM ABI versionada pero no congelada |
| `tools/nxeinfo` | **Migrar inmediatamente** | Standalone, serde_json only |
| `tools/nxpkg` | **Migrar inmediatamente** | Standalone, sin deps |
| `tools/nxdump` | **Migrar inmediatamente** | Standalone, sin deps |
| `tools/nltc` | **Migrar post-v1.0** | Atado a formato NLT (en evolución) |
| `tools/kbdcompile` | **Migrar post-v1.0** | Atado a formato KBD |
| `tools/nem-pack.py` | Mantener en NeoDOS | Parte del pipeline de build de drivers |
| `scripts/check_deps.py` | Mantener en NeoDOS | Validación arquitectónica del kernel |
| `scripts/crash_analyzer.py` | Mantener en NeoDOS | Conoce formato interno crash dump |
| `scripts/gen_nlt*.py` | **Migrar post-v1.0** | Atado a NLT, migrar con NeoTranslations |
| `scripts/gen_system_hiv.py` | Mantener en NeoDOS | Conoce estructura del Registry |
| `scripts/mcp_server/` | **Migrar inmediatamente** | Python independiente |
| `scripts/setup-network.sh` | Mantener en NeoDOS | Configuración de desarrollo |
| `data/locale/` | **Migrar post-v1.0** | Datos de traducción, comunitario |
| `data/keyboard/` | **Migrar post-v1.0** | Layouts de teclado |
| `docs/` | Mantener en NeoDOS (reevaluar post-v1.0) | Referencias cruzadas con código activo |
| `skills/` | Mantener en NeoDOS | Atados a AGENTS.md |
| `.opencode/` | Mantener en NeoDOS | Configuración del proyecto |
| `opencode.json` | Mantener en NeoDOS | Configuración del proyecto |
| `preferences/` | Mantener en NeoDOS | Configuración de pruebas |
| Imágenes de disco | Mantener en NeoDOS (.gitignore) | Artefactos de build |

---

## 9. Improvements Detectados

Los siguientes hallazgos deben trackearse como GitHub Issues:

| ID | Descripción | Prioridad | Complejidad | Impacto Arquitectónico |
|----|-------------|-----------|-------------|----------------------|
| REPO-SEP-008 | Extraer NeoTools (nxeinfo, nxpkg, nxdump) a repositorio independiente | ALTA | BAJA | Medio — elimina dependencias de host del tree principal |
| REPO-SEP-009 | Extraer NeoDOS-LSP a repositorio independiente | ALTA | BAJA | Medio — separa herramienta de desarrollo del SO |
| REPO-SEP-010 | Extraer NeoMCP (scripts/mcp_server/) a repositorio independiente | MEDIA | BAJA | Bajo — Python independiente |
| REPO-SEP-011 | Definir NEM ABI manifest (YAML/JSON) para publicación de drivers | MEDIA | MEDIA | Alto — permite separación futura de NeoDrivers |
| REPO-SEP-012 | Crear kernel ABI manifest versionado para herramientas externas | MEDIA | MEDIA | Alto — necesario para cualquier separación futura |
| REPO-SEP-013 | Publicar libneodos como crate independiente en crates.io (post-v1.0) | BAJA | MEDIA | Alto — base para NeoSDK |
| REPO-SEP-014 | Migrar drivers/ a CI independiente dentro del monorepo | MEDIA | BAJA | Medio — prepara separación futura |
| REPO-SEP-015 | Congelar formato NLT para permitir separación de NeoTranslations | MEDIA | ALTA | Alto — necesario para comunidad de traductores |

---

## 10. Resumen de Decisiones

```
MIGRAR INMEDIATAMENTE (3):
  ✔ NeoTools (nxeinfo, nxpkg, nxdump)
  ✔ NeoDOS-LSP
  ✔ NeoMCP (scripts/mcp_server/)

MIGRAR POST-V1.0 (API ESTABLE) (3):
  ⏳ NeoDrivers (drivers/*)
  ⏳ NeoTranslations (data/locale/ + nltc + kbdcompile)
  ⏳ NeoDocs (docs/* — reevaluar)

MANTENER EN NEODOS (25+):
  📦 neodos-kernel
  📦 neodos-bootloader
  📦 libneodos + libneodos-nxl
  📦 libmath-nxl + libconsole-nxl
  📦 libnet + libnet-nxl
  📦 userbin/* (34)
  📦 drivers/* (10 — temporal)
  📦 tools/nem-pack.py
  📦 scripts/* (excepto mcp_server/)
  📦 data/* (temporal)
  📦 docs/* (temporal)
  📦 skills/* + .opencode/

NO RECOMENDABLE SEPARAR:
  ❌ Tests unitarios (deben vivir con el código)
  ❌ NeoTest como repositorio

YA SEPARADO:
  ✅ NeoDev (github.com/NeoDOS-Project/NeoDev)
```

---

*Este documento es el resultado de una auditoría arquitectónica completa. No implica cambios en la estructura del repositorio. Las migraciones aquí descritas deben planificarse como tareas en GitHub Issues antes de ejecutarse.*
