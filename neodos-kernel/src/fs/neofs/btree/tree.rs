//! B-tree operations (insert, lookup, delete, walk, split/merge).

use alloc::vec::Vec;
use super::*;

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
        let mut garbage = Vec::new();
        Self::insert_tracked(io, root_lba, key, value, &mut garbage)
    }

    /// Como [`insert`], pero registra en `garbage` los LBAs de los nodos que
    /// el COW reemplaza (para reclamarlos tras confirmar la nueva raíz).
    pub fn insert_tracked(
        io: &mut impl BTreeIO,
        root_lba: u64,
        key: &[u8],
        value: &[u8],
        garbage: &mut Vec<u64>,
    ) -> Option<u64> {
        if root_lba == 0 {
            let mut root = BTreeNode::new(NodeType::Leaf);
            root.entries.push(BTreeEntry { key: key.to_vec(), value: value.to_vec() });
            return Some(io.write_node(&root));
        }
        let result = Self::ins(io, root_lba, key, value, garbage)?;
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

    fn ins(io: &mut impl BTreeIO, node_lba: u64, key: &[u8], value: &[u8], garbage: &mut Vec<u64>) -> Option<InsertResult> {
        let node = io.read_node(node_lba)?;
        // El nodo original queda reemplazado por su copia COW.
        garbage.push(node_lba);
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
            match Self::ins(io, child_lba, key, value, garbage)? {
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
        let mut garbage = Vec::new();
        Self::delete_tracked(io, root_lba, key, &mut garbage)
    }

    /// Como [`delete`], pero registra en `garbage` los LBAs de los nodos
    /// reemplazados (incluido un nodo raíz colapsado).
    pub fn delete_tracked(
        io: &mut impl BTreeIO,
        root_lba: u64,
        key: &[u8],
        garbage: &mut Vec<u64>,
    ) -> Option<Option<u64>> {
        if root_lba == 0 { return Some(None); }
        let result = Self::del(io, root_lba, key, garbage);
        match result? {
            None => Some(None),
            Some(lba) => {
                if let Some(node) = io.read_node(lba) {
                    if node.node_type == NodeType::Internal && node.entries.len() == 1 {
                        if let Some(child_lba) = u64_from_value(&node.entries[0].value) {
                            // El nodo raíz interno colapsado se acaba de escribir
                            // y ya no se referencia: es basura.
                            garbage.push(lba);
                            return Some(Some(child_lba));
                        }
                    }
                }
                Some(Some(lba))
            }
        }
    }

    fn del(io: &mut impl BTreeIO, node_lba: u64, key: &[u8], garbage: &mut Vec<u64>) -> Option<Option<u64>> {
        let node = io.read_node(node_lba)?;
        // El nodo original queda reemplazado (o eliminado) por su copia COW.
        garbage.push(node_lba);
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
            let new_child = Self::del(io, child_lba, key, garbage)?;
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
                            Self::try_borrow_or_merge(io, &mut new_node, child_idx, garbage);
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
        garbage: &mut Vec<u64>,
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
                return Self::borrow_left(io, parent, child_idx, left, child, garbage);
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
                return Self::borrow_right(io, parent, child_idx, child, right, garbage);
            }
        }

        // Merge with left sibling (preferred)
        if child_idx > 0 {
            let left_lba = match u64_from_value(&parent.entries[child_idx - 1].value) {
                Some(l) => l, None => return false,
            };
            let left = match io.read_node(left_lba) { Some(l) => l, None => return false };
            Self::merge_into_left(io, parent, child_idx, left, child, garbage)
        } else if child_idx + 1 < n {
            let right_idx = child_idx + 1;
            let right_lba = match u64_from_value(&parent.entries[right_idx].value) {
                Some(l) => l, None => return false,
            };
            let right = match io.read_node(right_lba) { Some(r) => r, None => return false };
            Self::merge_into_right(io, parent, child_idx, child, right, garbage)
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
        garbage: &mut Vec<u64>,
    ) -> bool {
        let sep = parent.entries[child_idx].key.clone();
        if let Some(l) = u64_from_value(&parent.entries[child_idx - 1].value) { garbage.push(l); }
        if let Some(l) = u64_from_value(&parent.entries[child_idx].value) { garbage.push(l); }

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
        garbage: &mut Vec<u64>,
    ) -> bool {
        let right_idx = child_idx + 1;
        let sep = parent.entries[right_idx].key.clone();
        if let Some(l) = u64_from_value(&parent.entries[child_idx].value) { garbage.push(l); }
        if let Some(l) = u64_from_value(&parent.entries[right_idx].value) { garbage.push(l); }

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
        garbage: &mut Vec<u64>,
    ) -> bool {
        let sep = parent.entries[child_idx].key.clone();
        if let Some(l) = u64_from_value(&parent.entries[child_idx - 1].value) { garbage.push(l); }
        if let Some(l) = u64_from_value(&parent.entries[child_idx].value) { garbage.push(l); }
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
        garbage: &mut Vec<u64>,
    ) -> bool {
        let sep = parent.entries[child_idx + 1].key.clone();
        if let Some(l) = u64_from_value(&parent.entries[child_idx].value) { garbage.push(l); }
        if let Some(l) = u64_from_value(&parent.entries[child_idx + 1].value) { garbage.push(l); }
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

    /// Visitar el LBA de cada nodo del árbol (raíz incluida, nodos internos
    /// y hojas). Se usa para reconstruir la free list recorriendo qué bloques
    /// de metadatos están realmente en uso.
    pub fn walk_lbas(io: &impl BTreeIO, root_lba: u64, f: &mut impl FnMut(u64)) {
        walk_lbas_recursive(io, root_lba, f);
    }
}

fn walk_lbas_recursive(io: &impl BTreeIO, lba: u64, f: &mut impl FnMut(u64)) {
    if lba == 0 { return; }
    f(lba);
    let node = match io.read_node(lba) { Some(n) => n, None => return };
    if !node.is_leaf() {
        for entry in &node.entries {
            if let Some(child) = u64_from_value(&entry.value) {
                walk_lbas_recursive(io, child, f);
            }
        }
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
