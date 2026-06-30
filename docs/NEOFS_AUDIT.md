# NeoFS Audit Report — v0.47

> Auditoría completa del sistema de archivos NeoDOS (NeoFS) y su interacción
> con el Object Manager (Ob), el Driver Runtime, y los drivers (especialmente
> e1000). Fecha: 2026-06-28.

---

## 1. Resumen Ejecutivo

| Área | Hallazgos | Severidad |
|------|-----------|-----------|
| NeoFS — Inode allocator | Fijo en 256 inodos, sin crecimiento dinámico | **ALTA** |
| NeoFS — Block bitmap | 320 bytes → 2560 bloques máx (~10 MB) | **ALTA** |
| NeoFS — Tamaño archivo | 12 bloques directos → 48 KB máx por archivo | **ALTA** |
| NeoFS — Hardcoded offsets | Sector 200 como base de datos, offsets duros | **MEDIA** |
| NeoFS — Sin journaling | Sin recovery tras crash | **MEDIA** |
| NeoFS — Sin checksums | Metadata sin CRC | **MEDIA** |
| Driver/NS — Ownership | El namespace no trackea creador de entries | **CRÍTICA** |
| Driver/NS — Protección | Directorios raíz no protegidos contra escritura | **CRÍTICA** |
| Driver/NS — Hot reload | ResourceRegistry no trackea entries Ob | **ALTA** |
| Driver/NS — e1000 | No es NEM, no hot-reload, no cleanup en fallo | **MEDIA** |
| Driver/NS — Namespace dup | Sin verificación de nombres reservados en VFS | **MEDIA** |
| Driver/NS — Capabilities | Sin CAP_NS_WRITE para operaciones de namespace | **BAJA** |
| Driver/NS — NIC names | nic_register sin deduplicación de nombres | **BAJA** |

---

## 2. Fase 6 — Driver / Namespace Interaction Audit

### 2.1 e1000.nem — Análisis detallado

**No es un NEM driver:** El código e1000 está compilado en el kernel como
módulo estático (`src/net/e1000.rs`), NO como un `.NEM` standalone. No pasa
por el pipeline de certificación (Loaded → Initialized → Registered → Bound
→ Active), no tiene DriverState, no es hot-reloadable.

**Registro de recursos:**
- `probe_e1000()` busca PCI → crea `E1000Nic` → llama `nic_register(nic)`
- `nic_register()` guarda en `NIC_REGISTRY` (un `Mutex<NicRegistry>`), un
  registro separado del Ob namespace
- El NIC no crea entradas en el Ob namespace. Quien las crea es
  `init_networking()` en `net/mod.rs`: crea `\Device\Tcp`, `\Device\Udp`,
  `\Device\Nic` como objetos Ob
- Si `probe_e1000()` falla (no hay NIC), los objetos `\Device\Tcp` y
  `\Device\Udp` quedan colgados en el namespace sin utilidad

**Hot reload:**
- No hay `DRIVER_UNLOAD` para e1000. No hay `driver_fini()`.
- Los recursos NIC no se liberan en ningún shutdown path.
- `nic_unregister()` existe pero nunca se llama desde el código actual.

**Namespace entries creadas por init_networking():**
```
\Device\Tcp        → ObType::Device, native_id=1
\Device\Udp        → ObType::Device, native_id=2
\Device\Nic        → Directory (vacío si no hay NIC)
```

### 2.2 Problemas de Ownership en el Namespace

El Ob namespace (`object/namespace.rs`) no trackea qué driver o proceso creó
cada entrada. Cualquier código con acceso al namespace puede:
- Llamar `ob_insert_object()` para crear entries en `\Device\`,
  `\Global\`, `\Driver\`, etc.
- Llamar `ob_remove_object()` para borrar entries existentes
- Llamar `ob_create_directory()` para crear subdirectorios en cualquier
  parte del árbol

**No hay verificación de permisos por tipo de objeto.** Un driver con
`CAP_BLOCK_DEVICE` podría crear entries en `\Registry\` sin tener permiso
para ello.

### 2.3 Nombres Reservados y Conflictos

El namespace no define una lista de nombres protegidos. Los directorios raíz
se crean en `init_object_namespace()`:

```rust
let root_dirs = ["Device", "DosDevices", "Global", "Driver", "FileSystem",
                  "Ob", "Registry", "Process"];
```

Un bug o driver malicioso podría:
- Sobrescribir `\Ob\Process\1` con un objeto de otro tipo
- Crear entries dentro de `\Global\Info\` (aunque éstas se crean en main.rs)
- Registrar un driver con nombre que colisione con otro existente

**Protección existente:** `ob_insert_object()` retorna `OB_ALREADY_EXISTS`
si ya hay un objeto en la misma ruta. Pero un driver podría llamar primero
`ob_remove_object()` y luego `ob_insert_object()` para reemplazar.

### 2.4 Resource Registry Gap

El `ResourceRegistry` en `hotreload.rs` trackea:
- `ResourceType::BlockDevice`
- `ResourceType::NetworkDevice`

Pero NO trackea:
- Ob namespace entries creadas por drivers
- EventBus handlers registrados por drivers
- ObObjects creados por drivers

Cuando un driver se descarga, sus Ob objects en el namespace no se limpian
automáticamente. El `remove()` en `driver_runtime.rs` solo destruye el
ObObject del driver en la tabla Ob, pero no remueve su entry del namespace.

### 2.5 Recomendaciones para Fase 6

1. **Añadir ownership tracking al namespace** — cada `NamespaceEntry` guarda
   `creator_id: Option<(ObType, u64)>` (driver_id o pid)
2. **Proteger directorios raíz** — entries en `\Device\`, `\Global\`,
   `\Driver\`, `\Registry\` solo modificables por creador o admin
3. **Añadir CAP_NS_WRITE** — capability para operaciones de namespace
4. **Extender ResourceRegistry** — trackear Ob namespace entries por driver
5. **Migrar e1000 a NEM driver** o al menos añadir init/shutdown simétrico
6. **Añadir validación de tipo** al insertar — si la ruta es `\Device\*`,
   el objeto debe ser `ObType::Device` o `ObType::BlockDevice`

---

## 3. NeoFS — Problemas Identificados

### 3.1 Inode Allocator Fijo (P1 — CRÍTICO)

**Archivo:** `src/fs/neodos_fs.rs:136-138`
```rust
pub struct InodeCache {
    pub(crate) inodes: [Option<Inode>; 256],  // ← fijo en 256
}
```

**Problema:** El sistema no puede crecer más allá de 256 inodos. Esto limita
el número total de archivos+directorios a 256. Con `System/`, `Programs/`,
`Users/`, esto se agota rápidamente.

**find_free_inode()** (línea 614-623) escanea linealmente de 1..255. Esto es
O(n) y no escala.

### 3.2 Block Bitmap Fijo (P2 — CRÍTICO)

**Archivo:** `src/fs/neodos_fs.rs:29-31`
```rust
pub struct BlockBitmap {
    bits: [u8; 320],  // ← 320 bytes = 2560 bits = 2560 bloques max
}
```

**Problema:** 2560 bloques de 4096 bytes = ~10 MB de datos máximo. El
superblock declara `num_blocks` pero el bitmap no puede representar más de
2560 bloques. Si `num_blocks > 2560`, el allocator falla silenciosamente.

### 3.3 Tamaño Máximo de Archivo (P3 — ALTA)

Cada inode tiene `direct_blocks: [u32; 12]`. El campo `indirect_block`
existe pero `inode_data_block_count()` (línea 258-270) solo cuenta los 12
directos. **Max file size = 12 × 4096 = 48 KB.**

### 3.4 Hardcoded Sector Offsets (P4 — MEDIA)

En múltiples lugares el código asume:
```rust
let block_sector = 200 + (current_block * 8);
```

Esto significa:
- Sector 0 = superblock
- Sectors 1-128 = inode table (512 bytes × 128 = 256 inodos × 256 bytes)
- Sectors 128-199 = gap (no usado)
- Sector 200+ = data blocks

Si se cambia `num_inodes` o `BLOCK_SIZE`, estos offsets dejan de ser
válidos. No hay una constante centralizada que calcule `DATA_START_SECTOR`.

### 3.5 Sin Journaling ni Recovery (P5 — ALTA)

No hay write-ahead log. Una operación de escritura multi-sector (crear
archivo: bitmap + inode + directory entry = 3 sectores) no es atómica.
Un crash entre escrituras deja el FS inconsistente.

### 3.6 Sin Checksums en Metadata (P6 — MEDIA)

El superblock tiene `reserved: [u8; 472]` sin checksum. Los inodos y
directory entries no tienen CRC. Corrupción de un solo byte en el superblock
puede hacer el FS ilegible sin posibilidad de detección.

### 3.7 Sin Name Reservation (P7 — BAJA)

No hay verificación de nombres reservados del sistema (CON, PRN, AUX, NUL,
etc.) ni de caracteres inválidos en nombres de archivo.

### 3.8 Hardcoded IDs en create_neodos_image.py

El script que genera la imagen FS asume la misma estructura de sectores.
Si se cambian las constantes en Rust, el script Python debe actualizarse
manualmente — no hay fuente única de verdad.

---

## 4. Priorización

| ID | Problema | Impacto | Esfuerzo | Prioridad |
|----|----------|---------|----------|-----------|
| NS-1 | Namespace ownership tracking | Data corruption | 3-4 días | **P0** |
| NS-2 | Proteger directorios raíz | Seguridad | 1-2 días | **P0** |
| NS-3 | Extender ResourceRegistry | Hot reload corrupto | 1 día | **P1** |
| FS-1 | Inode allocator dinámico | Límite 256 archivos | 2-3 días | **P1** |
| FS-2 | Block bitmap dinámico | Límite 10 MB | 2-3 días | **P1** |
| FS-3 | Indirect blocks (+48 KB) | Archivos grandes | 1-2 días | **P1** |
| FS-4 | Hardcoded offsets → constantes | Mantenibilidad | 1 día | **P2** |
| FS-5 | Journaling básico | Crash recovery | 1 semana | **P2** |
| FS-6 | Checksums en metadata | Integridad | 2-3 días | **P2** |
| NS-4 | CAP_NS_WRITE capability | Seguridad defensiva | 1 día | **P3** |
| NS-5 | e1000 shutdown/cleanup path | Limpieza recursos | 1 día | **P3** |
| FS-7 | Name reservation (DOS names) | Compatibilidad | 4 horas | **P3** |
| NS-6 | NIC name deduplication | Consistencia | 2 horas | **P4** |

---
