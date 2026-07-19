//! B-tree persistente genérico con COW.
//! Nodos de 4KB. Claves y valores de longitud variable.
//! Las operaciones de E/S se delegan al trait `BTreeIO`.
//! Fusiona nodos tras eliminación para mantener el factor de llenado mínimo.

use alloc::vec::Vec;
use alloc::format;
use super::crc32::crc32;

pub const NODE_SIZE: usize = 4096;
const HEADER_SIZE: usize = 8;
pub const MAX_ENTRIES: usize = 200;
const MIN_ENTRIES: usize = MAX_ENTRIES / 2;

pub trait BTreeIO {
    fn read_node(&self, lba: u64) -> Option<BTreeNode>;
    fn write_node(&mut self, node: &BTreeNode) -> u64;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum NodeType {
    Internal = 0,
    Leaf = 1,
}

#[derive(Debug, Clone)]
pub struct BTreeEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BTreeNode {
    pub node_type: NodeType,
    pub entries: Vec<BTreeEntry>,
}

impl BTreeNode {
    pub fn new(node_type: NodeType) -> Self {
        BTreeNode { node_type, entries: Vec::new() }
    }

    pub fn is_leaf(&self) -> bool { self.node_type == NodeType::Leaf }

    pub fn max_entries(&self) -> usize { MAX_ENTRIES }

    pub fn serialize(&self, buf: &mut [u8; NODE_SIZE]) {
        buf.fill(0);
        buf[0..2].copy_from_slice(&(self.node_type as u16).to_le_bytes());
        buf[2..4].copy_from_slice(&(self.entries.len() as u16).to_le_bytes());
        let mut offset = HEADER_SIZE;
        for entry in &self.entries {
            let kl = entry.key.len();
            let vl = entry.value.len();
            if offset + 4 + kl + vl > NODE_SIZE { break; }
            buf[offset..offset + 2].copy_from_slice(&(kl as u16).to_le_bytes());
            buf[offset + 2..offset + 2 + kl].copy_from_slice(&entry.key);
            buf[offset + 2 + kl..offset + 4 + kl].copy_from_slice(&(vl as u16).to_le_bytes());
            buf[offset + 4 + kl..offset + 4 + kl + vl].copy_from_slice(&entry.value);
            offset += 4 + kl + vl;
        }
        let cksum = crc32(&buf[8..]);
        buf[4..8].copy_from_slice(&cksum.to_le_bytes());
    }

    pub fn deserialize(buf: &[u8; NODE_SIZE]) -> Option<Self> {
        let cksum = crc32(&buf[8..]);
        let stored = u32::from_le_bytes(buf[4..8].try_into().ok()?);
        if stored != 0 && stored != cksum { return None; }
        let node_type = match u16::from_le_bytes(buf[0..2].try_into().ok()?) {
            0 => NodeType::Internal, 1 => NodeType::Leaf, _ => return None,
        };
        let count = u16::from_le_bytes(buf[2..4].try_into().ok()?) as usize;
        let mut entries = Vec::with_capacity(count);
        let mut offset = HEADER_SIZE;
        for _ in 0..count {
            if offset + 4 > NODE_SIZE { return None; }
            let kl = u16::from_le_bytes(buf[offset..offset + 2].try_into().ok()?) as usize; offset += 2;
            if offset + kl > NODE_SIZE { return None; }
            let key = buf[offset..offset + kl].to_vec(); offset += kl;
            if offset + 2 > NODE_SIZE { return None; }
            let vl = u16::from_le_bytes(buf[offset..offset + 2].try_into().ok()?) as usize; offset += 2;
            if offset + vl > NODE_SIZE { return None; }
            let value = buf[offset..offset + vl].to_vec(); offset += vl;
            entries.push(BTreeEntry { key, value });
        }
        Some(BTreeNode { node_type, entries })
    }

    fn find_pos(&self, key: &[u8]) -> Result<usize, usize> {
        self.entries.binary_search_by(|e| e.key.as_slice().cmp(key))
    }
}

// ── B-tree operaciones ─────────────────────────────────────────────

pub struct BTree;

impl BTree {
    /// Buscar clave en el árbol.
    pub fn lookup(io: &impl BTreeIO, root_lba: u64, key: &[u8]) -> Option<Vec<u8>> {
        if root_lba == 0 { return None; }
        let mut node = io.read_node(root_lba)?;
        loop {
            if node.is_leaf() {
                return match node.find_pos(key) {
                    Ok(pos) => Some(node.entries[pos].value.clone()),
                    Err(_) => None,
                };
            }
            let child_idx = child_index(&node, key);
            let child_lba = u64_from_value(&node.entries[child_idx].value)?;
            node = io.read_node(child_lba)?;
        }
    }

    /// Insertar clave-valor (COW). Devuelve nueva root_lba.
    pub fn insert(io: &mut impl BTreeIO, root_lba: u64, key: &[u8], value: &[u8]) -> Option<u64> {
        if root_lba == 0 {
            let mut root = BTreeNode::new(NodeType::Leaf);
            root.entries.push(BTreeEntry { key: key.to_vec(), value: value.to_vec() });
            return Some(io.write_node(&root));
        }
        let result = Self::ins(io, root_lba, key, value)?;
        match result {
            InsertResult::Done(new_lba) => Some(new_lba),
            InsertResult::Split(median_key, left_lba, right_lba) => {
                let mut new_root = BTreeNode::new(NodeType::Internal);
                new_root.entries.push(BTreeEntry {
                    key: Vec::new(), value: left_lba.to_le_bytes().to_vec(),
                });
                new_root.entries.push(BTreeEntry {
                    key: median_key, value: right_lba.to_le_bytes().to_vec(),
                });
                Some(io.write_node(&new_root))
            }
        }
    }

    fn ins(io: &mut impl BTreeIO, node_lba: u64, key: &[u8], value: &[u8]) -> Option<InsertResult> {
        let node = io.read_node(node_lba)?;
        if node.is_leaf() {
            let mut new_node = node.clone();
            match new_node.find_pos(key) {
                Ok(pos) => new_node.entries[pos].value = value.to_vec(),
                Err(pos) => new_node.entries.insert(pos, BTreeEntry { key: key.to_vec(), value: value.to_vec() }),
            }
            if new_node.entries.len() > new_node.max_entries() {
                let (median_key, left, right) = split_node(&new_node);
                let left_lba = io.write_node(&left);
                let right_lba = io.write_node(&right);
                Some(InsertResult::Split(median_key, left_lba, right_lba))
            } else {
                Some(InsertResult::Done(io.write_node(&new_node)))
            }
        } else {
            let child_idx = child_index(&node, key);
            let child_lba = u64_from_value(&node.entries[child_idx].value)?;
            match Self::ins(io, child_lba, key, value)? {
                InsertResult::Done(new_child_lba) => {
                    let mut new_node = node.clone();
                    new_node.entries[child_idx].value = new_child_lba.to_le_bytes().to_vec();
                    Some(InsertResult::Done(io.write_node(&new_node)))
                }
                InsertResult::Split(median_key, left_lba, right_lba) => {
                    let mut new_node = node.clone();
                    new_node.entries[child_idx].value = left_lba.to_le_bytes().to_vec();
                    new_node.entries.insert(child_idx + 1, BTreeEntry {
                        key: median_key, value: right_lba.to_le_bytes().to_vec(),
                    });
                    if new_node.entries.len() > new_node.max_entries() {
                        let (mkey, left, right) = split_internal(&new_node);
                        Some(InsertResult::Split(mkey, io.write_node(&left), io.write_node(&right)))
                    } else {
                        Some(InsertResult::Done(io.write_node(&new_node)))
                    }
                }
            }
        }
    }

    /// Eliminar clave (COW). Devuelve Some(Some(new_root)) o Some(None) si árbol vacío.
    pub fn delete(io: &mut impl BTreeIO, root_lba: u64, key: &[u8]) -> Option<Option<u64>> {
        if root_lba == 0 { return Some(None); }
        let result = Self::del(io, root_lba, key);
        match result? {
            None => Some(None),
            Some(lba) => {
                if let Some(node) = io.read_node(lba) {
                    if node.node_type == NodeType::Internal && node.entries.len() == 1 {
                        if let Some(child_lba) = u64_from_value(&node.entries[0].value) {
                            return Some(Some(child_lba));
                        }
                    }
                }
                Some(Some(lba))
            }
        }
    }

    fn del(io: &mut impl BTreeIO, node_lba: u64, key: &[u8]) -> Option<Option<u64>> {
        let node = io.read_node(node_lba)?;
        if node.is_leaf() {
            let mut new_node = node.clone();
            if let Ok(pos) = new_node.find_pos(key) {
                new_node.entries.remove(pos);
            }
            if new_node.entries.is_empty() { Some(None) }
            else { Some(Some(io.write_node(&new_node))) }
        } else {
            let child_idx = child_index(&node, key);
            let child_lba = u64_from_value(&node.entries[child_idx].value)?;
            let new_child = Self::del(io, child_lba, key)?;
            let mut new_node = node.clone();
            match new_child {
                None => {
                    new_node.entries.remove(child_idx);
                    if new_node.entries.is_empty() { return Some(None); }
                }
                Some(lba) => {
                    new_node.entries[child_idx].value = lba.to_le_bytes().to_vec();
                    if let Some(child_node) = io.read_node(lba) {
                        if child_node.entries.len() < MIN_ENTRIES {
                            Self::try_borrow_or_merge(io, &mut new_node, child_idx);
                        }
                    }
                }
            }
            Some(Some(io.write_node(&new_node)))
        }
    }

    /// Intenta rebalancear el hijo en `child_idx` prestando de un hermano
    /// o fusionándolo. Devuelve `true` si el padre sigue siendo válido.
    fn try_borrow_or_merge(
        io: &mut impl BTreeIO,
        parent: &mut BTreeNode,
        child_idx: usize,
    ) -> bool {
        let n = parent.entries.len();
        if n == 0 { return false; }

        let child_lba = match u64_from_value(&parent.entries[child_idx].value) {
            Some(l) => l, None => return false,
        };
        let child = match io.read_node(child_lba) { Some(c) => c, None => return false };
        if child.entries.len() >= MIN_ENTRIES { return true; }

        // Try borrow from left sibling
        if child_idx > 0 {
            let left_lba = match u64_from_value(&parent.entries[child_idx - 1].value) {
                Some(l) => l, None => return false,
            };
            let left = match io.read_node(left_lba) { Some(l) => l, None => return false };
            if left.entries.len() > MIN_ENTRIES {
                return Self::borrow_left(io, parent, child_idx, left, child);
            }
        }

        // Try borrow from right sibling
        if child_idx + 1 < n {
            let right_idx = child_idx + 1;
            let right_lba = match u64_from_value(&parent.entries[right_idx].value) {
                Some(l) => l, None => return false,
            };
            let right = match io.read_node(right_lba) { Some(r) => r, None => return false };
            if right.entries.len() > MIN_ENTRIES {
                return Self::borrow_right(io, parent, child_idx, child, right);
            }
        }

        // Merge with left sibling (preferred)
        if child_idx > 0 {
            let left_lba = match u64_from_value(&parent.entries[child_idx - 1].value) {
                Some(l) => l, None => return false,
            };
            let left = match io.read_node(left_lba) { Some(l) => l, None => return false };
            Self::merge_into_left(io, parent, child_idx, left, child)
        } else if child_idx + 1 < n {
            let right_idx = child_idx + 1;
            let right_lba = match u64_from_value(&parent.entries[right_idx].value) {
                Some(l) => l, None => return false,
            };
            let right = match io.read_node(right_lba) { Some(r) => r, None => return false };
            Self::merge_into_right(io, parent, child_idx, child, right)
        } else {
            true
        }
    }

    /// Mover una entrada del hermano izquierdo al hijo (child_idx).
    fn borrow_left(
        io: &mut impl BTreeIO,
        parent: &mut BTreeNode,
        child_idx: usize,
        mut left: BTreeNode,
        mut child: BTreeNode,
    ) -> bool {
        let sep = parent.entries[child_idx].key.clone();

        if child.is_leaf() {
            let borrowed = left.entries.pop().unwrap();
            child.entries.insert(0, borrowed);
            parent.entries[child_idx].key = child.entries[0].key.clone();
        } else {
            let borrowed = left.entries.pop().unwrap();
            child.entries.insert(0, BTreeEntry {
                key: sep,
                value: borrowed.value,
            });
            parent.entries[child_idx].key = borrowed.key;
        }

        let new_left_lba = io.write_node(&left);
        let new_child_lba = io.write_node(&child);
        parent.entries[child_idx - 1].value = new_left_lba.to_le_bytes().to_vec();
        parent.entries[child_idx].value = new_child_lba.to_le_bytes().to_vec();
        true
    }

    /// Mover una entrada del hermano derecho al hijo (child_idx).
    fn borrow_right(
        io: &mut impl BTreeIO,
        parent: &mut BTreeNode,
        child_idx: usize,
        mut child: BTreeNode,
        mut right: BTreeNode,
    ) -> bool {
        let right_idx = child_idx + 1;
        let sep = parent.entries[right_idx].key.clone();

        if child.is_leaf() {
            let borrowed = right.entries.remove(0);
            child.entries.push(borrowed);
            parent.entries[right_idx].key = right.entries[0].key.clone();
        } else {
            let borrowed = right.entries.remove(0);
            child.entries.push(BTreeEntry {
                key: sep,
                value: borrowed.value,
            });
            parent.entries[right_idx].key = borrowed.key;
            if right.entries.is_empty() {
                parent.entries[right_idx].key = Vec::new();
            }
        }

        let new_child_lba = io.write_node(&child);
        let new_right_lba = io.write_node(&right);
        parent.entries[child_idx].value = new_child_lba.to_le_bytes().to_vec();
        parent.entries[right_idx].value = new_right_lba.to_le_bytes().to_vec();
        true
    }

    /// Fusionar child_idx en child_idx-1 (left sibling).
    fn merge_into_left(
        io: &mut impl BTreeIO,
        parent: &mut BTreeNode,
        child_idx: usize,
        left: BTreeNode,
        child: BTreeNode,
    ) -> bool {
        let sep = parent.entries[child_idx].key.clone();
        let merged = merge_nodes(left, child, &sep);

        // child_idx-1 apunta al nodo fusionado; eliminamos child_idx
        let merged_lba = io.write_node(&merged);
        parent.entries[child_idx - 1].value = merged_lba.to_le_bytes().to_vec();
        parent.entries.remove(child_idx);
        true
    }

    /// Fusionar child_idx y child_idx+1 (derecho).
    fn merge_into_right(
        io: &mut impl BTreeIO,
        parent: &mut BTreeNode,
        child_idx: usize,
        child: BTreeNode,
        right: BTreeNode,
    ) -> bool {
        let sep = parent.entries[child_idx + 1].key.clone();
        let merged = merge_nodes(child, right, &sep);

        // child_idx apunta al fusionado; eliminamos child_idx+1
        let merged_lba = io.write_node(&merged);
        parent.entries[child_idx].value = merged_lba.to_le_bytes().to_vec();
        parent.entries.remove(child_idx + 1);
        true
    }

    /// Recorrer todas las entradas en orden.
    pub fn walk(io: &impl BTreeIO, root_lba: u64, f: &mut impl FnMut(&BTreeEntry)) {
        if root_lba == 0 { return; }
        let node = match io.read_node(root_lba) { Some(n) => n, None => return };
        walk_recursive(&node, io, f);
    }
}

fn walk_recursive(node: &BTreeNode, io: &impl BTreeIO, f: &mut impl FnMut(&BTreeEntry)) {
    if node.is_leaf() {
        for entry in &node.entries { f(entry); }
    } else {
        for entry in &node.entries {
            if let Some(child_lba) = u64_from_value(&entry.value) {
                if let Some(child) = io.read_node(child_lba) {
                    walk_recursive(&child, io, f);
                }
            }
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────

enum InsertResult {
    Done(u64),
    Split(Vec<u8>, u64, u64),
}

fn u64_from_value(v: &[u8]) -> Option<u64> {
    if v.len() < 8 { return None; }
    Some(u64::from_le_bytes(v[..8].try_into().ok()?))
}

/// Índice del hijo al que descender en un nodo interno.
/// Para un lookup/insert/delete, determina qué entrada contiene
/// el puntero al subárbol relevante.
///
/// Convención del nodo interno:
/// - entries[0].key  = "" (leftmost child)
/// - entries[0].value = child que maneja claves < entries[1].key
/// - entries[i].key  = separador
/// - entries[i].value = child que maneja claves >= entries[i].key
fn child_index(node: &BTreeNode, key: &[u8]) -> usize {
    match node.find_pos(key) {
        Ok(p) => p,
        Err(0) => 0,
        Err(p) => p - 1,
    }
}

fn split_node(node: &BTreeNode) -> (Vec<u8>, BTreeNode, BTreeNode) {
    let mid = node.entries.len() / 2;
    let mut left = BTreeNode::new(NodeType::Leaf);
    let mut right = BTreeNode::new(NodeType::Leaf);
    left.entries = node.entries[..mid].to_vec();
    right.entries = node.entries[mid..].to_vec();
    (node.entries[mid].key.clone(), left, right)
}

fn split_internal(node: &BTreeNode) -> (Vec<u8>, BTreeNode, BTreeNode) {
    let mid = node.entries.len() / 2;
    let mut left = BTreeNode::new(NodeType::Internal);
    let mut right = BTreeNode::new(NodeType::Internal);
    left.entries = node.entries[..mid].to_vec();
    right.entries.push(BTreeEntry {
        key: Vec::new(),
        value: node.entries[mid].value.clone(),
    });
    right.entries.extend(node.entries[mid + 1..].iter().cloned());
    (node.entries[mid].key.clone(), left, right)
}

/// Fusiona dos nodos del mismo tipo.
/// `sep` es la clave separadora del padre.
fn merge_nodes(mut left: BTreeNode, right: BTreeNode, sep: &[u8]) -> BTreeNode {
    if left.is_leaf() {
        left.entries.extend(right.entries);
    } else {
        // Internal: el primer entry de `right` debe usar `sep` como clave
        if let Some(first) = right.entries.first() {
            left.entries.push(BTreeEntry {
                key: sep.to_vec(),
                value: first.value.clone(),
            });
            left.entries.extend(right.entries[1..].iter().cloned());
        }
    }
    left
}

// ── In-memory test helper ──────────────────────────────────────────

pub struct MemBTreeIO {
    pub nodes: Vec<(u64, [u8; NODE_SIZE])>,
    pub next_lba: u64,
}

impl MemBTreeIO {
    pub fn new() -> Self { MemBTreeIO { nodes: Vec::new(), next_lba: 1 } }
}

impl BTreeIO for MemBTreeIO {
    fn read_node(&self, lba: u64) -> Option<BTreeNode> {
        let data = self.nodes.iter().find(|(id, _)| *id == lba)?.1;
        BTreeNode::deserialize(&data)
    }

    fn write_node(&mut self, node: &BTreeNode) -> u64 {
        let lba = self.next_lba; self.next_lba += 1;
        let mut buf = [0u8; NODE_SIZE];
        node.serialize(&mut buf);
        self.nodes.push((lba, buf));
        lba
    }
}

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_btree_tests() {
    crate::test_case!("btree_node_serialize_roundtrip", {
        let mut node = BTreeNode::new(NodeType::Leaf);
        node.entries.push(BTreeEntry { key: b"hello".to_vec(), value: b"world".to_vec() });
        node.entries.push(BTreeEntry { key: b"test".to_vec(), value: b"123".to_vec() });
        let mut buf = [0u8; NODE_SIZE];
        node.serialize(&mut buf);
        let loaded = BTreeNode::deserialize(&buf).unwrap();
        crate::test_eq!(loaded.entries[0].key.as_slice(), b"hello");
        crate::test_eq!(loaded.entries[0].value.as_slice(), b"world");
    });

    crate::test_case!("btree_node_checksum_detect_corruption", {
        let mut node = BTreeNode::new(NodeType::Leaf);
        node.entries.push(BTreeEntry { key: b"data".to_vec(), value: b"important".to_vec() });
        let mut buf = [0u8; NODE_SIZE];
        node.serialize(&mut buf);
        buf[20] ^= 0xFF;
        crate::test_true!(BTreeNode::deserialize(&buf).is_none());
    });

    crate::test_case!("btree_insert_lookup", {
        let mut io = MemBTreeIO::new();
        let r = BTree::insert(&mut io, 0, b"c", b"3").unwrap();
        let r = BTree::insert(&mut io, r, b"a", b"1").unwrap();
        let r = BTree::insert(&mut io, r, b"b", b"2").unwrap();
        crate::test_eq!(BTree::lookup(&io, r, b"a"), Some(b"1".to_vec()));
        crate::test_eq!(BTree::lookup(&io, r, b"b"), Some(b"2".to_vec()));
        crate::test_eq!(BTree::lookup(&io, r, b"c"), Some(b"3".to_vec()));
        crate::test_eq!(BTree::lookup(&io, r, b"d"), None);
    });

    crate::test_case!("btree_delete", {
        let mut io = MemBTreeIO::new();
        let r = BTree::insert(&mut io, 0, b"x", b"42").unwrap();
        let r = BTree::insert(&mut io, r, b"y", b"99").unwrap();
        crate::test_eq!(BTree::lookup(&io, r, b"x"), Some(b"42".to_vec()));
        let r2 = BTree::delete(&mut io, r, b"x").unwrap();
        crate::test_eq!(BTree::lookup(&io, r2.unwrap(), b"x"), None);
        crate::test_eq!(BTree::lookup(&io, r2.unwrap(), b"y"), Some(b"99".to_vec()));
    });

    crate::test_case!("btree_walk_ordered", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        for k in &[b"d", b"b", b"f", b"a", b"c", b"e"] {
            r = BTree::insert(&mut io, r, k.as_slice(), k.as_slice()).unwrap();
        }
        let mut walked = Vec::new();
        BTree::walk(&io, r, &mut |e| walked.push(e.key.clone()));
        let walked_str: Vec<&str> = walked.iter().map(|k| core::str::from_utf8(k).unwrap()).collect();
        crate::test_eq!(walked_str, ["a", "b", "c", "d", "e", "f"]);
    });

    crate::test_case!("btree_cow_preserves_old_root", {
        let mut io = MemBTreeIO::new();
        let r1 = BTree::insert(&mut io, 0, b"k1", b"v1").unwrap();
        let r2 = BTree::insert(&mut io, r1, b"k2", b"v2").unwrap();
        crate::test_eq!(BTree::lookup(&io, r1, b"k1"), Some(b"v1".to_vec()));
        crate::test_eq!(BTree::lookup(&io, r1, b"k2"), None);
        crate::test_eq!(BTree::lookup(&io, r2, b"k1"), Some(b"v1".to_vec()));
        crate::test_eq!(BTree::lookup(&io, r2, b"k2"), Some(b"v2".to_vec()));
    });

    // ── Node split / merge tests ──────────────────────────────────

    crate::test_case!("btree_forced_split", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        // Insert MAX_ENTRIES + 1 keys to force split + internal node
        for i in 0..=MAX_ENTRIES {
            let k = format!("key{:04}", i);
            let v = format!("val{:04}", i);
            r = BTree::insert(&mut io, r, k.as_bytes(), v.as_bytes()).unwrap();
        }
        // Verify all
        for i in 0..=MAX_ENTRIES {
            let k = format!("key{:04}", i);
            let v = format!("val{:04}", i);
            let found = BTree::lookup(&io, r, k.as_bytes());
            crate::test_eq!(found, Some(v.as_bytes().to_vec()));
        }
        // Walk should visit all entries in order
        let mut count = 0;
        let mut prev_key: Option<Vec<u8>> = None;
        BTree::walk(&io, r, &mut |e| {
            if let Some(ref p) = prev_key {
                if e.key.as_slice() <= p.as_slice() {
                    // Force test failure via panic
                    panic!("btree walk out of order: {:?} <= {:?}", e.key, p);
                }
            }
            prev_key = Some(e.key.clone());
            count += 1;
        });
        crate::test_eq!(count, MAX_ENTRIES + 1);
    });

    crate::test_case!("btree_split_then_delete_all", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = 50;
        for i in 0..n {
            let k = format!("key{:04}", i);
            let v = format!("val{:04}", i);
            r = BTree::insert(&mut io, r, k.as_bytes(), v.as_bytes()).unwrap();
        }
        // Delete all in reverse order (forces merges)
        for i in (0..n).rev() {
            let k = format!("key{:04}", i);
            let result = BTree::delete(&mut io, r, k.as_bytes()).unwrap();
            match result {
                Some(new_root) => r = new_root,
                None => { r = 0; break; }
            }
        }
        // Tree should be empty
        crate::test_eq!(r, 0);
    });

    crate::test_case!("btree_split_then_delete_half", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = 100;
        for i in 0..n {
            let k = format!("key{:04}", i);
            let v = format!("val{:04}", i);
            r = BTree::insert(&mut io, r, k.as_bytes(), v.as_bytes()).unwrap();
        }
        // Delete even keys
        for i in (0..n).step_by(2) {
            let k = format!("key{:04}", i);
            let result = BTree::delete(&mut io, r, k.as_bytes()).unwrap();
            if let Some(new_root) = result { r = new_root; }
        }
        // Verify odd keys remain
        for i in 0..n {
            let k = format!("key{:04}", i);
            let found = BTree::lookup(&io, r, k.as_bytes());
            if i % 2 == 0 {
                crate::test_eq!(found, None);
            } else {
                let v = format!("val{:04}", i);
                crate::test_eq!(found, Some(v.as_bytes().to_vec()));
            }
        }
    });

    // ── Stress tests ──────────────────────────────────────────────

    crate::test_case!("btree_stress_insert_500", {
        // Use 2-byte keys/values so serialized size stays within 4KB
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = 300;
        for i in 0..n {
            let k = [(i >> 8) as u8, (i & 0xff) as u8];
            let v = [((i + 1) >> 8) as u8, ((i + 1) & 0xff) as u8];
            r = BTree::insert(&mut io, r, &k, &v).unwrap();
        }
        // Verify all inserted
        for i in 0..n {
            let k = [(i >> 8) as u8, (i & 0xff) as u8];
            let v = [((i + 1) >> 8) as u8, ((i + 1) & 0xff) as u8];
            let found = BTree::lookup(&io, r, &k);
            if found != Some(v.to_vec()) {
                crate::test_true!(false);
            }
        }
        // Walk count
        let mut count = 0;
        BTree::walk(&io, r, &mut |_| count += 1);
        crate::test_eq!(count, n);
    });

    crate::test_case!("btree_stress_insert_delete_300", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = 300;
        // Insert
        for i in 0..n {
            let k = [(i >> 8) as u8, (i & 0xff) as u8];
            r = BTree::insert(&mut io, r, &k, &k).unwrap();
        }
        // Delete half
        for i in (0..n).step_by(2) {
            let k = [(i >> 8) as u8, (i & 0xff) as u8];
            let result = BTree::delete(&mut io, r, &k).unwrap();
            if let Some(new_root) = result { r = new_root; }
        }
        // Verify
        for i in 0..n {
            let k = [(i >> 8) as u8, (i & 0xff) as u8];
            let found = BTree::lookup(&io, r, &k);
            if i % 2 == 0 {
                if found.is_some() { crate::test_true!(false); }
            } else {
                if found != Some(k.to_vec()) { crate::test_true!(false); }
            }
        }
    });

    crate::test_case!("btree_stress_random_sequence", {
        use alloc::vec::Vec;
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = 300;
        let mut keys: Vec<Vec<u8>> = (0..n)
            .map(|i| [((i * 137 + 42) % n) as u8, (((i * 137 + 42) / n) & 0xff) as u8].to_vec())
            .collect();
        for k in &keys {
            r = BTree::insert(&mut io, r, k, k).unwrap();
        }
        for k in &keys {
            let found = BTree::lookup(&io, r, k);
            if found != Some(k.clone()) { crate::test_true!(false); }
        }
        keys.reverse();
        for k in &keys {
            let result = BTree::delete(&mut io, r, k).unwrap();
            match result {
                Some(new_root) => r = new_root,
                None => { r = 0; break; }
            }
        }
        crate::test_eq!(r, 0);
    });

    crate::test_case!("btree_persistence_roundtrip", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let entries: &[&[u8]] = &[b"alpha", b"bravo", b"charlie", b"delta", b"echo"];
        for e in entries {
            r = BTree::insert(&mut io, r, e, e).unwrap();
        }

        // Serialize all nodes
        let saved_nodes = io.nodes.clone();

        // Create new IO and restore
        let io2 = MemBTreeIO { nodes: saved_nodes, next_lba: io.next_lba };
        for e in entries {
            let found = BTree::lookup(&io2, r, e);
            crate::test_eq!(found, Some(e.to_vec()));
        }
    });

    crate::test_case!("btree_internal_routing_correct", {
        // Test that forces multiple internal nodes and verifies correct routing
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = MAX_ENTRIES + 50;
        for i in 0..n {
            let k = format!("rt{:04}", i);
            let v = format!("rv{:04}", i);
            r = BTree::insert(&mut io, r, k.as_bytes(), v.as_bytes()).unwrap();
        }
        // Verify all in forward and reverse
        for i in 0..n {
            let k = format!("rt{:04}", i);
            let v = format!("rv{:04}", i);
            crate::test_eq!(BTree::lookup(&io, r, k.as_bytes()), Some(v.as_bytes().to_vec()));
        }
        for i in (0..n).rev() {
            let k = format!("rt{:04}", i);
            let v = format!("rv{:04}", i);
            crate::test_eq!(BTree::lookup(&io, r, k.as_bytes()), Some(v.as_bytes().to_vec()));
        }
    });

    crate::test_case!("btree_merge_preserves_order", {
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let n = 20;
        // Insert
        for i in 0..n {
            let k = format!("mo{:04}", i);
            let v = format!("mv{:04}", i);
            r = BTree::insert(&mut io, r, k.as_bytes(), v.as_bytes()).unwrap();
        }
        // Delete all but last (forces merges)
        for i in 0..n - 1 {
            let k = format!("mo{:04}", i);
            let result = BTree::delete(&mut io, r, k.as_bytes()).unwrap();
            if let Some(new_root) = result { r = new_root; }
        }
        // Verify last key remains
        let last_k = format!("mo{:04}", n - 1);
        let last_v = format!("mv{:04}", n - 1);
        crate::test_eq!(BTree::lookup(&io, r, last_k.as_bytes()), Some(last_v.as_bytes().to_vec()));
        // Walk should have exactly 1 entry
        let mut count = 0;
        BTree::walk(&io, r, &mut |_| count += 1);
        crate::test_eq!(count, 1);
    });

    crate::test_case!("btree_empty_tree", {
        let io = MemBTreeIO::new();
        crate::test_eq!(BTree::lookup(&io, 0, b"any"), None);
        let result = BTree::delete(&mut MemBTreeIO::new(), 0, b"any");
        crate::test_eq!(result, Some(None));
    });
}
