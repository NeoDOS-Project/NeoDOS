// src/virtio/mod.rs — VirtIO subsystem (VIO-ARCH)
// Reusable base for all VirtIO drivers.

pub mod vring;
pub mod transport;



// ── Block device feature bits ────────────────────────────────────────
pub const BLK_F_SIZE_MAX: u32 = 1 << 1;
pub const BLK_F_SEG_MAX: u32 = 1 << 2;
pub const BLK_F_BLK_SIZE: u32 = 1 << 6;
pub const BLK_F_FLUSH: u32 = 1 << 14;
pub const BLK_F_DISCARD: u32 = 1 << 13;

pub const BLK_ACCEPTED_FEATURES: u32 = BLK_F_SIZE_MAX | BLK_F_SEG_MAX
    | BLK_F_BLK_SIZE | BLK_F_FLUSH | 0; // VIRTIO_F_VERSION_1 handled by transport

// ── Block request types ──────────────────────────────────────────────
pub const BLK_T_IN: u32 = 0;
pub const BLK_T_OUT: u32 = 1;
pub const BLK_T_FLUSH: u32 = 4;

// ── Block request/response struct (ABI stable) ───────────────────────
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct VirtioBlkReq {
    pub type_: u32,
    pub reserved: u32,
    pub sector: u64,
}

// ── Tests ────────────────────────────────────────────────────────────

pub fn register_tests() {
    use crate::test_case;
    use crate::test_eq;

    test_case!("vio_virtqueue_layout", {
        test_eq!(core::mem::size_of::<vring::VringDesc>(), 16);
        test_eq!(core::mem::size_of::<VirtioBlkReq>(), 16);
    });

    test_case!("vio_feature_constants", {
        // Verify feature bits match common constants
        test_eq!(BLK_F_SIZE_MAX, 1 << 1);
        test_eq!(BLK_F_SEG_MAX, 1 << 2);
        test_eq!(BLK_F_BLK_SIZE, 1 << 6);
        test_eq!(BLK_F_FLUSH, 1 << 14);
        test_eq!(BLK_T_IN, 0);
        test_eq!(BLK_T_OUT, 1);
        test_eq!(BLK_T_FLUSH, 4);
    });
}
