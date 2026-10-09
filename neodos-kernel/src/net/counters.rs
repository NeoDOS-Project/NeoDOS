use core::sync::atomic::{AtomicU64, Ordering};

pub struct NetCounters {
    pub rx_packets: AtomicU64,
    pub tx_packets: AtomicU64,
    pub arp_requests_rx: AtomicU64,
    pub arp_replies_tx: AtomicU64,
    pub icmp_requests_rx: AtomicU64,
    pub icmp_replies_tx: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
}

impl NetCounters {
    pub const fn new() -> Self {
        NetCounters {
            rx_packets: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            arp_requests_rx: AtomicU64::new(0),
            arp_replies_tx: AtomicU64::new(0),
            icmp_requests_rx: AtomicU64::new(0),
            icmp_replies_tx: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
        }
    }
}

pub static COUNTERS: NetCounters = NetCounters::new();

/// Per-interface counters (#373). Slots `0..MAX_NICS` are physical NICs by
/// registry id; slot `MAX_NICS` is loopback. Fixed-size so userland can index
/// it the same way as `NicInfo` enumeration (physical ids in order, loopback
/// last); see the `NetStats` query handler.
pub struct IfaceStats {
    pub rx_packets: AtomicU64,
    pub tx_packets: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub tx_bytes: AtomicU64,
    pub rx_errors: AtomicU64,
    pub tx_errors: AtomicU64,
}

impl IfaceStats {
    pub const fn new() -> Self {
        IfaceStats {
            rx_packets: AtomicU64::new(0),
            tx_packets: AtomicU64::new(0),
            rx_bytes: AtomicU64::new(0),
            tx_bytes: AtomicU64::new(0),
            rx_errors: AtomicU64::new(0),
            tx_errors: AtomicU64::new(0),
        }
    }

    fn note_rx(&self, len: usize) {
        self.rx_packets.fetch_add(1, Ordering::Relaxed);
        self.rx_bytes.fetch_add(len as u64, Ordering::Relaxed);
    }

    fn note_tx(&self, len: usize) {
        self.tx_packets.fetch_add(1, Ordering::Relaxed);
        self.tx_bytes.fetch_add(len as u64, Ordering::Relaxed);
    }
}

/// Slot holding loopback counters in [`NIC_STATS`].
pub const LOOPBACK_SLOT: usize = super::types::MAX_NICS;
/// Physical NIC slots plus loopback.
pub const STAT_SLOTS: usize = super::types::MAX_NICS + 1;

static NIC_STATS: [IfaceStats; STAT_SLOTS] = [
    IfaceStats::new(),
    IfaceStats::new(),
    IfaceStats::new(),
    IfaceStats::new(),
    IfaceStats::new(),
];

/// Map a NIC id (or the loopback sentinel) to a stats slot.
/// Unknown ids return `None` and are never counted.
pub fn stats_slot_for_nic(nic_id: u32) -> Option<usize> {
    if nic_id == super::loopback::LOOPBACK_NIC_ID {
        return Some(LOOPBACK_SLOT);
    }
    let slot = nic_id as usize;
    if slot < super::types::MAX_NICS {
        Some(slot)
    } else {
        None
    }
}

/// Record a received frame (`len` bytes) on `slot`.
pub fn note_rx(slot: usize, len: usize) {
    if slot < STAT_SLOTS {
        NIC_STATS[slot].note_rx(len);
    }
}

/// Record a transmitted frame (`len` bytes) on `slot`.
pub fn note_tx(slot: usize, len: usize) {
    if slot < STAT_SLOTS {
        NIC_STATS[slot].note_tx(len);
    }
}

/// Record a receive error on `slot` (runt frame, etc.).
pub fn note_rx_err(slot: usize) {
    if slot < STAT_SLOTS {
        NIC_STATS[slot].rx_errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// Record a transmit error on `slot` (queue full, driver reject).
pub fn note_tx_err(slot: usize) {
    if slot < STAT_SLOTS {
        NIC_STATS[slot].tx_errors.fetch_add(1, Ordering::Relaxed);
    }
}

/// Snapshot `(rx_packets, tx_packets, rx_bytes, tx_bytes, rx_errors, tx_errors)`.
pub fn snapshot(slot: usize) -> (u64, u64, u64, u64, u64, u64) {
    if slot < STAT_SLOTS {
        let s = &NIC_STATS[slot];
        (
            s.rx_packets.load(Ordering::Relaxed),
            s.tx_packets.load(Ordering::Relaxed),
            s.rx_bytes.load(Ordering::Relaxed),
            s.tx_bytes.load(Ordering::Relaxed),
            s.rx_errors.load(Ordering::Relaxed),
            s.tx_errors.load(Ordering::Relaxed),
        )
    } else {
        (0, 0, 0, 0, 0, 0)
    }
}

pub fn dump_counters() {
    let rx_pkts = COUNTERS.rx_packets.load(Ordering::Relaxed);
    let tx_pkts = COUNTERS.tx_packets.load(Ordering::Relaxed);
    let rx_bytes = COUNTERS.rx_bytes.load(Ordering::Relaxed);
    let tx_bytes = COUNTERS.tx_bytes.load(Ordering::Relaxed);
    let arp_rx = COUNTERS.arp_requests_rx.load(Ordering::Relaxed);
    let arp_tx = COUNTERS.arp_replies_tx.load(Ordering::Relaxed);
    let icmp_rx = COUNTERS.icmp_requests_rx.load(Ordering::Relaxed);
    let icmp_tx = COUNTERS.icmp_replies_tx.load(Ordering::Relaxed);

    ktrace!(crate::log::LogSubsys::Net, "╔══════════════════ NET COUNTERS ══════════════════╗");
    ktrace!(crate::log::LogSubsys::Net, "║ RX packets:  {:>8}   RX bytes: {:>10}   ║", rx_pkts, rx_bytes);
    ktrace!(crate::log::LogSubsys::Net, "║ TX packets:  {:>8}   TX bytes: {:>10}   ║", tx_pkts, tx_bytes);
    ktrace!(crate::log::LogSubsys::Net, "║ ARP Req RX:  {:>8}   ARP Rep TX: {:>8}    ║", arp_rx, arp_tx);
    ktrace!(crate::log::LogSubsys::Net, "║ ICMP Req RX: {:>8}   ICMP Rep TX: {:>8}    ║", icmp_rx, icmp_tx);
    ktrace!(crate::log::LogSubsys::Net, "╚══════════════════════════════════════════════════╝");
}
