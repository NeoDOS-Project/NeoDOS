//! B-tree persistente genérico con COW.
//! Nodos de 4KB. Claves y valores de longitud variable.
//! Las operaciones de E/S se delegan al trait `BTreeIO`.

#![allow(dead_code)]

use alloc::vec::Vec;
use super::crc32::crc32;

pub const NODE_SIZE: usize = 4096;
const HEADER_SIZE: usize = 8;
const MAX_ENTRIES: usize = 200;

/// Trait para E/S de nodos del B-tree. 
/// El implementor decide dónde/cómo se almacenan los nodos.
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
            let pos = node.find_pos(key).unwrap_or_else(|p| p);
            let child_lba = if pos < node.entries.len() {
                u64_from_value(&node.entries[pos].value)?
            } else {
                u64_from_value(node.entries.last()?.value.as_slice())?
            };
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
            let pos = node.find_pos(key).unwrap_or_else(|p| p);
            let child_lba = if pos < node.entries.len() {
                u64_from_value(&node.entries[pos].value)?
            } else {
                u64_from_value(node.entries.last()?.value.as_slice())?
            };
            match Self::ins(io, child_lba, key, value)? {
                InsertResult::Done(new_child_lba) => {
                    let mut new_node = node.clone();
                    let rp = if pos < node.entries.len() { pos } else { new_node.entries.len() - 1 };
                    new_node.entries[rp].value = new_child_lba.to_le_bytes().to_vec();
                    Some(InsertResult::Done(io.write_node(&new_node)))
                }
                InsertResult::Split(median_key, left_lba, right_lba) => {
                    let mut new_node = node.clone();
                    let ip = pos;
                    if ip < new_node.entries.len() {
                        new_node.entries[ip].value = left_lba.to_le_bytes().to_vec();
                        new_node.entries.insert(ip + 1, BTreeEntry {
                            key: median_key, value: right_lba.to_le_bytes().to_vec(),
                        });
                    } else {
                        let lp = new_node.entries.len() - 1;
                        new_node.entries[lp].value = left_lba.to_le_bytes().to_vec();
                        new_node.entries.push(BTreeEntry {
                            key: median_key, value: right_lba.to_le_bytes().to_vec(),
                        });
                    }
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
        Self::del(io, root_lba, key)
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
            let pos = node.find_pos(key).unwrap_or_else(|p| p);
            let child_lba = if pos < node.entries.len() {
                u64_from_value(&node.entries[pos].value)?
            } else {
                u64_from_value(node.entries.last()?.value.as_slice())?
            };
            let new_child = Self::del(io, child_lba, key)?;
            let mut new_node = node.clone();
            let rp = if pos < node.entries.len() { pos } else { new_node.entries.len() - 1 };
            match new_child {
                None => {
                    new_node.entries.remove(rp);
                    if new_node.entries.is_empty() { return Some(None); }
                }
                Some(lba) => { new_node.entries[rp].value = lba.to_le_bytes().to_vec(); }
            }
            Some(Some(io.write_node(&new_node)))
        }
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

enum InsertResult {
    Done(u64),
    Split(Vec<u8>, u64, u64),
}

fn u64_from_value(v: &[u8]) -> Option<u64> {
    if v.len() < 8 { return None; }
    Some(u64::from_le_bytes(v[..8].try_into().ok()?))
}

fn split_node(node: &BTreeNode) -> (Vec<u8>, BTreeNode, BTreeNode) {
    let mid = node.entries.len() / 2;
    let mut left = BTreeNode::new(node.node_type);
    let mut right = BTreeNode::new(node.node_type);
    left.entries = node.entries[..mid].to_vec();
    right.entries = node.entries[mid + 1..].to_vec();
    (node.entries[mid].key.clone(), left, right)
}

fn split_internal(node: &BTreeNode) -> (Vec<u8>, BTreeNode, BTreeNode) {
    split_node(node)
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
}
