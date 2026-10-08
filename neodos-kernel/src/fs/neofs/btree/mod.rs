//! B-tree persistente genérico con COW.
//! Nodos de 4KB. Claves y valores de longitud variable.
//! Las operaciones de E/S se delegan al trait `BTreeIO`.
//! Fusiona nodos tras eliminación para mantener el factor de llenado mínimo.

mod node;
mod tree;
mod mem;
mod tests;

pub use node::*;
pub use tree::*;
pub use mem::*;
pub use tests::register_btree_tests;

pub const NODE_SIZE: usize = 4096;
const HEADER_SIZE: usize = 8;
pub const MAX_ENTRIES: usize = 200;

pub trait BTreeIO {
    fn read_node(&self, lba: u64) -> Option<BTreeNode>;
    fn write_node(&mut self, node: &BTreeNode) -> u64;
}
