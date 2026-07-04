# Networking

## TCP/IP Stack

Directory: `src/net/` (13 files, ~3165 lines). Modular protocol stack with socket abstraction, NIC drivers, and DHCP client.

### Module Overview

| File | Lines | Responsibility |
|------|-------|---------------|
| `types.rs` | ~80 | `MacAddr`, `Ipv4Addr`, `SocketAddrV4`, `TcpState`, `SocketType`, `SocketDirection` |
| `ethernet.rs` | ~120 | Ethernet frame header (14 bytes: dst MAC, src MAC, ethertype), FCS computation, `build_ethernet_frame()` |
| `arp.rs` | ~250 | ARP protocol, 64-entry cache with 300s timeout, static entries, request/reply, `arp_resolve()` |
| `ipv4.rs` | ~300 | IPv4 header (20 bytes), header checksum, packet building, classification (loopback/multicast/link-local) |
| `icmp.rs` | ~100 | ICMP echo request/reply, checksum, `build_port_unreachable()` |
| `udp.rs` | ~150 | UDP header (8 bytes), pseudo-header checksum, `build_udp_datagram()` |
| `tcp.rs` | ~800 | TCP state machine (11 states), connection lifecycle, send/recv buffers (16 KB sliding window), segment building |
| `socket.rs` | ~700 | `SocketManager`, bind/connect/listen/send/recv/close, KWait wake, `udp_dispatch()`, `tcp_dispatch()` |
| `nic.rs` | ~150 | `NetworkInterface` trait, `NicRegistry` (4 slots), IP/next-hop/gateway management |
| `e1000.rs` | ~350 | Intel e1000 NIC driver (82540EM, 82543GC, 82545EM, 82574L), ring buffers, RX/TX descriptors, MMIO |
| `dhcp.rs` | ~200 | DHCP client (RFC 2131), DORA sequence, auto-start on boot, lease renewal |
| `tests.rs` | ~300 | 17+ integration tests |

### TCP State Machine

11 states defined in `TcpState`:

```rust
pub enum TcpState {
    Closed,      // Initial state, no connection
    Listen,      // Waiting for SYN
    SynSent,     // SYN sent, awaiting SYN+ACK
    SynRcvd,     // SYN received, SYN+ACK sent
    Established, // Connection established, data transfer
    FinWait1,    // FIN sent, awaiting ACK
    FinWait2,    // FIN ACKed, awaiting peer's FIN
    CloseWait,   // FIN received, waiting for app to close
    Closing,     // Both FINs sent, awaiting final ACK
    LastAck,     // FIN received, FIN sent, awaiting FIN ACK
    TimeWait,    // All done, waiting for delayed segments
}
```

Connection lifecycle: `build_tcp_segment()`, `send_tcp_segment()`, `tcp_send_syn_ack()`, `tcp_handle_ack()`. Send/recv buffers use a 16 KB sliding window.

## ObType Integration

`ObType::Socket` = 18. Sockets are Ob objects managed through the standard handle/namespace system.

### ObInfoClass (query)

| Class ID | Name | Returns |
|----------|------|---------|
| 17 | `SocketInfo` | Socket state, type, local/remote addresses |
| 18 | `SocketAddr` | Bound address and port |
| 19 | `TcpStatus` | TCP connection state |
| 20 | `NicInfo` | NIC configuration (IP, gateway, MAC) |
| 23 | `SocketRecv` | Received data (non-blocking read) |

### ObSetInfoClass (mutate)

| Class ID | Name | Effect |
|----------|------|--------|
| 18 | `SocketConnect` | Initiate TCP connection to remote addr |
| 19 | `SocketBind` | Bind socket to local address/port |
| 20 | `SocketListen` | Start listening for TCP connections |
| 21 | `SocketSend` | Send data on connected socket |
| 22 | `SocketClose` | Close socket (FIN or RST) |

Socket creation via `ob_create` with `attrs` encoding: bits 0-7 = socket type (1=TCP, 2=UDP, 3=Raw), bits 8-23 = port for well-known bindings.

## KWait Integration

Socket objects support KWait (kernel wait) for blocking I/O operations:

| Wait Reason | Triggers On |
|-------------|-------------|
| `SocketRead` | Data arrives in socket receive buffer |
| `SocketConnect` | TCP handshake completes (SYN+ACK received) |
| `SocketAccept` | New connection arrives on listening socket |

Blocking syscalls suspend the calling thread via KWait and wake when the corresponding event fires.

## Packet Flow

```
NIC (e1000)
  -> poll_packet() -> 2048 byte buffer
    -> ethernet::EthernetHeader parse (dst MAC, src MAC, ethertype)
      |
      +-> ARP (ethertype 0x0806):
      |     -> arp_resolve() -> cache lookup
      |     -> if request for our IP: build reply
      |     -> if reply: update cache
      |
      +-> IPv4 (ethertype 0x0800):
            -> Ipv4Header parse (version, IHL, total_len, protocol)
              |
              +-> ICMP (proto 1):
              |     -> echo request -> build echo reply -> send
              |     -> port unreachable -> build and send
              |
              +-> TCP (proto 6):
              |     -> tcp_dispatch() -> socket lookup by (src IP, src port, dst IP, dst port)
              |     -> segment processing (SYN, ACK, FIN, data)
              |     -> data delivery to socket recv buffer
              |     -> KWait wake for SocketRead/SocketConnect
              |
              +-> UDP (proto 17):
                    -> udp_dispatch() -> socket lookup by (dst port)
                    -> data delivery to socket recv buffer
                    -> KWait wake for SocketRead
```

Each receive direction produces a `NetPacket` struct that is consumed by the protocol handler. Unhandled protocols are silently dropped.

## NIC Initialization (Phase 3.88)

1. Create `\Device\Tcp` and `\Device\Udp` namespace entries in Ob
2. Probe PCI bus for Intel e1000 devices (vendor 0x8086, devices 0x100E/0x1004/0x100F/0x10D3)
3. Initialize found NIC: map MMIO BAR, allocate RX/TX ring buffers, configure descriptors
4. Set initial IP configuration (default: 0.0.0.0 until DHCP completes)
5. Start DHCP client in auto-renewal mode
6. ARP cache begins accepting entries

### NicRegistry

Manages up to 4 NIC slots. Each slot holds a `Box<dyn NetworkInterface>`.

```rust
pub struct NicRegistry {
    pub nics: [Option<NicSlot>; 4],
}
```

## DHCP Client

File: `src/net/dhcp.rs`. Standard DORA sequence per RFC 2131:

1. **DISCOVER**: broadcast DHCP discover (port 67 -> 255.255.255.255:68)
2. **OFFER**: receive DHCP offer from server (Yiaddr = offered IP)
3. **REQUEST**: broadcast DHCP request with offered server IP
4. **ACK**: receive DHCP acknowledgment with lease, mask, gateway, DNS

On success: updates NIC IP, subnet mask, default gateway. Stores lease information. Lease renewal runs on a configurable timer (default: 50% of lease time). Registry integration stores DHCPEnabled flag at `\Registry\Machine\Network\Interfaces\0\DHCPEnabled`.

## Source Files

| File | Path |
|------|------|
| types.rs | `src/net/types.rs` |
| ethernet.rs | `src/net/ethernet.rs` |
| arp.rs | `src/net/arp.rs` |
| ipv4.rs | `src/net/ipv4.rs` |
| icmp.rs | `src/net/icmp.rs` |
| udp.rs | `src/net/udp.rs` |
| tcp.rs | `src/net/tcp.rs` |
| socket.rs | `src/net/socket.rs` |
| nic.rs | `src/net/nic.rs` |
| e1000.rs | `src/net/e1000.rs` |
| dhcp.rs | `src/net/dhcp.rs` |
| tests.rs | `src/net/tests.rs` |

## Tests

17+ tests in `src/net/tests.rs` covering: MAC address formatting, IPv4 header checksum, ARP cache operations, TCP state machine transitions, TCP full lifecycle (listen -> connect -> established -> close), ICMP echo request/reply, socket creation/lookup/bind/connect, UDP header construction, NIC registry add/remove.
