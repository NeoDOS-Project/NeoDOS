//! B-tree node types and (de)serialization.

use alloc::vec::Vec;
use crate::fs::crc32::crc32;
use super::{NODE_SIZE, HEADER_SIZE, MAX_ENTRIES};

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

    pub(super) fn find_pos(&self, key: &[u8]) -> Result<usize, usize> {
        self.entries.binary_search_by(|e| e.key.as_slice().cmp(key))
    }
}
