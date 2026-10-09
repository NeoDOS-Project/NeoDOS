//! NeoFS v2 (NE2) — implementación completa del FileSystem trait.

#![allow(dead_code)]

use alloc::vec::Vec;
use alloc::string::String;
use crate::vfs::io::IoStack;
use crate::fs::vfs::{FileSystem, VfsNode, DirEntry, VfsError, MODE_DIR, MODE_FILE};
use crate::fs::btree::{BTree, BTreeNode, BTreeIO, NodeType, NODE_SIZE};
use crate::fs::freelist::{FreeList, FreeRegion};
use crate::fs::neodos_dir::{DirEntryV2, dir_lookup, dir_readdir, dir_count, DIRENTRY_SIZE, PERM_R, PERM_W, PERM_X, PERM_D};
use crate::fs::neodos_io::{file_read, file_write, crc32};
use crate::fs::snapshot::{SnapshotTable, SnapshotEntryRaw};

const SUPERBLOCK_MAGIC: u32 = 0x0032454E; // "NE2\0"
/// Límite defensivo del encadenado de nodos de free list (ciclos/corrupción).
const MAX_FREELIST_NODES: usize = 1 << 20;
/// Offset dentro del superblock donde se guarda su propio CRC32.
const SB_CHECKSUM_OFFSET: usize = 109;

#[repr(C, packed)]
#[derive(Clone, Copy)]
#[repr(C)]
pub(super) struct SuperblockNE2 {
    magic: u32,
    version: u32,
    root_btree_lba: u64,
    root_version: u64,
    root_timestamp: u64,
    num_blocks: u64,
    num_used: u64,
    num_free: u64,
    label_len: u8,
    label: [u8; 32],
    flags: u32,
    freelist_lba: u64,
    snapshot_table_lba: u64,
    reserved: [u8; 403],
}

/// Entrada del inode cache: raíz del B-tree del directorio (o del directorio
/// padre para un fichero), su `DirEntry` tal como vive en el padre, y el inode
/// del directorio padre (para propagar cambios de raíz hacia arriba).
#[derive(Clone)]
struct CachedInode {
    root: u64,
    entry: DirEntryV2,
    parent: Option<u32>,
}

pub struct NeoDosFsV2 {
    sb: SuperblockNE2,
    freelist: FreeList,
    pub io_stack: IoStack,
    inode_cache: Vec<Option<CachedInode>>,
    next_inode: u32,
    pub snapshot_table: SnapshotTable,
    /// LBAs de los nodos de free list actualmente persistidos (cabeza en
    /// `sb.freelist_lba`). Se liberan antes de reescribir la free list.
    freelist_chain: Vec<u64>,
    /// Bloques de B-tree y extents de datos reemplazados por COW y aún no
    /// reclamados. Solo pueden liberarse cuando no hay snapshots (que podrían
    /// referenciar árboles/datos antiguos); si los hay, se retienen hasta
    /// `snapshot_purge`.
    cow_garbage: Vec<(u64, u32)>,
    /// `true` si la tabla de snapshots cambió desde la última persistencia.
    snapshot_dirty: bool,
}

impl BTreeIO for NeoDosFsV2 {
    fn read_node(&self, block_lba: u64) -> Option<BTreeNode> {
        let sector_lba = block_lba * 8;
        let abs_sector = self.io_stack.translate_lba(sector_lba);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id)?;
        let mut buf = [0u8; NODE_SIZE];
        for i in 0..8usize {
            let s = dev.read_sector(abs_sector + i as u64).ok()?;
            buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
        }
        drop(bdevs);
        BTreeNode::deserialize(&buf)
    }

    fn write_node(&mut self, node: &BTreeNode) -> u64 {
        let block_lba = self.freelist.alloc_blocks(1).unwrap_or(0);
        if block_lba == 0 { return 0; }
        let sector_lba = block_lba * 8;
        let abs_sector = self.io_stack.translate_lba(sector_lba);
        // Lock order: PAGE_CACHE before BLOCK_DEVICES. This matches
        // `IoStack::read_sectors`/`write_sectors` and
        // `globals::flush_cache_if_needed`. Taking BLOCK_DEVICES first here
        // (as the code used to) is the inverse order and deadlocks against
        // those paths under SMP (#343).
        let _ord_pc = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
        let mut pc = crate::globals::PAGE_CACHE.lock();
        let _ord_bd = crate::lock_order::Guard::new(crate::lock_order::BLOCK_DEVICES);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs.get(self.io_stack.device_id) { Some(d) => d, None => return 0 };
        let mut buf = [0u8; NODE_SIZE];
        node.serialize(&mut buf);
        for i in 0..8usize {
            let mut sec = [0u8; 512];
            sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
            if dev.write_sector(abs_sector + i as u64, &sec).is_err() { return 0; }
        }
        // Invalidate page cache for these sectors — a freed data block
        // may have dirty pages left over from file_write, which would
        // overwrite B-tree metadata on flush.
        pc.invalidate_range(abs_sector, abs_sector + 8);
        block_lba
    }
}

impl NeoDosFsV2 {
    pub fn new(io_stack: IoStack) -> Result<Self, ()> {
        // Leer el superblock directamente del dispositivo (sin page cache):
        // un remontaje tras escrituras COW no debe observar sectores cacheados.
        let raw = read_superblock_raw(&io_stack)?;
        // Manually parse to avoid transmute layout issues
        let magic = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
        if magic != SUPERBLOCK_MAGIC { return Err(()); }
        let root_btree_lba = u64::from_le_bytes([raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15]]);
        let version = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let root_version = u64::from_le_bytes([raw[16], raw[17], raw[18], raw[19], raw[20], raw[21], raw[22], raw[23]]);
        let root_timestamp = u64::from_le_bytes([raw[24], raw[25], raw[26], raw[27], raw[28], raw[29], raw[30], raw[31]]);
        let num_blocks = u64::from_le_bytes([raw[32], raw[33], raw[34], raw[35], raw[36], raw[37], raw[38], raw[39]]);
        let num_used = u64::from_le_bytes([raw[40], raw[41], raw[42], raw[43], raw[44], raw[45], raw[46], raw[47]]);
        let num_free = u64::from_le_bytes([raw[48], raw[49], raw[50], raw[51], raw[52], raw[53], raw[54], raw[55]]);
        let label_len = raw[56];
        let mut label = [0u8; 32];
        label.copy_from_slice(&raw[57..89]);
        let flags = u32::from_le_bytes([raw[89], raw[90], raw[91], raw[92]]);
        let freelist_lba = u64::from_le_bytes([raw[93], raw[94], raw[95], raw[96], raw[97], raw[98], raw[99], raw[100]]);
        let snapshot_table_lba = u64::from_le_bytes([raw[101], raw[102], raw[103], raw[104], raw[105], raw[106], raw[107], raw[108]]);
        let reserved = {
            let mut r = [0u8; 403];
            let len = r.len().min(512 - 109);
            r[..len].copy_from_slice(&raw[109..109 + len]);
            r
        };
        let sb = SuperblockNE2 { magic, version, root_btree_lba, root_version, root_timestamp,
            num_blocks, num_used, num_free, label_len, label, flags, freelist_lba, snapshot_table_lba, reserved };

        let mut inode_cache = Vec::new();
        let root_entry = DirEntryV2::new_dir("\\");
        inode_cache.push(Some(CachedInode { root: sb.root_btree_lba, entry: root_entry, parent: None }));
        let snapshot_table = if sb.snapshot_table_lba > 0 {
            let mut node_buf = [0u8; NODE_SIZE];
            let sector_lba = sb.snapshot_table_lba * 8;
            let abs_sector = io_stack.translate_lba(sector_lba);
            let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
            if let Some(dev) = bdevs.get(io_stack.device_id) {
                for i in 0..8usize {
                    if let Ok(s) = dev.read_sector(abs_sector + i as u64) {
                        node_buf[i * 512..(i + 1) * 512].copy_from_slice(&s);
                    }
                }
            }
            drop(bdevs);
            SnapshotTable::deserialize(&node_buf).unwrap_or_else(|| SnapshotTable::new())
        } else {
            SnapshotTable::new()
        };

        let mut fs = NeoDosFsV2 {
            sb,
            freelist: FreeList::new(),
            io_stack,
            inode_cache,
            next_inode: 1,
            snapshot_table,
            freelist_chain: Vec::new(),
            cow_garbage: Vec::new(),
            snapshot_dirty: false,
        };

        // Recuperar la free list: si el superblock apunta a una cadena
        // persistida y es válida, se carga; en caso contrario se reconstruye
        // recorriendo el árbol de directorios.
        let head = fs.sb.freelist_lba;
        if head == 0 || !fs.load_freelist(head) {
            fs.recover_freelist();
        }

        Ok(fs)
    }

    /// Leer un bloque de 4KB directamente del dispositivo (sin page cache),
    /// coherente con `BTreeIO::read_node`.
    fn read_block_raw(&self, block_lba: u64, buf: &mut [u8; NODE_SIZE]) -> bool {
        let sector_lba = self.io_stack.translate_lba(block_lba * 8);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs.get(self.io_stack.device_id) { Some(d) => d, None => return false };
        for i in 0..8usize {
            match dev.read_sector(sector_lba + i as u64) {
                Ok(s) => buf[i * 512..(i + 1) * 512].copy_from_slice(&s),
                Err(_) => return false,
            }
        }
        true
    }

    /// Escribir un bloque de 4KB directamente al dispositivo e invalidar el
    /// page cache de sus sectores (igual que `BTreeIO::write_node`).
    fn write_block_raw(&self, block_lba: u64, buf: &[u8; NODE_SIZE]) -> bool {
        let sector_lba = self.io_stack.translate_lba(block_lba * 8);
        let _ord_pc = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
        let mut pc = crate::globals::PAGE_CACHE.lock();
        let _ord_bd = crate::lock_order::Guard::new(crate::lock_order::BLOCK_DEVICES);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = match bdevs.get(self.io_stack.device_id) { Some(d) => d, None => return false };
        for i in 0..8usize {
            let mut sec = [0u8; 512];
            sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
            if dev.write_sector(sector_lba + i as u64, &sec).is_err() { return false; }
        }
        pc.invalidate_range(sector_lba, sector_lba + 8);
        true
    }

    /// Cargar la cadena de nodos de free list a partir de su cabeza.
    /// Devuelve `false` si algún nodo es ilegible, la cadena tiene un ciclo o
    /// las regiones resultantes no son válidas.
    fn load_freelist(&mut self, head: u64) -> bool {
        let mut regions: Vec<FreeRegion> = Vec::new();
        let mut chain: Vec<u64> = Vec::new();
        let mut lba = head;
        while lba != 0 {
            if chain.len() >= MAX_FREELIST_NODES || chain.contains(&lba) {
                return false;
            }
            let mut buf = [0u8; NODE_SIZE];
            if !self.read_block_raw(lba, &mut buf) {
                return false;
            }
            let (mut node_list, next) = match FreeList::deserialize(&buf) {
                Some(v) => v,
                None => return false,
            };
            regions.append(&mut node_list.regions);
            chain.push(lba);
            lba = next;
        }

        let fl = FreeList { regions, dirty: false };
        if !fl.is_valid(self.sb.num_blocks) {
            return false;
        }
        self.freelist = fl;
        self.freelist_chain = chain;
        true
    }

    /// Persistencia **perezosa** de la free list.
    ///
    /// En vez de reescribir la lista en cada guardado, se **invalida** el puntero
    /// (`freelist_lba = 0`) y se reconstruye al montar recorriendo el árbol
    /// (snapshot-aware). Los bloques de la cadena anterior se devuelven a la
    /// lista. La free list solo se persiste en disco al formatear (mkfs/imagen),
    /// que da un arranque rápido en volúmenes intactos.
    fn save_freelist(&mut self) {
        if !self.freelist.dirty && self.sb.freelist_lba == 0 {
            return;
        }
        for &lba in &self.freelist_chain {
            self.freelist.free(lba, 1);
        }
        self.freelist_chain.clear();
        self.sb.freelist_lba = 0;
        self.freelist.dirty = false;
    }

    /// Persistir la tabla de snapshots (nodo tipo 4, una sola página) y
    /// actualizar `sb.snapshot_table_lba`. La tabla vacía se representa con
    /// `snapshot_table_lba = 0` (no ocupa bloque).
    fn save_snapshot_table(&mut self) {
        // Sin cambios desde la última persistencia → nada que hacer.
        if !self.snapshot_dirty {
            return;
        }
        if self.snapshot_table.snapshot_count() == 0 {
            if self.sb.snapshot_table_lba != 0 {
                self.freelist.free(self.sb.snapshot_table_lba, 1);
                self.sb.snapshot_table_lba = 0;
            }
            self.snapshot_dirty = false;
            return;
        }
        // Reutilizar el bloque actual (in-place) si ya existe.
        let lba = if self.sb.snapshot_table_lba != 0 {
            self.sb.snapshot_table_lba
        } else {
            match self.freelist.alloc(1) {
                Some((lba, _)) => lba,
                None => {
                    self.snapshot_dirty = false;
                    return;
                }
            }
        };
        let mut buf = [0u8; NODE_SIZE];
        self.snapshot_table.serialize(&mut buf);
        if self.write_block_raw(lba, &buf) {
            self.sb.snapshot_table_lba = lba;
        }
        self.snapshot_dirty = false;
    }


    /// Reclamar la basura COW. Solo es seguro con la tabla de snapshots vacía:
    /// con snapshots presentes un nodo reemplazado puede seguir siendo
    /// alcanzable desde una raíz antigua, así que se retiene hasta PURGE.
    ///
    /// Es seguro ahora que `propagate_dir_root` actualiza el `DirEntry` del
    /// padre al cambiar la raíz de un subdirectorio (#563).
    fn reclaim_cow_garbage(&mut self) {
        if self.snapshot_table.snapshot_count() > 0 {
            return;
        }
        let garbage = core::mem::take(&mut self.cow_garbage);
        for (lba, len) in garbage {
            // Los bloques 0 (superblock) y 1 (raíz inicial) están reservados:
            // no se reintroducen en la free list.
            if lba >= 2 && len > 0 {
                self.freelist.free(lba, len);
            }
        }
    }

    /// Reconstruir la free list recorriendo el árbol de directorios actual y los
    /// de **todos los snapshots**, marcando como usados los nodos B-tree, los
    /// extents de datos, los bloques 0/1 y el nodo de la tabla de snapshots.
    /// Necesario porque la free list no se persiste en cada guardado (lazy).
    fn recover_freelist(&mut self) {
        let total = self.sb.num_blocks;
        let mut used: Vec<u64> = alloc::vec![0, 1];
        let mut pending: Vec<u64> = Vec::new();
        if self.sb.root_btree_lba != 0 {
            pending.push(self.sb.root_btree_lba);
        }
        // Las raíces de los snapshots referencian árboles/datos que siguen vivos.
        for (_, snap) in self.snapshot_table.list() {
            if snap.root_lba != 0 {
                pending.push(snap.root_lba);
            }
        }
        while let Some(dir_root) = pending.pop() {
            self.collect_directory_blocks(dir_root, &mut used, &mut pending);
        }
        // El nodo de la tabla de snapshots también está en uso.
        if self.sb.snapshot_table_lba != 0 {
            used.push(self.sb.snapshot_table_lba);
        }
        used.sort_unstable();
        used.dedup();
        self.freelist = FreeList::from_used(&used, total);
        self.freelist_chain.clear();
        // Reconstruida: no hace falta persistirla (se reconstruirá al montar).
        self.freelist.dirty = false;
    }

    /// Marcar los bloques usados por el B-tree de `dir_root` y encolar los
    /// subdirectorios encontrados para su propio recorrido.
    fn collect_directory_blocks(&self, dir_root: u64, used: &mut Vec<u64>, pending: &mut Vec<u64>) {
        BTree::walk_lbas(self, dir_root, &mut |lba| used.push(lba));
        BTree::walk(self, dir_root, &mut |entry| {
            let e = DirEntryV2::from_btree_entry(entry);
            if e.extent_lba != 0 && e.extent_count > 0 {
                let end = e.extent_lba.saturating_add(e.extent_count as u64);
                for b in e.extent_lba..end {
                    used.push(b);
                }
            }
            if e.is_dir() && e.extent_lba != 0 && e.extent_lba != dir_root {
                pending.push(e.extent_lba);
            }
        });
    }

    fn alloc_inum(&mut self) -> u32 {
        let i = self.next_inode; self.next_inode += 1; i
    }

    /// Cachear una entrada. Para directorios, `root` es la raíz de su B-tree
    /// (`entry.extent_lba`); para ficheros, `fallback_root` (raíz del directorio
    /// padre). `parent` es el inode del directorio que la contiene.
    fn cache(&mut self, parent: u32, entry: DirEntryV2, fallback_root: u64) -> u32 {
        let root = if entry.is_dir() && entry.extent_lba > 0 { entry.extent_lba } else { fallback_root };
        if entry.is_dir() && entry.extent_lba > 0 {
            for i in 0..self.inode_cache.len() {
                if let Some(c) = &self.inode_cache[i] {
                    if c.root == root && c.entry.name == entry.name {
                        return i as u32;
                    }
                }
            }
        }
        let i = self.alloc_inum();
        if i as usize >= self.inode_cache.len() { self.inode_cache.resize(i as usize + 1, None); }
        self.inode_cache[i as usize] = Some(CachedInode { root, entry, parent: Some(parent) });
        i
    }

    /// Propagar un cambio de raíz de B-tree del directorio `inode` hacia arriba:
    /// actualiza su `DirEntry.extent_lba` en el directorio padre, reinserta esa
    /// entrada en el árbol del padre (COW) y continúa con el abuelo. Para la
    /// raíz (inode 0) actualiza `sb.root_btree_lba`.
    fn propagate_dir_root(&mut self, inode: u32, new_root: u64) {
        if inode == 0 {
            self.sb.root_btree_lba = new_root;
            if let Some(c) = self.inode_cache.get_mut(0).and_then(|x| x.as_mut()) {
                c.root = new_root;
            }
            return;
        }
        let (name, mut entry, parent) = match self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()) {
            Some(c) => (c.entry.name.clone(), c.entry.clone(), c.parent),
            None => return,
        };
        entry.extent_lba = new_root;
        if let Some(c) = self.inode_cache.get_mut(inode as usize).and_then(|x| x.as_mut()) {
            c.root = new_root;
            c.entry.extent_lba = new_root;
        }
        let parent = match parent { Some(p) => p, None => return };
        let parent_root = match self.inode_cache.get(parent as usize).and_then(|x| x.as_ref()) {
            Some(c) => c.root,
            None => return,
        };
        let mut garbage = Vec::new();
        let mut tmp = [0u8; DIRENTRY_SIZE];
        entry.serialize(&mut tmp);
        let new_parent_root = match BTree::insert_tracked(self, parent_root, &name, &tmp.to_vec(), &mut garbage) {
            Some(r) => r,
            None => return,
        };
        for lba in garbage { self.cow_garbage.push((lba, 1)); }
        self.propagate_dir_root(parent, new_parent_root);
    }

    /// Leer bytes de un `DirEntry` (inline o extents). Usado por `read` y por la
    /// extracción desde snapshots.
    fn read_entry_bytes(&mut self, entry: &DirEntryV2, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let abs_lba = self.io_stack.translate_lba(entry.extent_lba * 8);
        // Lock order: PAGE_CACHE before BLOCK_DEVICES (#343).
        let _ord_pc = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
        let mut pc = crate::globals::PAGE_CACHE.lock();
        let _ord_bd = crate::lock_order::Guard::new(crate::lock_order::BLOCK_DEVICES);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;
        let mut adj_entry = entry.clone();
        adj_entry.extent_lba = abs_lba;
        file_read(&adj_entry, offset, buf, &mut *pc, self.io_stack.device_id as u64, dev).map_err(|_| VfsError::IOError)
    }

    /// Resolver `path` (relativo al volumen, con o sin `X:` inicial) dentro del
    /// árbol cuya raíz es `root`. Devuelve el `DirEntry` final.
    fn resolve_entry_in(&mut self, root: u64, path: &str) -> Option<DirEntryV2> {
        let comps = path_components(path);
        if comps.is_empty() { return None; }
        let mut cur = root;
        for (i, c) in comps.iter().enumerate() {
            let e = dir_lookup(self, cur, c)?;
            if i + 1 == comps.len() { return Some(e); }
            if !e.is_dir() || e.extent_lba == 0 { return None; }
            cur = e.extent_lba;
        }
        None
    }

    /// Resolver el directorio padre de `path` en el árbol actual, devolviendo
    /// (inode del padre, nombre final).
    fn resolve_parent_inode(&mut self, path: &str) -> Option<(u32, alloc::string::String)> {
        let comps = path_components(path);
        if comps.is_empty() { return None; }
        let mut inode = 0u32;
        for c in &comps[..comps.len() - 1] {
            let n = self.lookup(inode, c).ok()?;
            inode = n.inode;
        }
        Some((inode, comps[comps.len() - 1].clone()))
    }

    fn save_sb(&mut self) -> Result<(), ()> {
        self.sb.root_version = self.sb.root_version.wrapping_add(1);
        self.sb.root_timestamp = crate::hal::get_ticks();
        // La tabla de snapshots se persiste antes que la free list: así el
        // bloque que ocupa queda excluido de la lista serializada.
        self.save_snapshot_table();
        // Reclamar nodos COW reemplazados (solo si no hay snapshots).
        self.reclaim_cow_garbage();
        // Persistir la free list: actualiza `sb.freelist_lba`.
        self.save_freelist();
        self.sb.num_used = self.sb.num_blocks.saturating_sub(self.freelist.total_free());
        self.sb.num_free = self.freelist.total_free();
        let cksum = superblock_crc(&self.sb).to_le_bytes();
        self.sb.reserved[..4].copy_from_slice(&cksum);
        let raw = unsafe { core::slice::from_raw_parts(&self.sb as *const _ as *const u8, 512) };
        let mut sector = [0u8; 512];
        sector.copy_from_slice(raw);
        self.io_stack.write_sector(0, &sector).ok();
        invalidate_cache(&self.io_stack, 0, 1);
        Ok(())
    }
}

/// Componentes de un path de volumen (`C:\A\B` o `\A\B`), sin el prefijo de
/// unidad ni separadores vacíos.
fn path_components(path: &str) -> Vec<alloc::string::String> {
    let p = if path.len() >= 2 && path.as_bytes()[1] == b':' { &path[2..] } else { path };
    p.split(['\\', '/']).filter(|s| !s.is_empty()).map(alloc::string::String::from).collect()
}

/// CRC32 de integridad del superblock. Cubre los 512 bytes con el propio
/// campo de checksum (`reserved[0..4]`) puesto a cero. Debe coincidir con
/// `SuperblockNE2::checksum` en `fs/fsck/ne2.rs`.
fn superblock_crc(sb: &SuperblockNE2) -> u32 {
    let raw = unsafe { core::slice::from_raw_parts(sb as *const _ as *const u8, 512) };
    let mut buf = [0u8; 512];
    buf.copy_from_slice(raw);
    buf[SB_CHECKSUM_OFFSET..SB_CHECKSUM_OFFSET + 4].fill(0);
    crc32(&buf)
}

/// Leer el sector 0 directamente del dispositivo, sin pasar por el page cache.
fn read_superblock_raw(io_stack: &IoStack) -> Result<[u8; 512], ()> {
    let abs_sector = io_stack.translate_lba(0);
    let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
    let dev = bdevs.get(io_stack.device_id).ok_or(())?;
    dev.read_sector(abs_sector)
}

/// Invalidar en el page cache un rango de sectores absolutos.
fn invalidate_cache(io_stack: &IoStack, start_sector: u64, count: u64) {
    let abs = io_stack.translate_lba(start_sector);
    let _ord_pc = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
    let mut pc = crate::globals::PAGE_CACHE.lock();
    pc.invalidate_range(abs, abs + count);
}

impl FileSystem for NeoDosFsV2 {
    fn read(&mut self, inode: u32, offset: u64, buf: &mut [u8]) -> Result<usize, VfsError> {
        let entry = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).map(|c| c.entry.clone()).ok_or(VfsError::NotFound)?;
        self.read_entry_bytes(&entry, offset, buf)
    }

    fn write(&mut self, inode: u32, offset: u64, buf: &[u8]) -> Result<usize, VfsError> {
        let (btree_root, entry, parent) = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref())
            .map(|c| (c.root, c.entry.clone(), c.parent)).ok_or(VfsError::NotFound)?;
        // Lock order: PAGE_CACHE before BLOCK_DEVICES (#343).
        let _ord_pc = crate::lock_order::Guard::new(crate::lock_order::PAGE_CACHE);
        let mut pc = crate::globals::PAGE_CACHE.lock();
        let _ord_bd = crate::lock_order::Guard::new(crate::lock_order::BLOCK_DEVICES);
        let mut bdevs = crate::globals::BLOCK_DEVICES.lock();
        let dev = bdevs.get(self.io_stack.device_id).ok_or(VfsError::IOError)?;
        let part_base = self.io_stack.translate_lba(0);
        let new_entry = file_write(&entry, offset, buf, &mut self.freelist, &mut *pc, self.io_stack.device_id as u64, dev, part_base).map_err(|_| VfsError::IOError)?;
        drop(bdevs);
        drop(_ord_bd);
        drop(pc);
        drop(_ord_pc);

        // Los extents antiguos del archivo quedan reemplazados por los nuevos
        // bloques COW; se reclamarán cuando no haya snapshots.
        if entry.extent_lba != 0 && entry.extent_count > 0 {
            self.cow_garbage.push((entry.extent_lba, entry.extent_count));
        }

        let mut garbage = Vec::new();
        let new_root = BTree::insert_tracked(self, btree_root, &new_entry.name, &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; new_entry.serialize(&mut tmp); tmp.to_vec()
        }, &mut garbage).ok_or(VfsError::IOError)?;
        for lba in garbage { self.cow_garbage.push((lba, 1)); }

        if let Some(p) = parent { self.propagate_dir_root(p, new_root); }
        if let Some(c) = self.inode_cache.get_mut(inode as usize).and_then(|x| x.as_mut()) {
            c.root = new_root;
            c.entry = new_entry;
        }
        Ok(buf.len())
    }

    fn lookup(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        let entry = dir_lookup(self, btree_root, name).ok_or(VfsError::NotFound)?;
        let size = if entry.inline_len > 0 { entry.inline_len as u32 } else { entry.size as u32 };
        let mode = entry.mode;
        let inum = self.cache(dir_inode, entry, btree_root);
        Ok(VfsNode { inode: inum, mode, size })
    }

    fn readdir(&mut self, dir_inode: u32, index: usize) -> Result<Option<DirEntry>, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        match dir_readdir(self, btree_root, index) {
            Some(e) => {
                let inum = self.cache(dir_inode, e, btree_root);
                let cached = self.inode_cache[inum as usize].as_ref().ok_or(VfsError::NotFound)?;
                let dname = core::str::from_utf8(&cached.entry.name).unwrap_or("?");
                let size = if cached.entry.inline_len > 0 { cached.entry.inline_len as u32 } else { cached.entry.size as u32 };
                Ok(Some(DirEntry { name: dname.into(), node: VfsNode { inode: inum, mode: cached.entry.mode, size } }))
            }
            None => Ok(None),
        }
    }

    fn mkdir(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        let empty = BTreeNode::new(NodeType::Leaf);
        let subdir_root = self.write_node(&empty);
        if subdir_root == 0 { return Err(VfsError::IOError); }

        let mut entry = DirEntryV2::new_dir(name);
        entry.extent_lba = subdir_root;
        entry.created = crate::hal::get_ticks(); entry.modified = entry.created;

        let mut garbage = Vec::new();
        let new_root = BTree::insert_tracked(self, btree_root, name.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; entry.serialize(&mut tmp); tmp.to_vec()
        }, &mut garbage).ok_or(VfsError::IOError)?;
        for lba in garbage { self.cow_garbage.push((lba, 1)); }

        self.propagate_dir_root(dir_inode, new_root);
        self.save_sb().map_err(|_| VfsError::IOError)?;
        let inum = self.cache(dir_inode, entry, new_root);
        Ok(VfsNode { inode: inum, mode: MODE_DIR | PERM_R | PERM_W | PERM_X | PERM_D, size: 0 })
    }

    fn create(&mut self, dir_inode: u32, name: &str) -> Result<VfsNode, VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        let entry = DirEntryV2::new_file(name);
        let mut garbage = Vec::new();
        let new_root = BTree::insert_tracked(self, btree_root, name.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; entry.serialize(&mut tmp); tmp.to_vec()
        }, &mut garbage).ok_or(VfsError::IOError)?;
        for lba in garbage { self.cow_garbage.push((lba, 1)); }
        self.propagate_dir_root(dir_inode, new_root);
        self.save_sb().map_err(|_| VfsError::IOError)?;
        let inum = self.cache(dir_inode, entry, new_root);
        Ok(VfsNode { inode: inum, mode: MODE_FILE | PERM_R | PERM_W | PERM_X | PERM_D, size: 0 })
    }

    fn stat(&mut self, inode: u32) -> Result<VfsNode, VfsError> {
        let entry = self.inode_cache.get(inode as usize).and_then(|x| x.as_ref()).map(|c| c.entry.clone()).ok_or(VfsError::NotFound)?;
        let size = if entry.inline_len > 0 { entry.inline_len as u32 } else { entry.size as u32 };
        Ok(VfsNode { inode, mode: entry.mode, size })
    }

    fn remove_file(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        // Los extents se enrutan por la basura COW: si hay snapshots que
        // los referencian, se retienen hasta PURGE (#569).
        if let Some(e) = dir_lookup(self, btree_root, name) {
            if e.extent_lba != 0 && e.extent_count > 0 { self.cow_garbage.push((e.extent_lba, e.extent_count)); }
        }
        let mut garbage = Vec::new();
        let nr = BTree::delete_tracked(self, btree_root, name.as_bytes(), &mut garbage).ok_or(VfsError::IOError)?;
        for lba in garbage { self.cow_garbage.push((lba, 1)); }
        if let Some(r) = nr { self.propagate_dir_root(dir_inode, r); }
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn remove_dir(&mut self, dir_inode: u32, name: &str) -> Result<(), VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        if let Some(e) = dir_lookup(self, btree_root, name) {
            if e.is_dir() {
                let count = dir_count(self, e.extent_lba);
                if count > 0 { return Err(VfsError::NotAFile); }
            }
            if e.extent_lba != 0 && e.extent_count > 0 { self.cow_garbage.push((e.extent_lba, e.extent_count)); }
        }
        let mut garbage = Vec::new();
        let nr = BTree::delete_tracked(self, btree_root, name.as_bytes(), &mut garbage).ok_or(VfsError::IOError)?;
        for lba in garbage { self.cow_garbage.push((lba, 1)); }
        if let Some(r) = nr { self.propagate_dir_root(dir_inode, r); }
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn rename(&mut self, dir_inode: u32, old: &str, new: &str) -> Result<(), VfsError> {
        let btree_root = self.inode_cache.get(dir_inode as usize).and_then(|x| x.as_ref()).map(|c| c.root).ok_or(VfsError::NotFound)?;
        let entry = dir_lookup(self, btree_root, old).ok_or(VfsError::NotFound)?;
        let entry_clone = entry.clone();
        let mut garbage = Vec::new();
        let ad = BTree::delete_tracked(self, btree_root, old.as_bytes(), &mut garbage).ok_or(VfsError::IOError)?;
        let ad = ad.unwrap_or(btree_root);
        let mut renamed = entry_clone; renamed.name = new.as_bytes().to_vec();
        let nr = BTree::insert_tracked(self, ad, new.as_bytes(), &{
            let mut tmp = [0u8; DIRENTRY_SIZE]; renamed.serialize(&mut tmp); tmp.to_vec()
        }, &mut garbage).ok_or(VfsError::IOError)?;
        for lba in garbage { self.cow_garbage.push((lba, 1)); }
        self.propagate_dir_root(dir_inode, nr);
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn volume_label(&self) -> Result<String, VfsError> {
        let len = self.sb.label_len as usize;
        Ok(core::str::from_utf8(&self.sb.label[..len]).unwrap_or("").into())
    }

    fn set_volume_label(&mut self, label: &str) -> Result<(), VfsError> {
        let len = label.len().min(32);
        self.sb.label_len = len as u8;
        self.sb.label[..len].copy_from_slice(&label.as_bytes()[..len]);
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn fs_type(&self) -> &'static str { "NE2" }
    fn total_sectors(&self) -> u64 { self.sb.num_blocks * 8 }

    fn snapshot_create(&mut self) -> Result<u64, VfsError> {
        let root_lba = self.sb.root_btree_lba;
        let timestamp = crate::hal::get_ticks();
        let id = self.snapshot_table.create(root_lba, timestamp);
        self.snapshot_dirty = true;
        self.save_sb().map_err(|_| VfsError::IOError)?;
        Ok(id)
    }

    fn snapshot_restore(&mut self, id: u64) -> Result<(), VfsError> {
        let root_lba = self.snapshot_table.restore(id).ok_or(VfsError::NotFound)?;
        self.sb.root_btree_lba = root_lba;
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn snapshot_list(&mut self, buf: &mut [u8]) -> Result<usize, VfsError> {
        let entries = self.snapshot_table.list();
        let entry_size = core::mem::size_of::<SnapshotEntryRaw>();
        let max_entries = buf.len() / entry_size;
        let count = entries.len().min(max_entries);
        for i in 0..count {
            let (id, snap) = &entries[i];
            let raw = SnapshotEntryRaw {
                id: *id,
                root_lba: snap.root_lba,
                timestamp: snap.timestamp,
            };
            let offset = i * entry_size;
            if offset + entry_size <= buf.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        &raw as *const SnapshotEntryRaw as *const u8,
                        buf.as_mut_ptr().add(offset),
                        entry_size,
                    );
                }
            }
        }
        Ok(entries.len())
    }

    fn snapshot_delete(&mut self, id: u64) -> Result<(), VfsError> {
        if !self.snapshot_table.delete(id) {
            return Err(VfsError::NotFound);
        }
        self.snapshot_dirty = true;
        self.save_sb().map_err(|_| VfsError::IOError)
    }

    fn snapshot_extract(&mut self, id: u64, src: &str, dst: &str) -> Result<u64, VfsError> {
        let root = self.snapshot_table.restore(id).ok_or(VfsError::NotFound)?;
        let entry = self.resolve_entry_in(root, src).ok_or(VfsError::NotFound)?;
        if entry.is_dir() { return Err(VfsError::NotAFile); }
        let size = if entry.inline_len > 0 { entry.inline_len as u64 } else { entry.size };
        const MAX_EXTRACT: u64 = 64 * 1024 * 1024;
        if size > MAX_EXTRACT { return Err(VfsError::IOError); }
        let mut data = alloc::vec![0u8; size as usize];
        if size > 0 { self.read_entry_bytes(&entry, 0, &mut data)?; }
        let (parent, name) = self.resolve_parent_inode(dst).ok_or(VfsError::NotFound)?;
        if self.lookup(parent, &name).is_ok() { let _ = self.remove_file(parent, &name); }
        let node = self.create(parent, &name)?;
        if size > 0 { self.write(node.inode, 0, &data)?; }
        self.save_sb().map_err(|_| VfsError::IOError)?;
        Ok(size)
    }

    fn snapshot_purge(&mut self) -> Result<(), VfsError> {
        self.snapshot_table.purge();
        self.snapshot_dirty = true;
        self.save_sb().map_err(|_| VfsError::IOError)
    }
    fn fsck(&mut self, repair: bool, _deep: bool, stats: &mut crate::fs::fsck::FsckStatsRaw) -> Result<(), VfsError> {
        let s = if repair {
            <Self as crate::fs::fsck::FsckTrait>::repair(self)
        } else {
            <Self as crate::fs::fsck::FsckTrait>::check(self)
        };
        *stats = s.to_raw();
        Ok(())
    }
}

/// Formatear una partición con NE2 (mkfs).
/// Escribe superblock, raíz B-tree vacía y nodo de free list inicial.
pub fn mkfs_ne2(io_stack: &IoStack, num_blocks: u64, label: &str) -> Result<(), ()> {
    // Layout inicial:
    //   block 0 = superblock
    //   block 1 = raíz B-tree vacía
    //   block 2 = nodo de free list inicial
    //   blocks 3.. = bloques libres
    if num_blocks < 4 {
        return Err(());
    }

    let mut label_arr = [0u8; 32];
    let len = label.len().min(32);
    label_arr[..len].copy_from_slice(&label.as_bytes()[..len]);

    // 1. Raíz B-tree vacía (block_lba = 1 → sector 8)
    let root_node = BTreeNode::new(NodeType::Leaf);
    let mut buf = [0u8; NODE_SIZE];
    root_node.serialize(&mut buf);

    // 2. Nodo de free list inicial (block_lba = 2 → sector 16)
    let freelist = FreeList::with_range(3, num_blocks - 3);
    let mut flbuf = [0u8; NODE_SIZE];
    freelist.serialize(&mut flbuf, 0);

    // 3. Superblock con checksum
    let mut sb = SuperblockNE2 {
        magic: SUPERBLOCK_MAGIC,
        version: 2,
        root_btree_lba: 1,
        root_version: 1,
        root_timestamp: crate::hal::get_ticks(),
        num_blocks,
        num_used: 3,
        num_free: num_blocks - 3,
        label_len: len as u8,
        label: label_arr,
        flags: 0,
        freelist_lba: 2,
        snapshot_table_lba: 0,
        reserved: [0u8; 403],
    };
    let crc = superblock_crc(&sb).to_le_bytes();
    sb.reserved[..4].copy_from_slice(&crc);

    for i in 0..8usize {
        let mut sec = [0u8; 512];
        sec.copy_from_slice(&buf[i * 512..(i + 1) * 512]);
        io_stack.write_sector(8 + i as u64, &sec)?;

        let mut fsec = [0u8; 512];
        fsec.copy_from_slice(&flbuf[i * 512..(i + 1) * 512]);
        io_stack.write_sector(16 + i as u64, &fsec)?;
    }

    let raw = unsafe { core::slice::from_raw_parts(&sb as *const _ as *const u8, 512) };
    let mut sector = [0u8; 512];
    sector.copy_from_slice(raw);
    io_stack.write_sector(0, &sector)?;

    // Evitar que un superblock/metadatos cacheados de un montaje previo
    // enmascaren el volumen recién formateado.
    invalidate_cache(io_stack, 0, 24);

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────

pub fn register_neodos_v2_tests() {
    use crate::fs::vfs::FileSystem;

    crate::test_case!("neofs_v2_mount_loads_persisted_freelist", {
        let sectors = alloc::vec![[0u8; 512]; 2048];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 256, "TEST").unwrap();

        let fs = NeoDosFsV2::new(io).unwrap();
        // mkfs persiste la free list en el bloque 2 y reserva [0..=2].
        crate::test_eq!(fs.sb.freelist_lba, 2);
        crate::test_eq!(fs.freelist.region_count(), 1);
        crate::test_eq!(fs.freelist.total_free(), 253);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_freelist_survives_remount", {
        let sectors = alloc::vec![[0u8; 512]; 2048];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 256, "TEST").unwrap();

        // Persistencia lazy: tras una mutación la free list queda invalidada.
        let mut fs = NeoDosFsV2::new(io).unwrap();
        fs.create(0, "A.TXT").unwrap();
        crate::test_eq!(fs.sb.freelist_lba, 0);
        drop(fs);

        // Al remontar se reconstruye y el fichero sigue accesible.
        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        crate::test_eq!(fs2.sb.freelist_lba, 0);
        crate::test_true!(fs2.freelist.is_valid(256));
        crate::test_true!(fs2.freelist.total_free() > 0);
        crate::test_true!(fs2.lookup(0, "A.TXT").is_ok());

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_freelist_recovers_without_persisted_list", {
        let sectors = alloc::vec![[0u8; 512]; 2048];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let num_blocks = 256u64;
        mkfs_ne2(&io, num_blocks, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        fs.create(0, "A.TXT").unwrap();
        drop(fs);

        // Borrar el puntero a la free list persistida para forzar la recuperación.
        let io2 = IoStack::new(dev_id);
        let mut sb = read_superblock_raw(&io2).unwrap();
        sb[93..101].copy_from_slice(&0u64.to_le_bytes());
        io2.write_sector(0, &sb).unwrap();
        invalidate_cache(&io2, 0, 1);
        drop(io2);

        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        crate::test_eq!(fs2.sb.freelist_lba, 0);
        crate::test_true!(fs2.freelist.total_free() > 0);
        crate::test_true!(fs2.freelist.is_valid(num_blocks));
        crate::test_true!(fs2.lookup(0, "A.TXT").is_ok());

        // Mutar y remontar de nuevo: se reconstruye y ambos ficheros siguen.
        fs2.create(0, "B.TXT").unwrap();
        drop(fs2);
        let mut fs3 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        crate::test_eq!(fs3.sb.freelist_lba, 0);
        crate::test_true!(fs3.freelist.is_valid(num_blocks));
        crate::test_true!(fs3.lookup(0, "A.TXT").is_ok());
        crate::test_true!(fs3.lookup(0, "B.TXT").is_ok());

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_snapshot_survives_remount", {
        let sectors = alloc::vec![[0u8; 512]; 2048];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        let num_blocks = 256u64;
        mkfs_ne2(&io, num_blocks, "TEST").unwrap();

        // Sin snapshots la tabla no ocupa bloque.
        let mut fs = NeoDosFsV2::new(io).unwrap();
        crate::test_eq!(fs.sb.snapshot_table_lba, 0);
        fs.create(0, "A.TXT").unwrap();

        // Crear un snapshot lo persiste (nodo tipo 4) en un bloque propio.
        let id = fs.snapshot_create().unwrap();
        let snap_lba = fs.sb.snapshot_table_lba;
        crate::test_true!(snap_lba > 0);
        crate::test_true!(fs.freelist.is_valid(num_blocks));
        drop(fs);

        // Remontar: la tabla se recupera del disco.
        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        crate::test_eq!(fs2.sb.snapshot_table_lba, snap_lba);
        crate::test_eq!(fs2.snapshot_table.snapshot_count(), 1);
        let entries = fs2.snapshot_table.list();
        crate::test_eq!(entries.len(), 1);
        crate::test_eq!(entries[0].0, id);
        crate::test_true!(fs2.snapshot_restore(id).is_ok());

        // PURGE vacía la tabla y libera su bloque.
        fs2.snapshot_purge().unwrap();
        crate::test_eq!(fs2.sb.snapshot_table_lba, 0);
        crate::test_true!(fs2.freelist.is_valid(num_blocks));
        drop(fs2);

        let fs3 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        crate::test_eq!(fs3.sb.snapshot_table_lba, 0);
        crate::test_eq!(fs3.snapshot_table.snapshot_count(), 0);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_subdir_persists_across_remount", {
        let sectors = alloc::vec![[0u8; 512]; 2048];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 256, "TEST").unwrap();

        // Crear D/, D/F.TXT, D/E/ y D/E/G.TXT (dos niveles de anidamiento).
        let mut fs = NeoDosFsV2::new(io).unwrap();
        fs.mkdir(0, "D").unwrap();
        let d = fs.lookup(0, "D").unwrap().inode;
        fs.create(d, "F.TXT").unwrap();
        fs.mkdir(d, "E").unwrap();
        let e = fs.lookup(d, "E").unwrap().inode;
        fs.create(e, "G.TXT").unwrap();
        drop(fs);

        // Remontar: cada nivel debe resolver a su raíz actual (no la obsoleta).
        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        let d2 = fs2.lookup(0, "D").unwrap().inode;
        crate::test_true!(fs2.lookup(d2, "F.TXT").is_ok());
        let e2 = fs2.lookup(d2, "E").unwrap().inode;
        crate::test_true!(fs2.lookup(e2, "G.TXT").is_ok());

        // Modificar un subdirectorio y remontar de nuevo.
        fs2.create(d2, "H.TXT").unwrap();
        drop(fs2);
        let mut fs3 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        let d3 = fs3.lookup(0, "D").unwrap().inode;
        crate::test_true!(fs3.lookup(d3, "H.TXT").is_ok());
        crate::test_true!(fs3.lookup(d3, "F.TXT").is_ok());

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_cow_reclaims_garbage", {
        let sectors = alloc::vec![[0u8; 512]; 4096];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 512, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        fs.create(0, "A.TXT").unwrap();
        let inode = fs.lookup(0, "A.TXT").unwrap().inode;
        let data = alloc::vec![0xABu8; 4096 * 10];

        fs.write(inode, 0, &data).unwrap();
        fs.set_volume_label("T").unwrap();
        let f1 = fs.freelist.total_free();

        // Reescribir el mismo tamaño debe ser neto cero: se allocan nuevos
        // bloques/extents y se reclaman los antiguos.
        for _ in 0..5 {
            fs.write(inode, 0, &data).unwrap();
            fs.set_volume_label("T").unwrap();
        }
        let f2 = fs.freelist.total_free();
        crate::test_eq!(f1, f2);
        crate::test_true!(fs.freelist.is_valid(512));
        crate::test_eq!(fs.snapshot_table.snapshot_count(), 0);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_cow_garbage_gated_by_snapshots", {
        let sectors = alloc::vec![[0u8; 512]; 4096];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 512, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        fs.create(0, "A.TXT").unwrap();
        let inode = fs.lookup(0, "A.TXT").unwrap().inode;
        let data = alloc::vec![0xCDu8; 4096 * 10];
        fs.write(inode, 0, &data).unwrap();
        let _snap = fs.snapshot_create().unwrap();

        // Con un snapshot presente los bloques reemplazados NO se liberan.
        let f_before = fs.freelist.total_free();
        for _ in 0..3 {
            fs.write(inode, 0, &data).unwrap();
            fs.set_volume_label("T").unwrap();
        }
        let f_after = fs.freelist.total_free();
        crate::test_true!(f_after < f_before);

        // PURGE vacía la tabla y permite reclamar lo retenido.
        fs.snapshot_purge().unwrap();
        let f_purged = fs.freelist.total_free();
        crate::test_true!(f_purged > f_after);
        crate::test_true!(fs.freelist.is_valid(512));

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_dir_60_entries_multileaf", {
        // 60 entradas de directorio (128 B c/u) no caben en una hoja de 4 KB:
        // fuerza árbol de directorio multi-hoja y su persistencia.
        let sectors = alloc::vec![[0u8; 512]; 8192];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 1024, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        for i in 0..60u32 {
            fs.create(0, &alloc::format!("F{:03}.TXT", i)).unwrap();
        }
        for i in 0..60u32 {
            crate::test_true!(fs.lookup(0, &alloc::format!("F{:03}.TXT", i)).is_ok());
        }
        let mut n = 0usize;
        while fs.readdir(0, n).unwrap().is_some() { n += 1; }
        crate::test_eq!(n, 60);
        drop(fs);

        // Remontar: el árbol multi-hoja debe persistir.
        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        for i in 0..60u32 {
            crate::test_true!(fs2.lookup(0, &alloc::format!("F{:03}.TXT", i)).is_ok());
        }
        let mut n = 0usize;
        while fs2.readdir(0, n).unwrap().is_some() { n += 1; }
        crate::test_eq!(n, 60);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_snapshot_delete_persists", {
        let sectors = alloc::vec![[0u8; 512]; 2048];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 256, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        let id0 = fs.snapshot_create().unwrap();
        let id1 = fs.snapshot_create().unwrap();
        let id2 = fs.snapshot_create().unwrap();
        crate::test_eq!(fs.snapshot_table.snapshot_count(), 3);

        // Borrar el intermedio; los demás siguen.
        crate::test_true!(fs.snapshot_delete(id1).is_ok());
        crate::test_true!(fs.snapshot_delete(id1).is_err());
        crate::test_eq!(fs.snapshot_table.snapshot_count(), 2);
        drop(fs);

        // Remontar: el borrado persiste y las generaciones no se reutilizan.
        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        crate::test_eq!(fs2.snapshot_table.snapshot_count(), 2);
        crate::test_true!(fs2.snapshot_restore(id1).is_err());
        crate::test_true!(fs2.snapshot_restore(id0).is_ok());
        crate::test_true!(fs2.snapshot_restore(id2).is_ok());
        let id3 = fs2.snapshot_create().unwrap();
        crate::test_true!(id3 > id2);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_snapshot_extract_file", {
        let sectors = alloc::vec![[0u8; 512]; 8192];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 1024, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        // A.TXT inline (8 B) y B.TXT con extents (5000 B).
        fs.create(0, "A.TXT").unwrap();
        let a = fs.lookup(0, "A.TXT").unwrap().inode;
        fs.write(a, 0, b"hello v1").unwrap();
        fs.create(0, "B.TXT").unwrap();
        let b = fs.lookup(0, "B.TXT").unwrap().inode;
        let big = alloc::vec![0xABu8; 5000];
        fs.write(b, 0, &big).unwrap();

        let id = fs.snapshot_create().unwrap();

        // Modificar A y borrar B en el árbol actual.
        fs.write(a, 0, b"hello v2").unwrap();
        fs.remove_file(0, "B.TXT").unwrap();
        crate::test_true!(fs.lookup(0, "B.TXT").is_err());

        // Extraer las versiones del snapshot a ficheros nuevos.
        crate::test_eq!(fs.snapshot_extract(id, "\\A.TXT", "\\A_OLD.TXT").unwrap(), 8);
        crate::test_eq!(fs.snapshot_extract(id, "\\B.TXT", "\\B_REC.TXT").unwrap(), 5000);

        // A_OLD = "hello v1"; B_REC = 5000 bytes 0xAB; A.TXT actual = "hello v2".
        let a_old = fs.lookup(0, "A_OLD.TXT").unwrap().inode;
        let mut buf = [0u8; 16];
        let r = fs.read(a_old, 0, &mut buf).unwrap();
        crate::test_eq!(&buf[..r], b"hello v1");
        let b_rec = fs.lookup(0, "B_REC.TXT").unwrap().inode;
        let mut buf2 = alloc::vec![0u8; 5000];
        crate::test_eq!(fs.read(b_rec, 0, &mut buf2).unwrap(), 5000);
        crate::test_true!(buf2.iter().all(|&x| x == 0xAB));
        let a_cur = fs.lookup(0, "A.TXT").unwrap().inode;
        let mut buf = [0u8; 16];
        let r = fs.read(a_cur, 0, &mut buf).unwrap();
        crate::test_eq!(&buf[..r], b"hello v2");

        // Extraer con id inexistente o fichero inexistente falla.
        crate::test_true!(fs.snapshot_extract(9999, "\\A.TXT", "\\X.TXT").is_err());
        crate::test_true!(fs.snapshot_extract(id, "\\NOPE.TXT", "\\X.TXT").is_err());

        // Persistencia: tras remontar siguen los extraídos y el borrado.
        drop(fs);
        let mut fs2 = NeoDosFsV2::new(IoStack::new(dev_id)).unwrap();
        let a2 = fs2.lookup(0, "A_OLD.TXT").unwrap().inode;
        let mut buf = [0u8; 16];
        let r = fs2.read(a2, 0, &mut buf).unwrap();
        crate::test_eq!(&buf[..r], b"hello v1");
        crate::test_true!(fs2.lookup(0, "B.TXT").is_err());
        crate::test_true!(fs2.lookup(0, "B_REC.TXT").is_ok());

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

    crate::test_case!("neofs_v2_metadata_bench", {
        // Benchmark de metadatos: mide ticks de N ciclos create+write+remove.
        let sectors = alloc::vec![[0u8; 512]; 8192];
        let dev_id = crate::fs::fsck::register_test_device(sectors);
        let io = IoStack::new(dev_id);
        mkfs_ne2(&io, 1024, "TEST").unwrap();

        let mut fs = NeoDosFsV2::new(io).unwrap();
        const N: u32 = 200;
        let mut best = u64::MAX;
        for _round in 0..5 {
            let t0 = unsafe { crate::hal::raw::raw_read_tsc() };
            for i in 0..N {
                let name = alloc::format!("B{:04}.TXT", i);
                fs.create(0, &name).unwrap();
                let ino = fs.lookup(0, &name).unwrap().inode;
                fs.write(ino, 0, b"hello world benchmark payload").unwrap();
                fs.remove_file(0, &name).unwrap();
            }
            let t1 = unsafe { crate::hal::raw::raw_read_tsc() };
            best = best.min(t1.wrapping_sub(t0));
        }
        crate::serial_println!("[BENCH] metadata {} create+write+remove = {} tsc (best of 5)", N, best);

        let _ = crate::globals::BLOCK_DEVICES.lock().force_remove(dev_id);
    });

}
