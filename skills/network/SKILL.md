---
name: network
description: Modify the TCP/IP stack, sockets, NIC integration, ARP, DNS/DHCP userland
---

# Networking

## When to use

Modifying the TCP/IP stack, the socket abstraction, NIC integration, the ARP
cache, the DNS resolver, or the DHCP user service.

## Goal

Implement network changes with correct protocol handling, socket lifecycle, NIC
management, and userspace DHCP/DNS integration.

## References

- `docs/networking/stack.md`, `docs/networking/userland.md`
- `src/net/` — `types.rs`, `ethernet.rs`, `arp.rs`, `ipv4.rs`, `icmp.rs`,
  `udp.rs`, `tcp.rs`, `socket.rs`, `nic.rs`, `loopback.rs`, `counters.rs`,
  `dns.rs` (kernel-internal), `mod.rs`, `tests/`
- NEM NIC driver: `drivers/e1000/`; kernel bridge
  `src/drivers/nem/loader/net_bridge.rs`
- `libdns/src/lib.rs` — DNS wire format + resolver core (host-testable)
- `libnet/src/dns.rs` — userland transport + Registry server config
- `userbin/dhcpd`, `userbin/netapplier`, `userbin/nslookup`, `userbin/ping`
- `src/object/types.rs` — `ObType::Socket = 18`

## Architecture

- `nic.rs` defines `NetworkInterface` and `NicRegistry` (4 slots). The Intel
  e1000 is a **NEM driver** (`drivers/e1000/`) registered through the net bridge —
  there is no kernel `src/net/e1000.rs`.
- Loopback (`loopback.rs`, `127.0.0.0/8`, #484): a virtual interface **outside**
  `NicRegistry`, drained through the single dispatch path
  `net_handle_incoming_packet()`; `\Device\Loopback`; synthetic MAC
  `02:00:00:00:00:01`.
- Counters (`counters.rs`) expose per-interface RX/TX stats via
  `ObInfoClass::NetStats (28)`.
- TCP: 11-state machine, 16 KB sliding window. `tcp_send` buffers; `tcp_tick()`
  (from `net_tick`) flushes up to MSS, honors `send_base`/`send_next`,
  retransmits on a 200 ms RTO, and drives FIN. `tcp_dispatch()` must not hold
  `SOCKET_MANAGER` across protocol actions.
- RX is driven by the Ring-0 `netpump` kernel thread and the `sys_yield` path —
  the e1000 is polled, not interrupt-driven.
- Packet flow: e1000 → ethernet → ARP(`0x0806`)/IPv4(`0x0800`) →
  ICMP(1)/TCP(6)/UDP(17) → socket + KWait wake.

## Ob integration

`ObType::Socket = 18`. Sockets are created via `ob_create` with `attrs` encoding
bits 0-7 = type (1=TCP, 2=UDP, 3=Raw), bits 8-23 = port.

| ObInfoClass | ID | Returns |
| ------------ | -- | --------- |
| `SocketInfo` | 17 | Socket state, type, local/remote addresses |
| `SocketAddr` | 18 | Bound address and port |
| `TcpStatus` | 19 | TCP connection state |
| `NicInfo` | 20 | NIC metadata (MAC, IP, link, PCI IDs, name, description) |
| `SocketRecv` | 23 | Received data (non-blocking) |
| `NetStats` | 28 | Per-interface counters |

| ObSetInfoClass | ID | Effect |
| -------------- | -- | -------- |
| `SocketConnect` | 18 | Connect TCP / set UDP remote |
| `SocketBind` | 19 | Bind socket |
| `SocketListen` | 20 | Listen (TCP) |
| `SocketSend` | 21 | Send on a connected socket |
| `SocketClose` | 22 | Close (FIN/RST) |
| `SetNicIp` | 27 | Set NIC IP/mask |
| `SetNicGateway` | 28 | Set NIC gateway (`0.0.0.0` = unset) |
| `SocketBindNic` | 29 | Pin a socket to a NIC (send interface) |

KWait reasons: `SocketRead`, `SocketConnect`, `SocketAccept`.

## DHCP / DNS (userland)

- DHCP is **not** in the kernel. `userbin/dhcpd` (Ring 3) performs DORA on a UDP
  socket (68/67), supervises the lease (renew at 50%, rebind at 87.5%, APIPA
  fallback `169.254.1.1`), and **publishes** config to the Registry; the resident
  `userbin/netapplier` service applies it to the NIC (`SetNicIp`/`SetNicGateway`).

  ```text
  \Registry\Machine\System\CurrentControlSet\Services\Network\Interfaces\0
  ```

- DNS: `libdns` (wire format) + `libnet/src/dns.rs` (UDP via `net.nxl`, Registry
  servers `DnsServer`, `DnsServer2/3`, bounded cache). `nslookup`/`ping` consume it.
  `src/net/dns.rs` is kernel-internal (tests/`dns_tick`) and not exposed to userland.

## Steps

1. **New protocol / change**: add/modify a file in `src/net/` and dispatch from
   `net_handle_incoming_packet()` in `src/net/mod.rs`.
2. **Socket ops**: edit `src/net/socket.rs` (`SocketManager`, 64 sockets).
3. **Send a packet**: build Ethernet + IP + transport with the helpers in
   `ethernet.rs`/`ipv4.rs`/`udp.rs`/`tcp.rs`, then send via `NIC_REGISTRY`.
4. **NIC**: implement `NetworkInterface`; physical NICs are registered through the
   NEM net bridge, not as a kernel module.
5. **Ob classes**: add variants in `src/object/types.rs`, implement in the socket
   `ObOperations`, document in `docs/networking/stack.md` and
   `docs/kernel/objects.md`, add a libneodos wrapper.
6. **Userland**: edit `userbin/dhcpd`, `userbin/netapplier`, and `libnet`/`libdns`
   for protocol/service changes.
7. **Tests**: `src/net/tests/`, registered via `register_net_tests()`.
8. **Build and test**

   ```bash
   cd neodos-kernel && cargo build
   neodev build --image && neodev test
   neodev check-deps
   ```

## Best practices

- Never allocate in IRQ context — NIC polling runs in process context
  (`netpump` / `sys_yield`).
- Don't hold `NIC_REGISTRY` (or `SOCKET_MANAGER`) while calling `send_packet` or
  protocol handlers.
- DHCP and DNS are userland-only; the kernel provides transport primitives.
- ARP cache: 64 entries, 300 s TTL, LRU eviction; gratuitous ARP on IP change.
- Respect ABI-frozen Ob info/set class IDs.

## Common mistakes

- Looking for a kernel e1000 module — it is a NEM driver under `drivers/e1000/`.
- Holding a socket/NIC lock across protocol actions (deadlock).
- Missing the UDP pseudo-header checksum.
- Implementing DHCP/DNS in the kernel.
- Socket fd leak (create without destroy on close).
- Matching UDP replies by the wrong port (use the socket's local port).

## Final checklist

- [ ] Protocol/socket changes tested (loopback or QEMU user networking)
- [ ] TCP state transitions valid (all 11 states)
- [ ] Socket lifecycle create → bind/connect → send/recv → close works
- [ ] ARP resolves; stale entries evicted
- [ ] e1000 (NEM) RX/TX rings don't leak or overflow
- [ ] `dhcpd` / `netapplier` compile and run
- [ ] Ob info/set class IDs correct (no conflicts)
- [ ] libneodos wrappers added for new socket ops
- [ ] `register_net_tests()` wired; tests pass
- [ ] `docs/networking/stack.md` updated
- [ ] `cargo build` + `neodev check-deps` pass
