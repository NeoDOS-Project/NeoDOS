// src/drivers/virtio/mod.rs — VirtIO bus/transport layer.
// Reusable base for all VirtIO drivers (blk today; net/snd planned).
// Device-specific ABI (e.g. virtio-blk feature bits and request structs)
// lives with the concrete driver in `drivers/hw/virtio_blk.rs`.

pub mod transport;
pub mod vring;
