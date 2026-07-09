# NeoDOS v0.49 — Handoff Session

> **Último commit:** `7950064` — "fix: freelist init from num_used, add coredir/dir.nxe, partition offset fixes"
> **Tests:** 677 pass, 0 fail

## Estado actual

NeoFS v2 (NE2) arranca, monta, carga NXL (`fs.nxl` OK), inicializa red y Registry.
**677 tests pasan, 0 fallan.** Todos los tests de VFS (crear, mkdir, readdir, unlink, rename) funcionan.

### Bugs conocidos sin resolver

1. ~~**5 tests fallan**~~ — **CORREGIDO** (sesión jul 09):
   - `spawn_hello_binary_path_resolve` — cambiado a `coredir.nxe`
   - `readdir_list_root` — `readdir` ahora devuelve inode válido (via `cache()`)
   - `mkdir_rmdir_roundtrip` — `update_inode_root()` tras mutación del B-tree
   - `unlink_file` — `update_inode_root()` en `create()` / `remove_file()`
   - `rename_file` — `update_inode_root()` en `rename()`

2. ~~**Debug prints activos**~~ — **CORREGIDO**: eliminados de `lookup()` en `neodos_v2.rs`

3. **Freelist no persistente**: `sb.freelist_lba = 0`, el kernel reconstruye freelist desde `sb.num_used`. 
   Cuando se escriben nuevos bloques, la freelist se modifica en RAM pero nunca se persiste.
   Al reiniciar, los bloques escritos se pierden. Solución: persistir freelist.

4. **file_read usa page cache con LBA absoluto**: en `read()`, se traduce `extent_lba * 8` a sector absoluto,
   pero la page cache usa `dev.read_sector(lba)` que espera LBA absoluto. Esto funciona pero es frágil.
   La traducción del offset de partición debería ser más limpia.

5. **Sin COW funcional**: Las escrituras (`write()`) escriben nuevos nodos B-tree pero la freelist 
   asigna bloques incorrectamente. No probado.

6. **Sin soporte de snapshots**: `SnapshotTable` existe en RAM pero no se persiste.
   `sys_ob_snapshot` (RAX 77) no implementado.

## Archivos clave

| Archivo | Descripción |
|---------|-------------|
| `src/fs/neodos_v2.rs` | FS v2: FileSystem trait + BTreeIO impl |
| `src/fs/btree.rs` | B-tree genérico con trait BTreeIO |
| `src/fs/freelist.rs` | Free list de regiones |
| `src/fs/snapshot.rs` | Snapshots (64 circulares) |
| `src/fs/neodos_dir.rs` | DirEntryV2 (128 bytes) + helpers |
| `src/fs/neodos_io.rs` | file_read / file_write con extents |
| `src/fs/mod.rs` | Todos los módulos registrados |
| `scripts/create_ne2_image.py` | Genera imagen NE2 con todos los ficheros |
| `docs/neofs_v2_design.md` | Diseño completo del FS |

## Cambios aplicados (jul 09)

1. **`cache()` dedup**: Si una entrada de directorio ya está en `inode_cache` (mismo `extent_lba` y `name`), se reutiliza su índice en vez de crear uno nuevo.
2. **`update_inode_root()`**: Nueva función helper que actualiza `inode_cache[inode].0 = new_root` tras mutaciones.
3. **`create()`, `mkdir()`, `remove_file()`, `remove_dir()`, `rename()`**: Ahora llaman `update_inode_root(dir_inode, new_root)` y usan `dir_inode == 0` (no comparación de LBAs) para decidir si actualizar `sb.root_btree_lba`.
4. **`write()`**: Corregido — ahora guarda `(new_root, new_entry)` en lugar de `(btree_root, new_entry)`.
5. **`lookup()`**: Debug prints eliminados.
6. **`readdir()`**: Ahora llama `cache()` para asignar inode válido a cada entrada (antes devolvía `inode: 0` fijo).
7. **Test `spawn_hello_binary_path_resolve`**: Cambiado a `coredir.nxe`.
8. **COW extent writes**: Corregido `file_write`/`file_read` en `neodos_io.rs` — el freelist devuelve números de bloque (4096-byte), pero la page cache espera LBAs de sector (512-byte). Añadido `partition_base_sector` para traducir LBAs relativos a absolutos.
9. **Tests COW**: Añadidos `cow_inline_write_read` y `cow_extent_write_read` en `syscall/tests.rs`.

## Proximo paso recomendado

1. Implementar `sys_ob_snapshot` (RAX 77)
2. Persistir freelist en disco
3. Eliminar `#![allow(dead_code)]` y warnings de `neodos_v2.rs`
4. Fix `NeoInit.nxe` case mismatch en `main.rs` (busca `neoinit.nxe`, no `NeoInit.nxe`)

## Referencia rápida

```bash
cd neodos/
bash scripts/build.sh --neodos-image           # compilar + imagen
timeout 300 python3 scripts/auto_test.py        # tests
Syscalls: RAX 60-66 = Ob API, RAX 77 = snapshot (no implementado)
ObType: 18 = Socket
Magic NE2: 0x0032454E
```
