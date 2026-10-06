//! In-memory BTreeIO implementation (tests/diagnostics).

use alloc::vec::Vec;
use super::*;

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
