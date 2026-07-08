//! Snapshot table — anillo circular de 64 entradas.
//! Almacenada en nodo de 4KB (tipo 4). Cada snapshot guarda root_btree_lba + timestamp.

#![allow(dead_code)]

use alloc::vec::Vec;

pub const MAX_SNAPSHOTS: usize = 64;
pub const NODE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub root_lba: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone)]
pub struct SnapshotTable {
    pub snapshots: Vec<Snapshot>,
    pub count: usize, // <= MAX_SNAPSHOTS
}

impl SnapshotTable {
    pub fn new() -> Self {
        SnapshotTable {
            snapshots: Vec::with_capacity(MAX_SNAPSHOTS),
            count: 0,
        }
    }

    /// Crear un snapshot con la raíz actual.
    /// Si ya hay 64, el más viejo se descarta (circular).
    pub fn create(&mut self, root_lba: u64, timestamp: u64) -> u64 {
        let id = self.count as u64;
        if self.snapshots.len() < MAX_SNAPSHOTS {
            self.snapshots.push(Snapshot { root_lba, timestamp });
            self.count = self.snapshots.len();
        } else {
            // Desplazar: descartar el más viejo, añadir al final
            self.snapshots.remove(0);
            self.snapshots.push(Snapshot { root_lba, timestamp });
        }
        id
    }

    /// Restaurar un snapshot por ID. Devuelve root_lba.
    pub fn restore(&self, id: u64) -> Option<u64> {
        let idx = self.resolve_id(id)?;
        Some(self.snapshots[idx].root_lba)
    }

    /// Obtener la lista de snapshots (id, root_lba, timestamp).
    pub fn list(&self) -> Vec<(u64, Snapshot)> {
        self.snapshots.iter().enumerate().map(|(i, s)| {
            (i as u64, *s)
        }).collect()
    }

    /// Vaciar la tabla.
    pub fn purge(&mut self) {
        self.snapshots.clear();
        self.count = 0;
    }

    /// Número de snapshots actuales.
    pub fn snapshot_count(&self) -> usize {
        self.count.min(self.snapshots.len())
    }

    fn resolve_id(&self, id: u64) -> Option<usize> {
        if (id as usize) < self.snapshots.len() {
            Some(id as usize)
        } else {
            None
        }
    }

    /// Serializar a nodo type 4.
    pub fn serialize(&self, buf: &mut [u8; NODE_SIZE]) {
        buf.fill(0);
        buf[0..2].copy_from_slice(&(4u16).to_le_bytes()); // node_type=4
        buf[2..4].copy_from_slice(&(self.snapshot_count() as u16).to_le_bytes());
        let mut offset = 8;
        for snapshot in self.snapshots.iter().take(MAX_SNAPSHOTS) {
            if offset + 16 > NODE_SIZE {
                break;
            }
            buf[offset..offset + 8].copy_from_slice(&snapshot.root_lba.to_le_bytes());
            buf[offset + 8..offset + 16].copy_from_slice(&snapshot.timestamp.to_le_bytes());
            offset += 16;
        }
        let cksum = crc32(&buf[8..]);
        buf[4..8].copy_from_slice(&cksum.to_le_bytes());
    }

    /// Deserializar desde nodo type 4.
    pub fn deserialize(buf: &[u8; NODE_SIZE]) -> Option<Self> {
        let cksum = crc32(&buf[8..]);
        let stored = u32::from_le_bytes(buf[4..8].try_into().ok()?);
        if stored != 0 && stored != cksum {
            return None;
        }
        let count = u16::from_le_bytes(buf[2..4].try_into().ok()?) as usize;
        let mut snapshots = Vec::with_capacity(count.min(MAX_SNAPSHOTS));
        let mut offset = 8;
        for _ in 0..count.min(MAX_SNAPSHOTS) {
            if offset + 16 > NODE_SIZE {
                break;
            }
            let root_lba = u64::from_le_bytes(buf[offset..offset + 8].try_into().ok()?);
            let timestamp = u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().ok()?);
            snapshots.push(Snapshot { root_lba, timestamp });
            offset += 16;
        }
        Some(SnapshotTable {
            count: snapshots.len(),
            snapshots,
        })
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_snapshot_tests() {
    crate::test_case!("snapshot_create_list_empty", {
        let st = SnapshotTable::new();
        crate::test_eq!(st.snapshot_count(), 0);
        let list = st.list();
        crate::test_eq!(list.len(), 0);
    });

    crate::test_case!("snapshot_create_one", {
        let mut st = SnapshotTable::new();
        let id = st.create(42, 1000);
        crate::test_eq!(id, 0);
        crate::test_eq!(st.snapshot_count(), 1);
        let root = st.restore(0).unwrap();
        crate::test_eq!(root, 42);
    });

    crate::test_case!("snapshot_create_multiple", {
        let mut st = SnapshotTable::new();
        for i in 0..10u64 {
            st.create(i * 100, i * 1000);
        }
        crate::test_eq!(st.snapshot_count(), 10);
        let root = st.restore(5).unwrap();
        crate::test_eq!(root, 500);
    });

    crate::test_case!("snapshot_circular_overflow", {
        let mut st = SnapshotTable::new();
        for i in 0..70u64 {
            st.create(i, i);
        }
        // Solo deben quedar 64
        crate::test_eq!(st.snapshot_count(), 64);
        // El snapshot 0 debe haber sido descartado
        let root6 = st.restore(6).unwrap();
        crate::test_eq!(root6, 70 - 64 + 6);
        // restore de id >= 64 falla
        crate::test_true!(st.restore(64).is_none());
    });

    crate::test_case!("snapshot_purge", {
        let mut st = SnapshotTable::new();
        st.create(100, 1);
        st.create(200, 2);
        st.purge();
        crate::test_eq!(st.snapshot_count(), 0);
    });

    crate::test_case!("snapshot_serialize_roundtrip", {
        let mut st = SnapshotTable::new();
        st.create(111, 100);
        st.create(222, 200);
        st.create(333, 300);
        let mut buf = [0u8; NODE_SIZE];
        st.serialize(&mut buf);
        let loaded = SnapshotTable::deserialize(&buf).unwrap();
        crate::test_eq!(loaded.snapshot_count(), 3);
        let r0 = loaded.restore(0).unwrap();
        crate::test_eq!(r0, 111);
        let r2 = loaded.restore(2).unwrap();
        crate::test_eq!(r2, 333);
    });
}
