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

pub fn dump_counters() {
    let rx_pkts = COUNTERS.rx_packets.load(Ordering::Relaxed);
    let tx_pkts = COUNTERS.tx_packets.load(Ordering::Relaxed);
    let rx_bytes = COUNTERS.rx_bytes.load(Ordering::Relaxed);
    let tx_bytes = COUNTERS.tx_bytes.load(Ordering::Relaxed);
    let arp_rx = COUNTERS.arp_requests_rx.load(Ordering::Relaxed);
    let arp_tx = COUNTERS.arp_replies_tx.load(Ordering::Relaxed);
    let icmp_rx = COUNTERS.icmp_requests_rx.load(Ordering::Relaxed);
    let icmp_tx = COUNTERS.icmp_replies_tx.load(Ordering::Relaxed);

    #[cfg(debug_assertions)]
    {
        crate::serial_println!("╔══════════════════ NET COUNTERS ══════════════════╗");
        crate::serial_println!("║ RX packets:  {:>8}   RX bytes: {:>10}   ║", rx_pkts, rx_bytes);
        crate::serial_println!("║ TX packets:  {:>8}   TX bytes: {:>10}   ║", tx_pkts, tx_bytes);
        crate::serial_println!("║ ARP Req RX:  {:>8}   ARP Rep TX: {:>8}    ║", arp_rx, arp_tx);
        crate::serial_println!("║ ICMP Req RX: {:>8}   ICMP Rep TX: {:>8}    ║", icmp_rx, icmp_tx);
        crate::serial_println!("╚══════════════════════════════════════════════════╝");
    }
}
