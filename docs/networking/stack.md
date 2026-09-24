# Networking

## TCP/IP Stack

Directory: `src/net/` (12 files, ~2500 lines). Modular protocol stack with socket abstraction, NIC drivers, and ARP cache.

### Module Overview

| File | Lines | Responsibility |
| ------ | ------- | --------------- |
| `types.rs` | ~80 | `MacAddr`, `Ipv4Addr`, `SocketAddrV4`, `TcpState`, `SocketType`, `SocketDirection` |
| `ethernet.rs` | ~120 | Ethernet frame header (14 bytes: dst MAC, src MAC, ethertype), FCS computation, `build_ethernet_frame()` |
| `arp.rs` | ~250 | ARP protocol, 64-entry cache with 300s timeout, static entries, request/reply, `arp_resolve()` |
| `ipv4.rs` | ~300 | IPv4 header (20 bytes), header checksum, packet building, classification (loopback/multicast/link-local) |
| `icmp.rs` | ~100 | ICMP echo request/reply, checksum, `build_port_unreachable()` |
| `udp.rs` | ~150 | UDP header (8 bytes), pseudo-header checksum, `build_udp_datagram()` |
| `tcp.rs` | ~800 | TCP state machine (11 states), connection lifecycle, send/recv buffers (16 KB sliding window), segment building |
| `socket.rs` | ~700 | `SocketManager`, bind/connect/listen/send/recv/close, KWait wake, `udp_dispatch()`, `tcp_dispatch()` |
| `nic.rs` | ~210 | `NetworkInterface` trait (9 methods), `NicRegistry` (4 slots), IP/next-hop/gateway, vendor/device/description per NIC. NICs registered via NEM bridge |
| `net_bridge.rs` | ~100 | NEM network bridge: `hst_register_network_device`, wraps NEM callbacks as `NetworkInterface` |
| `counters.rs` | ~45 | Per-protocol packet/byte counters (RX/TX, ARP, ICMP), periodic dump every 1000 ticks |
| `tests.rs` | ~300 | 18+ integration tests |

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
| ---------- | ------ | --------- |
| 17 | `SocketInfo` | Socket state, type, local/remote addresses |
| 18 | `SocketAddr` | Bound address and port |
| 19 | `TcpStatus` | TCP connection state |
| 20 | `NicInfo` | NIC metadata: MAC, IP, link status, vendor/device PCI IDs, driver name, description |
| 23 | `SocketRecv` | Received data (non-blocking read) |

### ObSetInfoClass (mutate)

| Class ID | Name | Effect |
| ---------- | ------ | -------- |
| 18 | `SocketConnect` | Initiate TCP connection or set UDP remote |
| 19 | `SocketBind` | Bind socket to local address/port |
| 20 | `SocketListen` | Start listening for TCP connections |
| 21 | `SocketSend` | Send data on connected socket |
| 22 | `SocketClose` | Close socket (FIN or RST) |
| 27 | `SetNicIp` | Set NIC IP address from userspace |

Socket creation via `ob_create` with `attrs` encoding: bits 0-7 = socket type (1=TCP, 2=UDP, 3=Raw), bits 8-23 = port for well-known bindings.

## KWait Integration

Socket objects support KWait (kernel wait) for blocking I/O operations:

| Wait Reason | Triggers On |
| ------------- | ------------- |
| `SocketRead` | Data arrives in socket receive buffer |
| `SocketConnect` | TCP handshake completes (SYN+ACK received) |
| `SocketAccept` | New connection arrives on listening socket |

Blocking syscalls suspend the calling thread via KWait and wake when the corresponding event fires.

## Packet Flow

```text
NIC (e1000)
  -> poll_packet() -> 2048 byte buffer
    -> ethernet::EthernetHeader parse (dst MAC, src MAC, ethertype)
      |
      +-> ARP (ethertype 0x0806):
      |     -> arp_resolve() -> cache lookup (broadcast IP -> broadcast MAC directly)
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

## NIC Initialization (Phase 3.88)

1. Create `\Device\Tcp` and `\Device\Udp` namespace entries in Ob
2. Probe PCI bus for Intel e1000 devices (vendor 0x8086, devices 0x100E/0x1004/0x100F/0x10D3)
3. Initialize found NIC: map MMIO BAR, allocate RX/TX ring buffers, configure descriptors
4. Set initial IP configuration (default: 0.0.0.0 — no kernel DHCP)
5. Userspace DHCP service (`dhcpd.nxe`) configures IP asynchronously
6. ARP cache begins accepting entries

### NicRegistry

Manages up to 4 NIC slots. Each slot holds a `Box<dyn NetworkInterface>`.

```rust
pub struct NicRegistry {
    pub nics: [Option<NicSlot>; 4],
}
```

## DHCP Client — Userspace Service

**DHCP is NOT handled by the kernel.** The protocol runs entirely in userspace as the `dhcpd.nxe` service, following the NT-like design principle where protocol stacks live in user mode and the kernel only provides transport primitives.

### Architecture

```text
dhcpd.nxe (Ring 3 user service)
  │
  ├─ Creates UDP socket (ObType::Socket = 18)
  │   ├─ bind(0.0.0.0:68)        ← DHCP client port
  │   └─ connect(255.255.255.255:67)  ← DHCP server port (broadcast)
  │
  ├─ Performs DORA sequence:
  │   ├─ DISCOVER → wait → OFFER
  │   ├─ REQUEST  → wait → ACK
  │   └─ On ACK: set NIC IP via libnet::set_ip()
  │
  ├─ Manages lease renewal at 50% of lease time
  ├─ Falls back to APIPA (169.254.1.1) if DHCP fails
  └─ Persists IP configuration to Registry

Kernel (Ring 0) provides:
  ├─ NIC access (e1000 kernel stub or NEM driver)
  ├─ Ethernet / ARP / IPv4 / UDP protocol processing
  ├─ Socket abstraction with recv/send via Ob syscalls
  └─ No DHCP protocol logic — removed in v0.48.8+
```

### DORA Sequence (RFC 2131)

1. **DISCOVER**: Broadcast UDP packet with DHCPDISCOVER message type
2. **OFFER**: Received on bound UDP socket (port 68), parsed for offered IP and options
3. **REQUEST**: Unicast or broadcast DHCPREQUEST with offered server ID
4. **ACK**: Final acknowledgment, IP is configured via `ob_set_info(SetNicIp)`

### Lease Renewal

After binding, `dhcpd.nxe` enters a loop that:

- Tracks elapsed time via `sys_yield()` batches (approximating seconds)
- At 50% of lease time, sends unicast DHCPREQUEST to the serving DHCP server
- On ACK: lease is extended, timer resets
- On failure: retries up to `MAX_RETRIES`, then restarts DORA

### Why Userspace?

The previous kernel-based DHCP implementation had fundamental architectural problems:

- `build_dhcp_packet()` used `Vec` (heap allocation) from timer IRQ context
- `nic_send_packet()` acquired `NIC_REGISTRY.lock()` (spinlock) from IRQ context, risking deadlock
- `dhcp_tick()` in the idle loop never ran because user threads were always `Ready`
- Result: DHCP never progressed, and `netcfg` always fell back to APIPA

Moving DHCP to userspace resolves all these issues:

- All memory allocation happens in process context (safe)
- No spinlocks held in IRQ context
- Socket operations are fully preemptible
- Clean separation of concerns: kernel transports packets, user mode runs protocols

### Registry Integration

`dhcpd.nxe` stores network configuration in:

```text
\Registry\Machine\System\CurrentControlSet\Services\Network\Interfaces\0
  DHCPEnabled = 1 (DWORD)
  IPAddress   = <assigned IP> (DWORD, big-endian byte order)
  SubnetMask  = <subnet mask> (DWORD)
  DHCPBound   = 1/0 (DWORD, 1 when DHCP lease is active)
  DHCPServer  = <server IP> (DWORD)
```

## QEMU Networking

NeoDOS uses QEMU's user-mode networking (SLiRP) by default, which requires **no root/sudo privileges**.

### Default Mode (sudo-free)

```bash
-netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1 \
-device e1000,netdev=net0
```

QEMU's built-in DHCP server assigns IPs in the 10.0.1.x range starting at 10.0.1.80.
The host is accessible at 10.0.1.1 and provides NAT to the outside world.

### TAP Mode (requires privileges)

```bash
bash scripts/qemu-debug.sh --tap
```

TAP networking requires:

- `/dev/net/tun` access (requires `CAP_NET_ADMIN` or root)
- A preconfigured `tap0` interface on the host

Useful when the guest needs direct network access (e.g., DHCP from a real LAN server).

## Source Files

| File | Path |
| ------ | ------ |
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
| tests.rs | `src/net/tests.rs` |

User-mode DHCP service: `userbin/dhcpd/src/main.rs`

## ARP Resolution Flow

When a packet needs to be sent to an IP address (e.g., ICMP ping), the ARP resolution follows this sequence:

1. **Cache lookup**: `arp_lookup(target_ip)` checks the 64-entry ARP cache.
   - Broadcast IP → broadcast MAC (no ARP needed).
   - Cache hit → returns MAC immediately.
   - Cache miss → proceeds to send ARP request.

2. **ARP Request**: An Ethernet frame is built with:
   - Destination MAC: `FF:FF:FF:FF:FF:FF` (broadcast)
   - Ethertype: `ETH_TYPE_ARP (0x0806)`
   - ARP operation: `ARP_OP_REQUEST (1)`
   - Sender MAC/IP: NIC's MAC and IP
   - Target IP: the IP to resolve

3. **Wait for Reply**: The sender polls `network_poll_all()` in a tight loop with RDTSC-based timeout (500ms). Each poll checks for received packets via the e1000 RX ring.

4. **ARP Reply Reception**: `net_handle_incoming_packet()` processes incoming Ethernet frames:
   - If ethertype is `ETH_TYPE_ARP (0x0806)` and operation is `ARP_OP_REPLY (2)`:
     - Extracts sender IP and MAC from the ARP payload
     - Calls `arp_insert()` to add/update the cache entry
   - `arp_insert()` updates existing entries or inserts new ones (LRU eviction at 64 entries)

5. **Resolution Complete**: The polling loop in `icmp_ping()` detects the cache entry and returns the resolved MAC address.

6. **ICMP Echo Request**: The resolved MAC is used as the destination in the Ethernet frame, and the ICMP echo request is sent.

7. **ICMP Echo Reply Wait**: Polls `LAST_PING_REPLY` atomic with RDTSC-based timeout (configurable, default 1s).

### Important implementation details

- The `icmp_ping()` function in `icmp.rs` contains its own ARP resolution logic (inline in `or_else` closure) rather than calling `arp_resolve()`. This is because `icmp_ping()` needs to block waiting for the reply, while `arp_resolve()` is fire-and-forget.
- The ARP request Ethernet frame **must** use `ETH_TYPE_ARP (0x0806)` as the ethertype. Using `ETH_TYPE_IPV4 (0x0800)` will cause the receiver to reject the frame.
- The ARP cache is protected by `spin::Mutex`. Entries expire after 300 seconds (checked every 10 ticks).
- QEMU user-mode (SLiRP) networking supports ARP. VirtualBox Bridge Mode forwards ARP to the physical network.

### Known limitations

- Only one ARP resolution can be in-flight at a time (no pending queue for concurrent requests).
- The e1000 RX descriptor status is read without explicit `read_volatile`, relying on the compiler generating fresh memory reads via `&mut self` reference.
- Gratuitous ARP sent automatically on IP address change via `nic_set_ip()`.

## Tests

17+ tests in `src/net/tests.rs` covering: MAC address formatting, IPv4 header checksum, ARP cache operations, TCP state machine transitions, TCP full lifecycle (listen -> connect -> established -> close), ICMP echo request/reply, socket creation/lookup/bind/connect, UDP header construction, NIC registry add/remove.

Kernel DHCP tests removed (DHCP is now a userspace service).
