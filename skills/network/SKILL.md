---
name: network
description: Modify TCP/IP stack, sockets, NIC drivers, ARP, DNS, DHCP user service
---

# Networking

## When to use

Modifying the TCP/IP stack, socket abstraction, NIC driver (e1000), ARP cache, DNS resolver, DHCP service, or network-related Ob types/syscalls.

## Goal

Correctly implement network changes with proper protocol handling, socket lifecycle, NIC management, and userspace DHCP integration.

## References

- `docs/networking/stack.md` — subsystem documentation
- `src/net/types.rs` — MacAddr, Ipv4Addr, SocketAddrV4, TcpState, SocketType
- `src/net/ethernet.rs` — Ethernet frame header, FCS, ETH_TYPE constants
- `src/net/arp.rs` — ARP cache (64 entries, 300s TTL), resolve/insert/request/reply
- `src/net/ipv4.rs` — IPv4 header, checksum, packet building
- `src/net/icmp.rs` — ICMP echo request/reply, port unreachable
- `src/net/udp.rs` — UDP header, pseudo-header checksum
- `src/net/tcp.rs` — TCP state machine (11 states), sliding window, segment building
- `src/net/socket.rs` — SocketManager, bind/connect/listen/send/recv/close, KWait wake
- `src/net/nic.rs` — NetworkInterface trait, NicRegistry (4 slots)
- `src/net/e1000.rs` — Intel e1000 NIC driver, ring buffers, MMIO
- `src/net/dns.rs` — DNS resolver, cache (64 entries), UDP transport
- `src/net/mod.rs` — init_networking(), net_tick(), packet dispatch
- `src/object/types.rs` — ObInfoClass (17-20, 23), ObSetInfoClass (18-22, 27), ObType::Socket (18)
- `userbin/dhcpd/src/main.rs` — Userspace DHCP client (DORA sequence)

## Architecture

### Module Map

```
src/net/
├── types.rs      — MacAddr, Ipv4Addr, SocketAddrV4, TcpState, SocketType, SocketDirection
├── ethernet.rs   — EthernetHeader, ETH_TYPE_ARP (0x0806), ETH_TYPE_IPV4 (0x0800)
├── arp.rs        — ArpPacket, cache (64 entries, 300s TTL), arp_resolve()
├── ipv4.rs       — Ipv4Header, checksum, build_ipv4_header()
├── icmp.rs       — IcmpHeader, echo request/reply, port unreachable
├── udp.rs        — UdpHeader, pseudo-header checksum, build_udp_datagram()
├── tcp.rs        — TcpHeader, 11-state machine, 16 KB sliding window, segment building
├── socket.rs     — SocketManager (64 sockets), dispatch, KWait integration
├── nic.rs        — NetworkInterface trait, NicRegistry (4 slots), TX/RX
├── e1000.rs      — Intel e1000: MMIO, RX/TX rings, descriptor management
├── dns.rs        — DnsCache (64 entries, 300s TTL), resolve(), UDP transport
├── mod.rs        — init_networking(), net_tick(), net_handle_incoming_packet()
└── tests.rs      — 17+ integration tests
```

### Packet Flow

```
e1000 poll_packet() → 2048 byte buffer
  → ethernet parse (dst MAC, src MAC, ethertype)
    → ARP (0x0806):     arp_resolve() → cache lookup / reply
    → IPv4 (0x0800):
      → ICMP (proto 1):  echo request → reply; port unreachable
      → UDP (proto 17):  udp_dispatch() → socket by dst port
      → TCP (proto 6):   tcp_dispatch() → socket by (src IP, src port, dst IP, dst port)
```

### ObType Integration

**ObType::Socket** = 18. Sockets are created via `ob_create` with `attrs` encoding: bits 0-7 = socket type (1=TCP, 2=UDP, 3=Raw), bits 8-23 = port.

| ObInfoClass | ID | Description |
| ------------ | -- | ----------- |
| SocketInfo | 17 | State, type, local/remote addresses |
| SocketAddr | 18 | Bound address and port |
| TcpStatus | 19 | TCP connection state |
| NicInfo | 20 | NIC config (IP, gateway, MAC) |
| SocketRecv | 23 | Received data (non-blocking read) |

| ObSetInfoClass | ID | Effect |
| -------------- | -- | ------ |
| SocketConnect | 18 | Initiate TCP connection or set UDP remote |
| SocketBind | 19 | Bind socket to local address/port |
| SocketListen | 20 | Start listening for TCP connections |
| SocketSend | 21 | Send data on connected socket |
| SocketClose | 22 | Close socket (FIN or RST) |
| SetNicIp | 27 | Set NIC IP address from userspace |

### TCP State Machine

11 states: `Closed → Listen / SynSent → SynReceived → Established → FinWait1/2, CloseWait, Closing, LastAck, TimeWait`.

### KWait Integration

| Wait Reason | Triggers On |
| ----------- | ----------- |
| `SocketRead` | Data arrives in socket recv buffer |
| `SocketConnect` | TCP handshake completes |
| `SocketAccept` | New connection arrives on listening socket |

### DHCP (Userspace)

DHCP runs as `dhcpd.nxe` (Ring 3). Uses UDP socket (port 68/67), performs DORA:
1. DISCOVER (broadcast) → OFFER
2. REQUEST → ACK
3. On ACK: `ob_set_info(SetNicIp)` to configure NIC IP
4. Lease renewal at 50% of lease time, fallback to APIPA (169.254.1.1)

Registry path: `\Registry\Machine\System\CurrentControlSet\Services\Network\Interfaces\0`

## Steps

### 1. Add a new protocol or modify existing

Add a new file in `src/net/` and register in `src/net/mod.rs`. Follow the pattern:

```rust
// In src/net/myproto.rs
use super::types::*;

pub const MY_PROTO: u8 = 123;

pub fn handle_my_proto(payload: &[u8], src_ip: Ipv4Addr, dst_ip: Ipv4Addr) {
    // process protocol data
}
```

Dispatch in `net_handle_incoming_packet()`:
```rust
} else if ip_hdr.protocol() == MY_PROTO {
    handle_my_proto(payload, ip_hdr.src_ip(), ip_hdr.dst_ip());
}
```

### 2. Create or modify a socket operation

```rust
// In socket.rs
pub fn socket_operation(socket_id: u32, op: SocketOp, args: &[u8]) -> Result<(), NetError> {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = mgr.get_mut(socket_id).ok_or(NetError::InvalidSocket)?;
    match op {
        SocketOp::Bind => { /* ... */ }
        SocketOp::Connect => { /* ... */ }
        // ...
    }
}
```

### 3. Send a packet

```rust
// Build Ethernet + IP + UDP
let eth = EthernetHeader::new(dst_mac, src_mac, ETH_TYPE_IPV4);
let ip = build_ipv4_header(src_ip, dst_ip, IPV4_PROTO_UDP, udp_len, 0);
let udp = build_udp_datagram(src_port, dst_port, data);

// Assemble and send
let mut pkt = Vec::new();
pkt.extend_from_slice(eth.as_bytes());
pkt.extend_from_slice(ip.as_bytes());
pkt.extend_from_slice(udp.as_bytes());
pkt.extend_from_slice(data);

let mut registry = NIC_REGISTRY.lock();
if let Some(nic) = registry.get_mut(nic_id) {
    let _ = nic.send_packet(&pkt);
}
```

### 4. Modify ARP cache behavior

```rust
// In arp.rs
pub fn arp_resolve(ip: Ipv4Addr, nic_id: u32) -> Option<MacAddr> {
    // Check cache first
    if let Some(mac) = arp_cache_lookup(ip) {
        return Some(mac);
    }
    // Send ARP request
    send_arp_request(ip, nic_id);
    None // caller must retry after response
}
```

### 5. Add a NIC driver

Implement `NetworkInterface` trait:

```rust
pub trait NetworkInterface {
    fn mac_address(&self) -> MacAddr;
    fn ip_address(&self) -> Ipv4Addr;
    fn set_ip_address(&mut self, ip: Ipv4Addr);
    fn poll_packet(&mut self, buf: &mut [u8]) -> Option<usize>;
    fn send_packet(&mut self, data: &[u8]) -> Result<(), NetError>;
    fn reset(&mut self);
}
```

Register in `NicRegistry` during `init_networking()`.

### 6. Modify userspace DHCP

Edit `userbin/dhcpd/src/main.rs`. The service uses libneodos socket API:

```rust
// Create UDP socket
let sock = libnet::socket_create(SocketType::Udp, 68);
libnet::socket_bind(sock, Ipv4Addr::unspecified(), 68);
libnet::socket_connect(sock, Ipv4Addr::broadcast(), 67);

// Send DISCOVER
libnet::socket_send(sock, dhcp_discover_packet());

// Receive OFFER
let mut buf = [0u8; 1500];
let len = libnet::socket_recv(sock, &mut buf);
```

### 7. Add a new ObInfoClass/ObSetInfoClass variant

1. Add variant to `ObInfoClass` or `ObSetInfoClass` in `src/object/types.rs`
2. Implement query/set in the socket's `ObOperation` impl (in `src/net/socket.rs`)
3. Add docs in `docs/networking/stack.md` and `docs/kernel/objects.md`
4. Add libneodos wrapper if userspace-accessible

### 8. Write network tests

Tests go in `src/net/tests.rs`, registered via `register_net_tests()` from `src/net/mod.rs`.

## Best practices

- Never allocate in IRQ context — NIC polling runs in process context (net_tick).
- ARP cache has 64 entries with 300s TTL — stale entries are evicted on insert.
- SocketManager has 64 max sockets — check return values.
- TCP buffers are 16 KB each (send + recv) — respect window size.
- DHCP is userspace-only — no kernel DHCP logic.
- Use `NIC_REGISTRY.lock()` sparingly — don't hold across packet construction.
- `ObType::Socket` = 18 — keep consistent with object types table.
- e1000 ring buffers are pre-allocated at init — no runtime allocation in RX/TX paths.

## Common mistakes

- Holding `NIC_REGISTRY.lock()` while calling `send_packet()` — can deadlock if NIC driver acquires same lock.
- Forgetting pseudo-header checksum in UDP — packets dropped by receiver.
- TCP state machine: transitioning from wrong state (e.g., `Send_data` in `SynSent`).
- Not draining the e1000 RX ring — buffer fills, packets dropped.
- Socket fd leak: `ob_create` socket but never `ob_destroy` on close.
- Modifying ABI-frozen ObInfoClass/ObSetInfoClass IDs (17-22) — must keep backward compat.
- ARP cache not updated on reply — subsequent packets use stale entry.
- DNS cache with unbounded growth — respect `DNS_MAX_CACHE(64)`.

## Final checklist

- [ ] Protocol changes tested with loopback or QEMU user networking
- [ ] TCP state machine transitions verified (all 11 states)
- [ ] Socket lifecycle: create → bind/connect → send/recv → close works
- [ ] ARP cache resolves correctly, stale entries evicted
- [ ] NIC driver (e1000) RX/TX rings not leaking or overflowing
- [ ] DHCP userspace service (dhcpd.nxe) compiles and runs
- [ ] ObInfoClass/ObSetInfoClass variants use correct IDs (no conflicts)
- [ ] libneodos wrappers added for new socket operations
- [ ] Tests registered via `register_net_tests()` and pass
- [ ] `docs/networking/stack.md` updated (new protocol, socket ops, event types)
- [ ] `cargo build` succeeds, `scripts/check_deps.py` passes
