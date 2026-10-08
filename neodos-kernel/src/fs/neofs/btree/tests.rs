// ── Tests ──────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use super::*;

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

    crate::test_case!("btree_wide_values_multileaf", {
        // Valores de 128 bytes (como un DirEntry): ~28 entradas por hoja, así
        // que 60 fuerzan árbol multi-hoja. Antes del split por bytes, el
        // serializador truncaba la hoja (count > entradas escritas).
        let mut io = MemBTreeIO::new();
        let mut r = 0;
        let value = alloc::vec![0xABu8; 128];
        for i in 0..60u32 {
            let k = format!("f{:04}", i);
            r = BTree::insert(&mut io, r, k.as_bytes(), &value).unwrap();
        }
        for i in 0..60u32 {
            let k = format!("f{:04}", i);
            crate::test_eq!(BTree::lookup(&io, r, k.as_bytes()), Some(value.clone()));
        }
        let mut count = 0;
        BTree::walk(&io, r, &mut |_| count += 1);
        crate::test_eq!(count, 60);

        // Borrar 40 (fuerza merges/borrows) y comprobar las 20 restantes.
        for i in 0..40u32 {
            let k = format!("f{:04}", i);
            let nr = BTree::delete(&mut io, r, k.as_bytes()).unwrap();
            r = nr.unwrap_or(0);
        }
        let mut count = 0;
        BTree::walk(&io, r, &mut |_| count += 1);
        crate::test_eq!(count, 20);
        for i in 40..60u32 {
            let k = format!("f{:04}", i);
            crate::test_true!(BTree::lookup(&io, r, k.as_bytes()).is_some());
        }
    });
}
