//! Free list de regiones contiguas.
//! Almacenada en nodos de 4KB (tipo 3). Alloc best-fit. Free mergea adyacentes.
//! Las regiones se serializan encadenadas mediante `next_lba` y su cabeza se
//! guarda en `SuperblockNE2::freelist_lba` para recuperarlas al montar.

#![allow(dead_code)]

use alloc::vec::Vec;
use crate::fs::crc32::crc32;

pub const REGION_SIZE: usize = 12; // start_lba(8) + length(4)
pub const REGIONS_PER_NODE: usize = 340;
pub const NODE_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy)]
pub struct FreeRegion {
    pub start_lba: u64,
    pub length: u32,
}

#[derive(Debug, Clone)]
pub struct FreeList {
    pub regions: Vec<FreeRegion>,
    /// `true` si hubo alloc/free desde la última persistencia en disco.
    pub dirty: bool,
}

impl FreeList {
    pub fn new() -> Self {
        FreeList { regions: Vec::new(), dirty: false }
    }

    /// Crear freelist inicial con una sola región que cubre todo el espacio libre.
    pub fn with_range(start_lba: u64, total_blocks: u64) -> Self {
        let mut fl = FreeList::new();
        fl.regions.push(FreeRegion {
            start_lba,
            length: total_blocks as u32,
        });
        fl.dirty = true;
        fl
    }

    /// Alocar `count` bloques contiguos. Best-fit: elige la región libre más
    /// pequeña que quepa, para minimizar la fragmentación. Divide la región
    /// si es mayor que lo pedido. Devuelve (start_lba, length_real).
    pub fn alloc(&mut self, count: u32) -> Option<(u64, u32)> {
        if count == 0 {
            return None;
        }
        let mut best: Option<usize> = None;
        for (i, region) in self.regions.iter().enumerate() {
            if region.length >= count {
                match best {
                    Some(b) if self.regions[b].length <= region.length => {}
                    _ => best = Some(i),
                }
            }
        }
        let i = best?;
        self.dirty = true;
        let region = self.regions[i];
        if region.length == count {
            self.regions.remove(i);
        } else {
            self.regions[i] = FreeRegion {
                start_lba: region.start_lba + count as u64,
                length: region.length - count,
            };
        }
        Some((region.start_lba, count))
    }

    /// Alocar sin merge posterior (para COW donde se sabe que se usará).
    pub fn alloc_blocks(&mut self, count: u32) -> Option<u64> {
        self.alloc(count).map(|(lba, _)| lba)
    }

    /// Liberar un rango de bloques. Mergea con regiones adyacentes.
    pub fn free(&mut self, start_lba: u64, length: u32) {
        self.dirty = true;
        let new = FreeRegion { start_lba, length };

        // Buscar posición de inserción ordenada por start_lba
        let mut merged = new;
        let mut i = 0;
        while i < self.regions.len() {
            let r = self.regions[i];

            // ¿r está inmediatamente antes de merged?
            if r.start_lba + r.length as u64 == merged.start_lba {
                merged = FreeRegion {
                    start_lba: r.start_lba,
                    length: r.length + merged.length,
                };
                self.regions.remove(i);
                continue;
            }

            // ¿r está inmediatamente después de merged?
            if merged.start_lba + merged.length as u64 == r.start_lba {
                merged = FreeRegion {
                    start_lba: merged.start_lba,
                    length: merged.length + r.length,
                };
                self.regions.remove(i);
                continue;
            }

            // ¿merged está completamente antes de r?
            if merged.start_lba + merged.length as u64 <= r.start_lba {
                self.regions.insert(i, merged);
                return;
            }

            i += 1;
        }
        // Si llegamos aquí, merged va al final
        self.regions.push(merged);
    }

    /// Número total de bloques libres.
    pub fn total_free(&self) -> u64 {
        let mut total = 0u64;
        for r in &self.regions {
            total += r.length as u64;
        }
        total
    }

    /// Número de regiones libres.
    pub fn region_count(&self) -> usize {
        self.regions.len()
    }

    /// Validar la lista: regiones no vacías, ordenadas, sin solapes y dentro
    /// de los límites de la partición. Los bloques 0 (superblock) y 1 (raíz
    /// inicial) nunca pueden aparecer como libres.
    pub fn is_valid(&self, total_blocks: u64) -> bool {
        let mut prev_end = 2u64;
        for r in &self.regions {
            if r.length == 0 || r.start_lba < prev_end {
                return false;
            }
            match r.start_lba.checked_add(r.length as u64) {
                Some(end) if end <= total_blocks => prev_end = end,
                _ => return false,
            }
        }
        true
    }

    /// Construir una free list a partir del conjunto ordenado de bloques
    /// usados (complemento dentro de `[0, total_blocks)`). Usado por la
    /// recuperación al montar cuando no hay free list persistida o es inválida.
    pub fn from_used(used: &[u64], total_blocks: u64) -> Self {
        let mut regions = Vec::new();
        let mut block = 0u64;
        let mut idx = 0usize;
        while block < total_blocks {
            while idx < used.len() && used[idx] < block {
                idx += 1;
            }
            if idx < used.len() && used[idx] == block {
                block += 1;
                continue;
            }
            let start = block;
            while block < total_blocks {
                while idx < used.len() && used[idx] < block {
                    idx += 1;
                }
                if idx < used.len() && used[idx] == block {
                    break;
                }
                block += 1;
            }
            regions.push(FreeRegion {
                start_lba: start,
                length: (block - start) as u32,
            });
        }
        FreeList { regions, dirty: true }
    }

    /// Serializar freelist al formato de nodo type 3.
    /// Si no cabe en un nodo, establece next_lba para encadenar.
    pub fn serialize(&self, buf: &mut [u8; NODE_SIZE], next_lba: u64) {
        buf.fill(0);
        // Header
        buf[0..2].copy_from_slice(&(3u16).to_le_bytes()); // node_type=3
        let count = self.regions.len().min(REGIONS_PER_NODE);
        buf[2..4].copy_from_slice(&(count as u16).to_le_bytes());
        // Entradas
        let mut offset = 8;
        for i in 0..count {
            let r = &self.regions[i];
            buf[offset..offset + 8].copy_from_slice(&r.start_lba.to_le_bytes());
            buf[offset + 8..offset + 12].copy_from_slice(&r.length.to_le_bytes());
            offset += REGION_SIZE;
        }
        // next_lba al final del payload
        if offset + 8 <= NODE_SIZE {
            buf[offset..offset + 8].copy_from_slice(&next_lba.to_le_bytes());
        }
        // CRC32 del payload
        let cksum = crc32(&buf[8..]);
        buf[4..8].copy_from_slice(&cksum.to_le_bytes());
    }

    /// Deserializar freelist desde un nodo.
    pub fn deserialize(buf: &[u8; NODE_SIZE]) -> Option<(FreeList, u64)> {
        let cksum = crc32(&buf[8..]);
        let stored = u32::from_le_bytes(buf[4..8].try_into().ok()?);
        if stored != 0 && stored != cksum {
            return None;
        }
        let count = u16::from_le_bytes(buf[2..4].try_into().ok()?) as usize;
        let mut regions = Vec::with_capacity(count);
        let mut offset = 8;
        for _ in 0..count.min(REGIONS_PER_NODE) {
            if offset + 12 > NODE_SIZE {
                break;
            }
            let start_lba = u64::from_le_bytes(buf[offset..offset + 8].try_into().ok()?);
            let length = u32::from_le_bytes(buf[offset + 8..offset + 12].try_into().ok()?);
            regions.push(FreeRegion { start_lba, length });
            offset += REGION_SIZE;
        }
        let next_lba = if offset + 8 <= NODE_SIZE {
            u64::from_le_bytes(buf[offset..offset + 8].try_into().ok()?)
        } else {
            0
        };
        Some((FreeList { regions, dirty: false }, next_lba))
    }
}

// ── Tests ──────────────────────────────────────────────────────────

pub fn register_freelist_tests() {
    crate::test_case!("freelist_alloc_simple", {
        let mut fl = FreeList::with_range(100, 1000);
        crate::test_eq!(fl.total_free(), 1000);
        let (lba, len) = fl.alloc(10).unwrap();
        crate::test_eq!(lba, 100);
        crate::test_eq!(len, 10);
        crate::test_eq!(fl.total_free(), 990);
        crate::test_eq!(fl.region_count(), 1);
        let (lba2, _) = fl.alloc(20).unwrap();
        crate::test_eq!(lba2, 110);
    });

    crate::test_case!("freelist_alloc_exhaust", {
        let mut fl = FreeList::with_range(0, 50);
        let (_, len) = fl.alloc(50).unwrap();
        crate::test_eq!(len, 50);
        crate::test_eq!(fl.total_free(), 0);
        crate::test_true!(fl.alloc(1).is_none());
    });

    crate::test_case!("freelist_free_merge_right", {
        let mut fl = FreeList::with_range(100, 100);
        let (lba, _) = fl.alloc(40).unwrap();
        crate::test_eq!(lba, 100);
        // lba=100, len=40 allocated. Free region starts at 140, len=60
        crate::test_eq!(fl.region_count(), 1);
        crate::test_eq!(fl.regions[0].start_lba, 140);
        // Free the allocated block
        fl.free(100, 40);
        // Should merge back with the 140/60 region → 100/100
        crate::test_eq!(fl.region_count(), 1);
        crate::test_eq!(fl.regions[0].start_lba, 100);
        crate::test_eq!(fl.regions[0].length, 100);
    });

    crate::test_case!("freelist_free_merge_left", {
        let mut fl = FreeList::with_range(100, 100);
        let (lba, _) = fl.alloc(100).unwrap();
        crate::test_eq!(fl.total_free(), 0);
        crate::test_eq!(lba, 100);
        // Free at 200 (which was never allocated — simula escenario real)
        fl.free(200, 50);
        crate::test_eq!(fl.region_count(), 1);
        crate::test_eq!(fl.regions[0].start_lba, 200);
        // Free adjacent before it
        fl.free(150, 50);
        crate::test_eq!(fl.region_count(), 1);
        crate::test_eq!(fl.regions[0].start_lba, 150);
        crate::test_eq!(fl.regions[0].length, 100);
    });

    crate::test_case!("freelist_serialize_roundtrip", {
        let mut fl = FreeList::new();
        fl.free(1000, 500);
        fl.free(2000, 300);
        let mut buf = [0u8; NODE_SIZE];
        fl.serialize(&mut buf, 0);
        let (loaded, next) = FreeList::deserialize(&buf).unwrap();
        crate::test_eq!(next, 0);
        crate::test_eq!(loaded.region_count(), 2);
        crate::test_eq!(loaded.regions[0].start_lba, 1000);
        crate::test_eq!(loaded.regions[0].length, 500);
        crate::test_eq!(loaded.regions[1].start_lba, 2000);
        crate::test_eq!(loaded.regions[1].length, 300);
    });

    crate::test_case!("freelist_alloc_blocks", {
        let mut fl = FreeList::with_range(50, 200);
        let lba = fl.alloc_blocks(10).unwrap();
        crate::test_eq!(lba, 50);
        let lba2 = fl.alloc_blocks(5).unwrap();
        crate::test_eq!(lba2, 60);
        let lba3 = fl.alloc_blocks(185).unwrap();
        crate::test_eq!(lba3, 65);
        crate::test_eq!(fl.total_free(), 0);
    });

    crate::test_case!("freelist_alloc_best_fit", {
        // Best-fit debe escoger la región de 50 bloques (200) antes que la
        // de 100 bloques (0), aun estando esta última primero.
        let mut fl = FreeList::new();
        fl.free(0, 100);
        fl.free(200, 50);
        let (lba, len) = fl.alloc(40).unwrap();
        crate::test_eq!(lba, 200);
        crate::test_eq!(len, 40);
        // La región elegida sobrevive como 10 bloques en 240.
        crate::test_eq!(fl.region_count(), 2);
        crate::test_eq!(fl.regions[1].start_lba, 240);
        crate::test_eq!(fl.regions[1].length, 10);
    });

    crate::test_case!("freelist_alloc_rejects_zero", {
        let mut fl = FreeList::with_range(100, 100);
        crate::test_true!(fl.alloc(0).is_none());
        crate::test_eq!(fl.total_free(), 100);
    });

    crate::test_case!("freelist_is_valid", {
        let mut fl = FreeList::new();
        fl.free(2, 10);
        fl.free(20, 5);
        crate::test_true!(fl.is_valid(100));

        let overlap = FreeList { regions: alloc::vec![
            FreeRegion { start_lba: 2, length: 10 },
            FreeRegion { start_lba: 5, length: 10 },
        ], dirty: false };
        crate::test_false!(overlap.is_valid(100));

        let out_of_bounds = FreeList { regions: alloc::vec![
            FreeRegion { start_lba: 2, length: 200 },
        ], dirty: false };
        crate::test_false!(out_of_bounds.is_valid(100));

        let touches_superblock = FreeList { regions: alloc::vec![
            FreeRegion { start_lba: 0, length: 4 },
        ], dirty: false };
        crate::test_false!(touches_superblock.is_valid(100));
    });

    crate::test_case!("freelist_from_used_reconstruct", {
        let used = [0u64, 1, 5, 6, 7];
        let fl = FreeList::from_used(&used, 10);
        crate::test_eq!(fl.region_count(), 2);
        crate::test_eq!(fl.regions[0].start_lba, 2);
        crate::test_eq!(fl.regions[0].length, 3);
        crate::test_eq!(fl.regions[1].start_lba, 8);
        crate::test_eq!(fl.regions[1].length, 2);
        crate::test_true!(fl.is_valid(10));
    });
}
