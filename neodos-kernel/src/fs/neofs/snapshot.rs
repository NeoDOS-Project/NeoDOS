//! Snapshot table — anillo circular de 64 entradas.
//! Almacenada en nodo de 4KB (tipo 4). Cada snapshot guarda root_btree_lba + timestamp.

/// ABI-stable entry for snapshot list operations.
#[repr(C)]
pub struct SnapshotEntryRaw {
    pub id: u64,
    pub root_lba: u64,
    pub timestamp: u64,
}

#[allow(dead_code)]
use alloc::vec::Vec;
use crate::fs::crc32::crc32;

pub const MAX_SNAPSHOTS: usize = 64;
pub const NODE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct Snapshot {
    pub root_lba: u64,
    pub timestamp: u64,
    /// Número de generación monótono; actúa como id estable del snapshot.
    pub generation: u64,
}

#[derive(Debug, Clone)]
pub struct SnapshotTable {
    pub snapshots: Vec<Snapshot>,
    /// Siguiente generación a asignar. No se guarda explícitamente: se
    /// recalcula como `max(generation)+1` al deserializar.
    pub next_generation: u64,
}

impl SnapshotTable {
    pub fn new() -> Self {
        SnapshotTable {
            snapshots: Vec::with_capacity(MAX_SNAPSHOTS),
            next_generation: 0,
        }
    }

    /// Crear un snapshot con la raíz actual. Devuelve su número de generación.
    /// Si ya hay 64, el más viejo se descarta (anillo circular).
    pub fn create(&mut self, root_lba: u64, timestamp: u64) -> u64 {
        let generation = self.next_generation;
        self.next_generation = self.next_generation.wrapping_add(1);
        if self.snapshots.len() >= MAX_SNAPSHOTS {
            self.snapshots.remove(0);
        }
        self.snapshots.push(Snapshot { root_lba, timestamp, generation });
        generation
    }

    /// Restaurar un snapshot por su generación. Devuelve root_lba.
    pub fn restore(&self, id: u64) -> Option<u64> {
        self.snapshots.iter().find(|s| s.generation == id).map(|s| s.root_lba)
    }

    /// Borrar un snapshot por su generación. Devuelve `true` si existía.
    pub fn delete(&mut self, id: u64) -> bool {
        match self.snapshots.iter().position(|s| s.generation == id) {
            Some(pos) => { self.snapshots.remove(pos); true }
            None => false,
        }
    }

    /// Lista de snapshots (generación, snapshot).
    pub fn list(&self) -> Vec<(u64, Snapshot)> {
        self.snapshots.iter().map(|s| (s.generation, *s)).collect()
    }

    /// Vaciar la tabla.
    pub fn purge(&mut self) {
        self.snapshots.clear();
    }

    /// Número de snapshots actuales.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Serializar a nodo type 4.
    pub fn serialize(&self, buf: &mut [u8; NODE_SIZE]) {
        buf.fill(0);
        buf[0..2].copy_from_slice(&(4u16).to_le_bytes()); // node_type=4
        buf[2..4].copy_from_slice(&(self.snapshot_count() as u16).to_le_bytes());
        let mut offset = 8;
        for snapshot in self.snapshots.iter().take(MAX_SNAPSHOTS) {
            if offset + 24 > NODE_SIZE {
                break;
            }
            buf[offset..offset + 8].copy_from_slice(&snapshot.root_lba.to_le_bytes());
            buf[offset + 8..offset + 16].copy_from_slice(&snapshot.timestamp.to_le_bytes());
            buf[offset + 16..offset + 24].copy_from_slice(&snapshot.generation.to_le_bytes());
            offset += 24;
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
            if offset + 24 > NODE_SIZE {
                break;
            }
            let root_lba = u64::from_le_bytes(buf[offset..offset + 8].try_into().ok()?);
            let timestamp = u64::from_le_bytes(buf[offset + 8..offset + 16].try_into().ok()?);
            let generation = u64::from_le_bytes(buf[offset + 16..offset + 24].try_into().ok()?);
            snapshots.push(Snapshot { root_lba, timestamp, generation });
            offset += 24;
        }
        let next_generation = snapshots.iter().map(|s| s.generation).max().map_or(0, |g| g + 1);
        Some(SnapshotTable { snapshots, next_generation })
    }
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
        // Solo deben quedar 64 (generaciones 6..69)
        crate::test_eq!(st.snapshot_count(), 64);
        // Las generaciones más viejas se descartaron.
        crate::test_true!(st.restore(0).is_none());
        crate::test_true!(st.restore(5).is_none());
        // La generación 6 sobrevive con su root original.
        crate::test_eq!(st.restore(6).unwrap(), 6);
        crate::test_eq!(st.restore(69).unwrap(), 69);
        crate::test_true!(st.restore(70).is_none());
    });

    crate::test_case!("snapshot_purge", {
        let mut st = SnapshotTable::new();
        st.create(100, 1);
        st.create(200, 2);
        st.purge();
        crate::test_eq!(st.snapshot_count(), 0);
    });

    crate::test_case!("snapshot_generation_monotonic_and_delete", {
        let mut st = SnapshotTable::new();
        crate::test_eq!(st.create(10, 1), 0);
        crate::test_eq!(st.create(20, 2), 1);
        crate::test_eq!(st.create(30, 3), 2);
        // Borrar la generación 1; las demás siguen accesibles.
        crate::test_true!(st.delete(1));
        crate::test_eq!(st.snapshot_count(), 2);
        crate::test_true!(st.restore(1).is_none());
        crate::test_eq!(st.restore(0).unwrap(), 10);
        crate::test_eq!(st.restore(2).unwrap(), 30);
        // Borrar de nuevo falla.
        crate::test_true!(!st.delete(1));
        // La siguiente generación NO reutiliza ids.
        crate::test_eq!(st.create(40, 4), 3);
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
        // La generación se conserva y la siguiente no colisiona.
        crate::test_eq!(loaded.list()[0].0, 0);
        crate::test_eq!(loaded.list()[2].0, 2);
        let mut loaded = loaded;
        crate::test_eq!(loaded.create(444, 400), 3);
    });
}
