# NeoFS v2 — Diseño

> **Formato:** `"NE2\0"` — limpio, sin retrocompatibilidad.
> **Filosofía:** Un FS para el usuario de a pie. Rápido, fiable, transparente.

---

## 1. Experiencia de Usuario (cómo se siente)

| Operación | Experiencia |
| ----------- | ------------- |
| `DIR C:\` | Instantáneo, aunque haya 10000 archivos |
| `COPY` de 500MB | Velocidad constante, sin fragmentación por extents |
| `DEL C:\Proyecto\*.*` | Instantáneo — solo borra una entrada del árbol |
| `RD C:\Proyecto` | Borra todo el subárbol aunque tenga archivos |
| `TYPE` archivo corrupto | `ERROR: INFORME.TXT daño en LBA 40824. COPY para regenerar.` |
| Corte de luz | Al arrancar el FS está idéntico (COW, no hay journal) |
| `FSCK C:` | 1 segundo — "0 errores". No escanea todo el disco |
| `SNAPSHOT LIST` | Muestra los últimos 64 estados del FS |
| `SNAPSHOT RESTORE 5` | Instantáneo — cambia la raíz del B-tree |
| Archivo de 10 bytes | 0 lecturas de disco para leerlo (inline data) |

---

## 2. Estructura en Disco

### 2.1 Mapa General

```text
LBA 0     Superblock (512B)
LBA 1     Root pointer (512B) — { root_btree_lba, version, timestamp }
LBA 2+    B-tree nodes (4KB cada uno)
          └── Nodos internos: apuntan a hijos
          └── Nodos hoja directory: DirEntry[]
          └── Nodos hoja indirectos: lista de extents (overflow)
          └── Nodos de freelist: lista de regiones libres
          └── Nodos de snapshot table: versión → raíz antigua
...       Data blocks (4KB cada uno)
```

### 2.2 Superblock (512 bytes)

```text
Offset  Size  Campo            Descripción
0       4     magic            "NE2\0"
4       4     version          Formato versión = 2
8       8     root_btree_lba   LBA del nodo raíz del B-tree del directorio raíz
16      8     root_version     Contador de versiones (se incrementa en cada escritura)
24      8     root_timestamp   Unix timestamp de la última modificación
32      8     num_blocks       Bloques totales en la partición
40      8     num_used         Bloques usados (aproximado)
48      8     num_free         Bloques libres (aproximado)
56      1     label_len        Longitud de la etiqueta del volumen (0-32)
57      32    label            Etiqueta (hasta 32 caracteres UTF-8)
89      4     flags            bit0=dirty, bit1=needs_fsck
93      4     checksum_interval Frecuencia de verificación de checksums (0=desactivado)
97      4     freelist_lba     LBA del primer nodo de freelist, 0 = usar bitmap implícito
101     4     snapshot_count   Número de snapshots almacenados
105     8     snapshot_table   LBA del nodo de tabla de snapshots
113     399   reserved         0
```

Total: 512 bytes.

### 2.3 B-tree Node (4KB)

```text
Offset  Size  Campo            Descripción
0       2     node_type        0=internal, 1=directory, 2=extent_list, 3=freelist, 4=snapshot_table
2       2     num_entries      Entradas válidas en este nodo
4       4     checksum         CRC32 del payload (offset 8..4096)
8       4088  payload          Según node_type
```

### 2.4 Node Type 1 — Directory Node (hoja de directorio)

Cada entrada = 128 bytes:

```text
Offset  Size  Campo            Descripción
0       8     inode_num        Número de inodo
8       8     data_lba         LBA del bloque de datos (0 = inline data)
16      4     size             Tamaño en bytes
20      8     created          Unix timestamp
28      8     modified         Unix timestamp
36      2     mode             bits 0-4 permisos (R,W,X,S,D), bit 6=directory, bit 7=file
38      2     link_count       Enlaces duros (futuro)
40      4     checksum         CRC32 del contenido del archivo
44      4     inline_len       0 = no inline. Si > 0, son los primeros N bytes del archivo aquí
48      208   inline_data      208 bytes de datos inline (si inline_len > 0) O nombre (si no)
                           El nombre empieza en byte 48 con un byte de longitud, luego hasta 207 bytes UTF-8
```

→ 32 entradas por nodo. Un directorio con 32000 archivos → altura 3-4.

**Inline data:** Si `size <= 208`, los datos están aquí mismo. Cero lecturas de disco para leer el archivo. El nombre y los datos comparten el mismo campo de 208 bytes (nunco hay ambos).

**Archivos grandes:** El `data_lba` apunta a un bloque de datos (extent único). Para archivos >4KB, se usa `data_lba` como puntero a un **nodo type 2 (extent_list)**.

### 2.5 Node Type 2 — Extent List (overflow para archivos grandes)

Cada entrada = 12 bytes:

```text
Offset  Size  Campo            Descripción
0       8     start_lba        LBA de inicio
8       4     length           Número de bloques contiguos
```

Caben ~340 entradas por nodo. Con altura 2 del B-tree, un archivo puede tener ~340 extents. Cada extent de 4KB → ~1.3GB. Si se necesita más, el último extent puede apuntar a otro nodo type 2.

### 2.6 Node Type 3 — Free List

Cada entrada = 12 bytes:

```text
Offset  Size  Campo            Descripción
0       8     start_lba        LBA de inicio de la región libre
8       4     length           Número de bloques libres contiguos
```

Caben ~340 regiones por nodo. Si se acaba el espacio, el nodo tiene `next_lba` al final (últimos 8 bytes del payload) apuntando a otro nodo freelist.

### 2.7 Node Type 4 — Snapshot Table

Cada entrada = 16 bytes:

```text
Offset  Size  Campo            Descripción
0       8     root_btree_lba   Raíz del B-tree en ese snapshot
8       8     timestamp        Cuándo se creó
```

Caben ~255 entradas por nodo. Máximo 64 snapshots (anillo circular).

---

## 3. Operaciones del Sistema de Archivos

### 3.1 Leer archivo

```text
path = "C:\DOCS\INFORME.TXT"
1. Cargar superblock → root_btree_lba
2. Recorrer B-tree: por cada componente, buscar en nodo directory
   "C:" → lookup en directorio raíz → nodo de "DOCS"
   "DOCS" → lookup en nodo "DOCS" → nodo de "INFORME.TXT"
3. Leer DirEntry de INFORME.TXT
4. Calcular CRC32 de los datos
5. Si checksum no coincide → devolver error específico (con LBA)
6. Si inline_len > 0 → devolver inline_data[0..size]
7. Si data_lba → leer extent(s) y devolver datos
```

### 3.2 Escribir archivo (COW)

```text
1. Leer DirEntry existente (o crear uno nuevo en el B-tree)
2. Alocar nuevos bloques de datos (de la freelist)
3. Escribir datos en los nuevos bloques
4. Calcular CRC32 de los datos
5. Crear nuevo DirEntry con nuevos extents (o inline)
6. Insertar nuevo DirEntry en el B-tree (COW: nuevos nodos hasta la raíz)
7. Nuevo root_btree_lba → escribir en superblock
8. root_version++ en superblock
9. root_timestamp = now en superblock
10. Si version % 64 == 0 → crear snapshot automático
```

**COW asegura consistencia:** Si el sistema se cae entre el paso 6 y 7, el superblock sigue apuntando a la raíz vieja. El archivo está intacto. No hay journal, no hay replay.

### 3.3 Crear archivo

```text
1. Alocar inode_num (contador en superblock, sin tabla global)
2. Crear DirEntry: name, size=0, mode, created, link_count=1, checksum=0, inline_len=0
3. Insertar en el B-tree del directorio padre (COW)
4. root_version++
```

### 3.4 Borrar archivo

```text
DEL INFORME.TXT

1. Lookup en B-tree del directorio padre
2. Si el archivo tiene bloques de datos: caminar extents, liberar bloques a freelist
3. Eliminar la entrada del B-tree (COW)
4. root_version++
```

`DEL` siempre libera los bloques de datos inmediatamente. Es O(log n + extents).

### 3.5 Borrar directorio

```text
RD VACIO
1. Si el directorio tiene entradas (archivos o subdirs): "Directory not empty"
2. Si está vacío: eliminar entrada del B-tree (COW), root_version++

RD /F PROYECTO  (force: borra aunque tenga contenido)
1. Lookup del nodo directory de PROYECTO
2. Caminar el subárbol recursivamente recolectando todos los LBAs de datos
3. Liberar todos los bloques de datos a freelist
4. Eliminar la entrada del B-tree (COW)
5. root_version++
```

### 3.6 Renombrar

```text
1. Lookup del DirEntry en el directorio padre
2. Modificar el nombre en el DirEntry
3. Actualizar el B-tree (COW)
4. root_version++
```

### 3.7 Snapshot

```text
SNAPSHOT CREATE
1. Copiar (root_btree_lba, root_timestamp) a la snapshot table
2. snapshot_count++ (circular, máximo 64)

SNAPSHOT RESTORE N
1. root_btree_lba = snapshot[N].root_btree_lba
2. root_version++
3. El FS ahora ve el árbol como estaba en el momento N

SNAPSHOT PURGE
1. Vaciar snapshot table
2. root_version++
3. Los bloques que solo referenciaban snapshots viejos se recolectan

Nota: RESTORE no pierde los snapshots más recientes. El FS sigue
escribiendo en el nuevo root_btree_lba. Los snapshots viejos siguen
accesibles. Solo PURGE libera espacio definitivamente.
```

### 3.8 FSCK (rápido)

```text
1. Verificar checksum del superblock
2. root_version coincide con el esperado?
3. Verificar checksums de todos los nodos B-tree reachables
4. Verificar que freelist + used_blocks = total_blocks
5. NO verificar checksums de datos (opcional con flag --deep)
```

---

## 4. API

### 4.1 VFS trait (sin cambios)

```rust
trait FileSystem: Send {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError>;
    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError>;
    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError>;
    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError>;
    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError>;
    fn remove_file(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError>;
    fn remove_dir(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError>;
    fn rename(&mut self, dir_inode: u32, old_name: &str, new_name: &str) -> Result<(), VfsError>;
    fn volume_label(&self) -> Result<String, VfsError>;
    fn set_volume_label(&mut self, label: &str) -> Result<(), VfsError>;
    fn fs_type(&self) -> &'static str;
    fn total_sectors(&self) -> u64;
}
```

### 4.2 Nuevos métodos en NeoDosFs

```rust
impl NeoDosFs {
    fn create_snapshot(&mut self) -> Result<u64, FsError>;
    fn list_snapshots(&self) -> Result<Vec<SnapshotInfo>, FsError>;
    fn restore_snapshot(&mut self, id: u64) -> Result<(), FsError>;
    fn purge_snapshots(&mut self) -> Result<(), FsError>;
}
```

### 4.3 Syscall nueva

#### RAX 77 — `sys_ob_snapshot`

```text
RBX = fd (handle a la raíz del FS, ej: \Global\FileSystem\C:\)
RCX = op: 0=CREATE, 1=RESTORE, 2=LIST, 3=PURGE
RDX = buf (para LIST: buffer de salida; para RESTORE: snapshot_id u64)
R8  = buf_size

Returns:
  CREATE → snapshot_id (u64) o error
  RESTORE → 0 o error
  LIST → número de snapshots escritos en buf
  PURGE → 0 o error

Errors: -Inval, -NoEnt, -Io, -NoSys (si no es NeoFS)
```

---

## 5. Permisos (mode bits)

```text
bit 0 = R (read)
bit 1 = W (write)
bit 2 = X (execute)
bit 3 = S (system)
bit 4 = D (delete)
bit 6 = directory flag
bit 7 = file flag
```

Sin DOS attributes. Sin owner/group (futuro: SID). Sin ACLs (futuro).

---

## 6. NeoFS v1

NeoFS v1 (magic "NEOD") is **obsolete and has been removed**. The kernel rejects NEOD superblocks at mount time with a clear error message. NeoFS v2 (NE2) is the only native filesystem format.

---

## 7. Archivos creados/modificados

```text
neodos-kernel/src/
├── fs/
│   ├── mod.rs                          ─ módulos v2 (neodos_v2, neodos_dir, neodos_io, btree, freelist, snapshot)
│   ├── btree.rs                        ─ B-tree persistente genérico
│   ├── neodos_v2.rs                    ─ NeoDosFsV2 sobre B-tree + extents + COW
│   ├── neodos_dir.rs                   ─ Directorio B-tree (DirEntryV2)
│   ├── neodos_io.rs                    ─ Extent read/write + inline data
│   ├── freelist.rs                     ─ Free region list
│   ├── snapshot.rs                     ─ Snapshot table + GC lazy
│   ├── vfs.rs                          ─ sin cambios (trait FileSystem igual)
│   ├── neodos_fs.rs                    ─ ELIMINADO (NeoFS v1 obsoleto)
│   ├── fsck.rs                         ─ ELIMINADO (fsck v1 eliminado con v1)
│   └── journal.rs                      ─ ELIMINADO (reemplazado por COW)
├── syscall/
│   ├── mod.rs                          ─ añadir handler_ob_snapshot en SSDT (RAX 77)
│   └── ob.rs                           ─ añadir handler_ob_snapshot
└── object/
    └── types.rs                        ─ posible ObInfoClass::Snapshot
```

---

## 8. Tests (mínimos por invariante)

| Test | Invariante |
| ------ | ----------- |
| `neofs_v2_create_file` | Crear archivo → size=0, mode correcto, existe en B-tree |
| `neofs_v2_write_read_roundtrip` | Escribir datos → leer → iguales |
| `neofs_v2_cow_root_version_inc` | Escribir → root_version++ |
| `neofs_v2_cow_preserves_old_data` | Escribir, guardar root_version, escribir más, leer root vieja → datos originales |
| `neofs_v2_delete_removes_entry` | DEL → lookup falla |
| `neofs_v2_delete_frees_inode` | DEL → inode_num reusable |
| `neofs_v2_rmdir_recursive` | RD con archivos dentro → todos eliminados |
| `neofs_v2_checksum_verify` | Escribir datos, modificar byte en disco → read detecta CRC mismatch |
| `neofs_v2_inline_small_file` | Archivo <208 bytes → sin bloques de datos |
| `neofs_v2_large_file_extents` | Archivo >4KB → múltiples extents, lectura correcta |
| `neofs_v2_snapshot_create_list` | Crear snapshots → listar → IDs correctos |
| `neofs_v2_snapshot_restore` | Modificar, snapshot, modificar más, restaurar → datos del snapshot |
| `neofs_v2_freelist_alloc_free` | Alocar bloque → usado. Liberar → libre |
| `neofs_v2_freelist_merge_adjacent` | Liberar bloques adyacentes → una región |
| `neofs_v2_fsck_clean` | FS sin errores → fsck no reporta nada |
| `neofs_v2_fsck_corrupt_btree` | Nodo B-tree corrupto → fsck lo detecta |
| `neofs_v2_dir_10k_entries` | 10000 archivos en un directorio → DIR funciona |
| `neofs_v2_rename_file` | REN → nombre viejo falla, nombre nuevo existe |
| `neofs_v2_mkdir_lookup` | MD → DIR muestra el directorio, CD entra |
| `neofs_v2_write_power_loss_safe` | Simular corte entre COW pasos → datos preservados |

---

## 9. Plan de Implementación (por orden)

1. `src/fs/btree.rs` — B-tree: insert, lookup, delete, walk, COW clone
2. `src/fs/freelist.rs` — Free list: alloc, free, merge, save/load
3. `src/fs/snapshot.rs` — Snapshot table: create, list, restore, purge
4. `src/fs/neodos_v2.rs` — FileSystem trait impl con B-tree + extents + COW
5. `src/fs/fsck.rs` — Scrub de B-tree + checksums
6. `src/syscall/ob.rs` — handler_ob_snapshot (RAX 77)
7. Tests

---

*Esto es el diseño. No se ha escrito código de implementación.*
