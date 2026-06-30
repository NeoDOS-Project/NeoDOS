# NeoFS vNext — Roadmap

> Roadmap priorizado para la evolución del sistema de archivos NeoDOS (NeoFS)
> y la interacción Driver/Namespace.
>
> Basado en la auditoría completa en [NEOFS_AUDIT.md](NEOFS_AUDIT.md).

---

## Fase 1 — Estabilidad (v0.48)

Objetivo: eliminar límites artificiales y prevenir corrupción básica.

### FS-1.1: Dynamic inode allocator
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 2-3 días

- [ ] Reemplazar `InodeCache.inodes: [Option<Inode>; 256]` por `Vec<Option<Inode>>`
- [ ] Actualizar `find_free_inode()` para trabajar con Vec dinámico
- [ ] Actualizar `load_inode()` para extender el Vec si es necesario
- [ ] Actualizar `write_inode()` para manejar índices > 256
- [ ] Modificar superblock: `num_inodes` ahora es dinámico en lugar de fijo en 256
- [ ] Migrar sector offset calculation: `inode_sector = 1 + (inode_num / 2)` sigue
      funcionando si el Vec se carga bajo demanda

**Criterio:** Crear 300+ archivos en el FS. `find_free_inode()` no falla por
límite de tabla. Todos los tests existentes pasan.

### FS-1.2: Dynamic block bitmap
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 2-3 días

- [ ] Reemplazar `BlockBitmap.bits: [u8; 320]` por `Vec<u8>` con tamaño = `num_blocks / 8`
- [ ] Actualizar `alloc()`, `free()`, `mark_used()` para Vec dinámico
- [ ] Al montar: leer `num_blocks` del superblock, dimensionar bitmap
- [ ] Actualizar `rebuild_bitmap()` para recorrer todos los inodos existentes

**Criterio:** FS con 10000 bloques (~40 MB) monta correctamente. Bitmap
dimensionado automáticamente.

### FS-1.3: Eliminar hardcoded sector offsets
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 1 día

- [ ] Definir constantes o cálculos centralizados:
  - `INODE_TABLE_SECTORS = (num_inodes * 256 + 511) / 512`
  - `DATA_START_SECTOR = 1 + INODE_TABLE_SECTORS`
- [ ] Reemplazar `200` literal con `DATA_START_SECTOR`
- [ ] Actualizar `create_neodos_image.py` para usar la misma fórmula

**Criterio:** Cambiar `BLOCK_SIZE` o `num_inodes` recalcula automáticamente
DATA_START_SECTOR. Script Python sincronizado.

### NS-1.1: Namespace ownership tracking
**Archivos:** `src/object/namespace.rs`, `src/object/mod.rs`
**Esfuerzo:** 3-4 días

- [ ] Añadir `creator: Option<(ObType, u64)>` a `NamespaceEntry` (objeto) y
      `DirectoryObject` (directorio)
- [ ] `ob_insert_object()` registra el creator del caller actual
- [ ] `ob_remove_object()` verifica que el caller es el creator o admin
- [ ] Añadir `is_admin()` helper (usa el token del EPROCESS actual)
- [ ] Añadir `ob_force_remove_object()` para admin (NDREG, hot reload)

**Criterio:** Un driver no puede borrar entries de otro driver. Admin puede.
Tests de permisos.

### NS-1.2: Proteger directorios raíz del namespace
**Archivos:** `src/object/namespace.rs`
**Esfuerzo:** 1-2 días

- [ ] Marcar `\Device`, `\Global`, `\Driver`, `\FileSystem`, `\Ob`,
      `\Registry`, `\Process`, `\DosDevices` como `protected: bool`
- [ ] `ob_insert_object()` y `ob_create_directory()` deniegan crear entries
      directas bajo root directories si no hay `CAP_ADMIN` o el entry es
      del tipo esperado
- [ ] Añadir `ob_is_protected_path()` helper

**Criterio:** Shell no puede `ob_create(Directory, "\Device\Foo")` como
usuario normal. Admin sí.

---

## Fase 2 — Robustez (v0.49)

Objetivo: integridad de datos, journaling, recovery.

### FS-2.1: Indirect block support (>48 KB files)
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 1-2 días

- [ ] Implementar `inode_data_block_count()` que considere `indirect_block`
- [ ] Implementar `get_inode_block_ptr()` con indirect block traversal:
      single indirect → 1024 bloques → ~4 MB
- [ ] Implementar `allocate_indirect_block()` para archivos > 12 bloques
- [ ] Actualizar `read_file_to_buf()` para soportar > 12 bloques

**Criterio:** Archivo de 1 MB se escribe y lee correctamente.

### FS-2.2: Basic journaling (write-ahead log)
**Archivos:** `src/fs/journal.rs` (nuevo)
**Esfuerzo:** 1 semana

- [ ] Definir estructura de journal: `JournalEntry { operation, sector, data, checksum }`
- [ ] Reservar sectores de journal al formatear (2-4 MB al inicio del FS)
- [ ] `begin_transaction()` → escribe entries al journal
- [ ] `commit_transaction()` → marca como completado, escribe datos reales
- [ ] `recover_journal()` → replay al montar si transacción sin completar
- [ ] Operaciones protegidas: create_file, write_file, delete_file, mkdir, rmdir

**Criterio:** Crash durante `create_file()` → al remontar, recovery replay
completa o deshace la operación. FS consistente tras crash simulado.

### FS-2.3: Metadata checksums
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 2-3 días

- [ ] Superblock: usar `reserved[0..4]` para checksum CRC32 del superblock
- [ ] Inodo: añadir campo `checksum: u32` (CRC32 de `inode_num..padding`)
- [ ] Directory entry: usar byte `attributes` para checksum simple o extender struct
- [ ] Verificar checksums al leer, recalcular al escribir
- [ ] Superblock corrupto → mensaje de error, no panic

**Criterio:** Modificar un byte del superblock en disco → FS detecta error
al montar. Tests de corrupción inducida.

### NS-2.1: Extender ResourceRegistry para Ob entries
**Archivos:** `src/drivers/hotreload.rs`
**Esfuerzo:** 1 día

- [ ] Añadir `ResourceType::ObNamespace` = 2
- [ ] `hotreload_track_ob_entry(driver_id, path)` registra entry
- [ ] Al hot-unload (`hot_unload_driver()`): recorrer recursos ObNamespace
      y llamar `ob_remove_object()` para cada entry
- [ ] Al hot-unload: destruir ObObject del driver si existe

**Criterio:** Driver registra `\Device\Foo`, se descarga → entry eliminada
del namespace. ObObject destruido.

---

## Fase 3 — Características (v0.50)

Objetivo: features de FS moderno.

### FS-3.1: Extended attributes
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 2-3 días

- [ ] Añadir `extended_attrs_block: u32` al inodo (apunta a bloque con
      pares clave-valor)
- [ ] Implementar `get_attr(inode, name)` y `set_attr(inode, name, value)`
- [ ] Atributos del sistema: `creation_time`, `last_access`, `owner`,
      `hidden`, `compressed`, `encrypted`

### FS-3.2: Hard links y symlinks en NeoFS
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 2-3 días

- [ ] Hard links: incrementar `link_count` en inodo, múltiples directory
      entries apuntando al mismo inodo
- [ ] Symlinks: archivo especial cuyo contenido es el target path
- [ ] Implementar `link()`, `unlink()`, `symlink()`, `readlink()`

### FS-3.3: Sparse files
**Archivos:** `src/fs/neodos_fs.rs`
**Esfuerzo:** 1-2 días

- [ ] Block pointer = 0 = "hole" (leer como ceros, no ocupa disco)
- [ `get_inode_block_ptr()` retorna `None` para holes
- [ ] `write_file()` con saltos puede crear holes

---

## Fase 4 — Rendimiento (v0.51+)

Objetivo: velocidad y escalabilidad.

### FS-4.1: Buffer cache with LRU
**Archivos:** `src/buffer/` (ya existe page cache, mejorar)
**Esfuerzo:** 2-3 días

- [ ] Page cache existente: extender con límite de memoria configurable
- [ ] LRU eviction policy
- [ ] Dirty page writeback asíncrono via work queue
- [ ] Hit/miss stats exportados via `\Global\Info\Cache`

### FS-4.2: Read-ahead
**Archivos:** `src/fs/neodos_fs.rs`, `src/vfs/io.rs`
**Esfuerzo:** 2-3 días

- [ ] Detectar acceso secuencial (offset siempre creciente)
- [ ] Pre-cargar N bloques siguientes en page cache
- [ ] Configurable: `read_ahead_pages` (default 4)

### FS-4.3: Async I/O via IRP
**Archivos:** `src/vfs/io.rs`, `src/irp/mod.rs`
**Esfuerzo:** 3-4 días

- [ ] `iostack_read_sectors_async()` → devuelve `IrpId`
- [ ] Completar IRP via callback o wake blocked thread
- [ ] Integrar con `sys_ioctl` / `sys_ob_wait` para I/O completion

---

## Resumen de Fases

| Fase | Versión | Items | Esfuerzo total |
|------|---------|-------|----------------|
| 1 — Estabilidad | v0.48 | 5 items | ~10 días |
| 2 — Robustez | v0.49 | 4 items | ~12 días |
| 3 — Características | v0.50 | 3 items | ~7 días |
| 4 — Rendimiento | v0.51+ | 3 items | ~8 días |

**Principio:** No pasar a la siguiente fase hasta que todos los items de la
fase actual estén completados y validados con `auto_test.py`.

---
