# NeoDOS Userland Networking + System Configuration + Package Architecture

**Versión:** v0.2 (diseño detallado)
**Fecha:** 2026-07-01
**Estado:** Borrador — Pendiente de implementación

---

## Índice

1. [Visión General](#1-visión-general)
2. [net.nxl — Librería de Red Userland](#2-netnxl--librería-de-red-userland)
3. [Herramientas NXE de Red](#3-herramientas-nxe-de-red)
4. [NeoInit System Configuration](#4-neoinit-system-configuration)
5. [Registry Usage Model](#5-registry-usage-model)
6. [NeoPkg — Package System](#6-neopkg--package-system)
7. [Necesidades del Kernel](#7-necesidades-del-kernel)
8. [Roadmap](#8-roadmap)
9. [Diagramas de Flujo](#9-diagramas-de-flujo)
10. [Consideraciones de Implementación](#10-consideraciones-de-implementación)
11. [Apéndice: Cambios por Archivo](#11-apéndice-cambios-por-archivo)

---

## 1. Visión General

Este documento define la arquitectura para exponer el subsistema de red del kernel
a procesos userland mediante NXL libraries, establece el modelo de configuración
del sistema usando Registry + filesystem, y sienta las bases del sistema de paquetes
(NeoPkg).

### 1.1 Principios de diseño

| Principio | Descripción |
|-----------|-------------|
| **Separación de capas** | net.nxl no toca hardware, no conoce e1000, no implementa TCP/IP |
| **Configuración estructural** en Registry | Solo estructura del sistema, no datos de usuario |
| **Datos de usuario** en NeoFS | Logs, perfiles, documentos, configuraciones editables |
| **Componentes CORE protegidos** | NeoPkg no permite eliminar kernel/NeoInit/NeoFS |
| **API unificada via Ob** | Todas las operaciones de red usan `ob_create(ObType::Socket)`, `ob_set_info`, `ob_query_info` |
| **Carga bajo demanda** | net.nxl se carga solo cuando se necesita, no al boot |
| **No bloqueo en userland** | Llamadas bloqueantes (recv, connect) usan KWait/ObWait |

### 1.2 Stack completo

```
Aplicaciones NXE                          ← Ring 3
  (ipconfig.nxe, ping.nxe, dhcp.nxe)
        |
        v
System Libraries NXL                      ← Ring 3
  (net.nxl)
        |
        v
libneodos (Ob API wrappers)               ← Ring 3
  sys_ob_create, sys_ob_set_info, sys_ob_query_info
        |
        v  (INT 0x80)
├──────────────────────────────────────┤
│ Kernel syscall handlers               │  ← Ring 0
│   handler_ob_create → ObType::Socket  │
│   handler_ob_set_info → SocketSend    │
│   handler_ob_query_info → NicInfo     │
├──────────────────────────────────────┤
        |
        v
Kernel Socket Manager                    ← Ring 0
  (src/net/socket.rs)
        |
        v
Kernel TCP/IP Stack                      ← Ring 0
  (src/net/{tcp,udp,ipv4,icmp,arp,ethernet}.rs)
        |
        v
Kernel NIC Registry / e1000 driver       ← Ring 0
  (src/net/{nic,e1000}.rs)
        |
        v
Hardware (e1000 NIC)                     ← Hardware
```

---

## 2. net.nxl — Librería de Red Userland

### 2.1 Responsabilidad

`net.nxl` es la API userland para operaciones de red. Se carga en el slot NXL 3
(`0x1e0c0000`) bajo demanda.

**No debe:**
- Acceder a hardware (MMIO, PCI, DMA)
- Conocer e1000 ni ningún NIC driver específico
- Implementar protocolos TCP/IP, ARP, ICMP, DHCP (eso es kernel o app)
- Gestionar buffers DMA o descriptores de anillo
- Hacer operaciones bloqueantes fuera del mecanismo ObWait

**Debe proporcionar:**
- Abstracción de interfaces de red (listar, consultar estado)
- Abstracción de sockets (crear, conectar, enviar, recibir, cerrar)
- Consulta de estadísticas de red
- Configuración de IP/gateway (delegado al kernel via ob_set_info)
- (Futuro) resolución DNS

### 2.2 API de net.nxl

Cada función de net.nxl llama a syscalls Ob. La tabla de exports de net.nxl
se lee como:

```rust
let net = *(0x1e0c0000 as *const NetAbiTable);
```

#### 2.2.1 Structs compartidas

```rust
#[repr(C)]
pub struct NetInterfaceInfo {
    pub nic_id: u32,
    pub mac: [u8; 6],              // 6 bytes
    pub _pad1: [u8; 2],            // alinear a 4
    pub ipv4: [u8; 4],
    pub subnet_mask: [u8; 4],
    pub gateway: [u8; 4],
    pub dns_server: [u8; 4],
    pub link_up: u8,                // 0=down, 1=up
    pub _pad2: [u8; 7],            // alinear a 8
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub speed_mbps: u32,
    pub driver_name: [u8; 32],      // null-terminated
}
// Tamaño total: 100 bytes

#[repr(C)]
pub struct NetStats {
    pub interfaces: u32,
    pub total_rx_packets: u64,
    pub total_tx_packets: u64,
    pub total_rx_bytes: u64,
    pub total_tx_bytes: u64,
    pub arp_entries: u32,
    pub socket_count: u32,
    pub _pad: [u8; 4],
}

#[repr(C)]
pub struct NetSocketAddr {
    pub ip: [u8; 4],
    pub port: u16,                  // network byte order (big-endian)
    pub _pad: [u8; 2],
}

#[repr(C)]                          // para SocketInfo query
pub struct NetSocketInfo {
    pub socket_type: u32,           // 1=TCP, 2=UDP, 3=Raw
    pub direction: u32,             // 0=None, 1=Connecting, 2=Connected, 3=Listening, 4=Closed
    pub local_ip: [u8; 4],
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub recv_available: u32,        // bytes disponibles en recv_buf del kernel
    pub tcp_state: u32,            // solo para TCP
}
```

#### 2.2.2 Funciones de interfaz

```rust
/// Número de interfaces de red disponibles.
pub fn net_interface_count() -> Result<u32, NetError>;

/// Obtener información de una interfaz por índice.
pub fn net_get_interface_info(nic_id: u32) -> Result<NetInterfaceInfo, NetError>;

/// Obtener MAC de una interfaz como bytes.
pub fn net_get_mac(nic_id: u32) -> Result<[u8; 6], NetError>;

/// Obtener dirección IPv4 como bytes.
pub fn net_get_ip(nic_id: u32) -> Result<[u8; 4], NetError>;

/// Obtener gateway como bytes.
pub fn net_get_gateway(nic_id: u32) -> Result<[u8; 4], NetError>;

/// Obtener estadísticas globales de red.
pub fn net_get_stats() -> Result<NetStats, NetError>;
```

**Implementación concreta:**

```rust
pub fn net_interface_count() -> Result<u32, NetError> {
    let fd = syscall::sys_ob_open("\\Global\\Info\\NicInfo", ob_access::READ)?;
    let mut buf = [0u8; 2048];
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::NicInfo, &mut buf)?;
    syscall::sys_close(fd)?;

    // NicInfo devuelve array de NetInterfaceInfo. Count = n / sizeof(NetInterfaceInfo)
    let count = n / core::mem::size_of::<NetInterfaceInfo>();
    Ok(count as u32)
}

pub fn net_get_interface_info(nic_id: u32) -> Result<NetInterfaceInfo, NetError> {
    let fd = syscall::sys_ob_open("\\Global\\Info\\NicInfo", ob_access::READ)?;
    let mut buf = [0u8; 2048];
    let _n = syscall::sys_ob_query_info(fd, ObInfoClass::NicInfo, &mut buf)?;
    syscall::sys_close(fd)?;

    let size = core::mem::size_of::<NetInterfaceInfo>();
    let offset = nic_id as usize * size;
    if offset + size > buf.len() {
        return Err(NetError::NotFound);
    }
    let ptr = &buf[offset] as *const u8 as *const NetInterfaceInfo;
    Ok(unsafe { *ptr })
}
```

**Sobre stats:** El kernel expondrá un nuevo `\Global\Info\NetworkStats` con
ObInfoClass::NetworkStats (21). En la primera versión, se calcula sumando las
stats de todas las NICs vía NicInfo.

#### 2.2.3 Funciones de socket

```rust
/// Crear un socket. `path` es nombre en namespace Ob (ej: "\\MyApp\\Ping").
/// `socket_type`: SOCKET_TCP(1), SOCKET_UDP(2), SOCKET_RAW(3).
pub fn net_socket_create(path: &str, socket_type: u32) -> Result<u8, NetError> {
    let attrs = socket_type & 0xFF;
    let fd = syscall::sys_ob_create(path, ob_type::SOCKET, None, attrs as u64)?;
    Ok(fd)
}

/// Vincular socket a dirección local.
pub fn net_socket_bind(fd: u8, addr: &NetSocketAddr) -> Result<(), NetError> {
    let bytes = addr_as_bytes(addr);  // 8 bytes: ip[4] + port[2] + pad[2]
    syscall::sys_ob_set_info(fd, ob_set_info_class::SOCKET_BIND, &bytes)?;
    Ok(())
}

/// Conectar a destino remoto.
pub fn net_socket_connect(fd: u8, addr: &NetSocketAddr) -> Result<(), NetError> {
    let bytes = addr_as_bytes(addr);
    syscall::sys_ob_set_info(fd, ob_set_info_class::SOCKET_CONNECT, &bytes)?;
    // Para TCP, esperar conexión establecida:
    syscall::sys_ob_wait(fd)?;     // bloquea hasta Connected o error
    Ok(())
}

/// Poner en escucha (solo TCP).
pub fn net_socket_listen(fd: u8) -> Result<(), NetError> {
    syscall::sys_ob_set_info(fd, ob_set_info_class::SOCKET_LISTEN, &[])?;
    Ok(())
}

/// Enviar datos. Devuelve bytes enviados.
pub fn net_socket_send(fd: u8, data: &[u8]) -> Result<usize, NetError> {
    let n = syscall::sys_ob_set_info(fd, ob_set_info_class::SOCKET_SEND, data)?;
    Ok(n)
}

/// Recibir datos. Devuelve bytes recibidos (0 = EOF/closed).
pub fn net_socket_recv(fd: u8, buf: &mut [u8]) -> Result<usize, NetError> {
    // --- Estrategia de receive ---
    // 1. Consultar SocketInfo para ver cuántos bytes hay en recv_buf del kernel
    // 2. Si hay datos, llamar ob_query_info(fd, SocketInfo) que también copia datos
    // 3. Si no hay datos y socket abierto, hacer ob_wait(fd) y reintentar
    //
    // El kernel expone SocketInfo (class 17) con un campo recv_available.
    // Para recibir datos reales, usamos un nuevo ObInfoClass::SocketRecv(23).
    // O bien extendemos SocketInfo para incluir payload inline.

    // === Alternativa 1: SocketRecv query ===
    let mut recv_buf = [0u8; 2048];
    let n = syscall::sys_ob_query_info(fd, ObInfoClass::SocketRecv, &mut recv_buf)?;
    let copy_len = n.min(buf.len());
    buf[..copy_len].copy_from_slice(&recv_buf[..copy_len]);
    Ok(copy_len)

    // === Alternativa 2: ob_wait + consulta ===
    // Loop: ob_wait(fd) → consulta SocketInfo → si recv_available > 0, copiar
}

/// Cerrar socket.
pub fn net_socket_close(fd: u8) -> Result<(), NetError> {
    syscall::sys_ob_set_info(fd, ob_set_info_class::SOCKET_CLOSE, &[])?;
    syscall::sys_close(fd)?;
    Ok(())
}

/// Obtener estado TCP.
pub fn net_get_tcp_status(fd: u8) -> Result<TcpState, NetError> {
    let mut buf = [0u8; 4];
    syscall::sys_ob_query_info(fd, ObInfoClass::TcpStatus, &mut buf)?;
    let state = u32::from_le_bytes(buf);
    Ok(TcpState::from(state))
}

/// Obtener dirección local y remota.
pub fn net_get_socket_addr(fd: u8) -> Result<(NetSocketAddr, NetSocketAddr), NetError> {
    let mut buf = [0u8; 32];  // SocketAddr info class
    syscall::sys_ob_query_info(fd, ObInfoClass::SocketAddr, &mut buf)?;
    let local = NetSocketAddr { ip: buf[0..4].try_into().unwrap(), port: u16::from_be_bytes(buf[4..6].try_into().unwrap()), _pad: [0;2] };
    let remote = NetSocketAddr { ip: buf[8..12].try_into().unwrap(), port: u16::from_be_bytes(buf[12..14].try_into().unwrap()), _pad: [0;2] };
    Ok((local, remote))
}
```

#### 2.2.4 Errores de net.nxl

```rust
#[repr(i64)]
pub enum NetError {
    Ok = 0,
    NotFound = -2,
    AccessDenied = -4,
    InvalidFd = -5,
    NotSupported = -7,
    WouldBlock = -8,     // no hay datos disponibles (non-blocking)
    NoMemory = -3,
    ConnectionRefused = -100,
    ConnectionReset = -101,
    ConnectionTimeout = -102,
    NetworkDown = -103,
    HostUnreachable = -104,
    DnsResolveFailed = -200,
}
```

**Nota:** Los códigos negativos > -16 son específicos de red, no entran en
conflicto con `SyscallError` (-1 a -15) porque los syscall wrappers traducen
los códigos de error del kernel (que siempre están en -1..-15) a errores de
net.nxl.

#### 2.2.5 net_socket_recv — diseño detallado

`net_socket_recv` es la función más compleja. El kernel actual no tiene un
`ObInfoClass` específico para receive de datos de socket. Dos opciones:

**Opción A: Nuevo ObInfoClass::SocketRecv = 23**

Añadir en el kernel `ObInfoClass::SocketRecv = 23`. El handler en
`src/syscall/ob.rs`:

```rust
_ if info_class == ObInfoClass::SocketRecv as u32 => {
    // Obtener socket_id del ObObject native_id
    let ob = ob_lookup(fd)?;
    let socket_id = ob.native_id as u32;

    let mut mgr = SOCKET_MANAGER.lock();
    let socket = mgr.get_socket_mut(socket_id).ok_or(Err(...))?;

    // Copiar datos de recv_buf al buffer de usuario
    let available = socket.recv_buf.len().min(user_buf.len());
    user_buf[..available].copy_from_slice(&socket.recv_buf[..available]);
    socket.recv_buf.drain(..available);

    Ok(available as i64)
}
```

**Opción B: ob_wait para bloqueo, luego SocketInfo query**

- `ob_wait(fd)` se señaliza cuando llegan datos al socket
- Después de `ob_wait`, consultar SocketInfo (class 17) que incluye recv_available
- Leer datos mediante lectura páginada (no implementada)

**Decisión:** Usar Opción A (SocketRecv class) por simplicidad. El `ob_wait` se
usará para bloqueo cuando `recv_buf` esté vacío, con el mecanismo
`wake_socket_readers()` existente.

**Flujo completo de net_socket_recv:**

```rust
pub fn net_socket_recv(fd: u8, buf: &mut [u8]) -> Result<usize, NetError> {
    // Consultar si hay datos disponibles
    let mut info_buf = [0u8; 32];  // NetSocketInfo
    let n = sys_ob_query_info(fd, ObInfoClass::SocketInfo, &mut info_buf)?;
    let info: &NetSocketInfo = unsafe { &*(info_buf.as_ptr() as *const NetSocketInfo) };

    if info.recv_available > 0 {
        // Hay datos: leerlos
        let n = sys_ob_query_info(fd, ObInfoClass::SocketRecv, buf)?;
        return Ok(n);
    }

    if info.direction == 4 /* Closed */ {
        return Ok(0);  // EOF
    }

    // No hay datos: bloquear hasta que lleguen
    // Nota: ob_wait requiere que el socket esté en KWait como SyncEvent
    // El kernel señaliza SyncEvent cuando datos llegan al recv_buf
    sys_ob_wait(fd)?;  // bloquea hasta que datos disponibles

    // Reintentar lectura
    let n = sys_ob_query_info(fd, ObInfoClass::SocketRecv, buf)?;
    Ok(n)
}
```

### 2.3 Tabla de exports de net.nxl

```rust
// libnet/src/lib.rs — genera net.nxl, slot 0x1e0c0000

pub const NET_NXL_BASE: u64 = 0x1e0c0000;
pub const NET_ABI_VERSION: u32 = 1;

#[repr(C)]
pub struct NetAbiTable {
    pub version: u32,

    // === Interfaces ===
    pub net_interface_count: fn() -> i64,
    pub net_get_interface_info: fn(nic_id: u32, buf: *mut u8, buf_len: usize) -> i64,
    pub net_get_stats: fn(buf: *mut u8) -> i64,

    // === Sockets ===
    pub net_socket_create: fn(path: *const u8, socket_type: u32) -> i64,
    pub net_socket_bind: fn(fd: u8, addr_bytes: *const u8) -> i64,
    pub net_socket_connect: fn(fd: u8, addr_bytes: *const u8) -> i64,
    pub net_socket_listen: fn(fd: u8) -> i64,
    pub net_socket_send: fn(fd: u8, data: *const u8, len: usize) -> i64,
    pub net_socket_recv: fn(fd: u8, buf: *mut u8, buf_len: usize) -> i64,
    pub net_socket_close: fn(fd: u8) -> i64,

    // === Configuración ===
    pub net_set_ip: fn(nic_id: u32, ip_bytes: *const u8) -> i64,
    pub net_set_gateway: fn(nic_id: u32, gw_bytes: *const u8) -> i64,

    // === Estado ===
    pub net_get_tcp_status: fn(fd: u8, out: *mut u8) -> i64,
    pub net_get_socket_addr: fn(fd: u8, out: *mut u8) -> i64,
}
```

### 2.4 net.nxl acceso a libneodos

net.nxl necesita syscall wrappers. La solución es que `libneodos.h` exponga
la dirección base del NXL de libneodos como parte del `AbiTable`, o net.nxl
use directamente inline asm para syscalls (como hace `console.nxl`).

**Decisión:** net.nxl usará inline asm directamente para las 4 syscalls Ob
que necesita (ob_create, ob_set_info, ob_query_info, ob_wait, ob_close). Esto
evita dependencias entre NXLs y mantiene net.nxl autocontenido.

```rust
// Dentro de net.nxl (libnet/):
fn ob_set_info(fd: u8, class: u32, buf: *const u8, len: usize) -> i64 {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx", "push rcx", "push rdx", "push r8",
            "mov rbx, {fd}", "mov rcx, {class}", "mov rdx, {buf}", "mov r8, {len}",
            "mov rax, 63", "int 0x80",
            "pop r8", "pop rdx", "pop rcx", "pop rbx",
            fd = in(reg) fd as u64,
            class = in(reg) class as u64,
            buf = in(reg) buf as u64,
            len = in(reg) len as u64,
            out("rax") r,
            options(nostack),
        );
    }
    r
}
```

### 2.5 Integración de net.nxl con el shell

NeoShell debe poder cargar net.nxl cuando se ejecuta un comando de red.
Propuesta: el shell detecta que el comando no es un built-in ni un .nxe
en PATH, pero si es un comando de red conocido, carga net.nxl y ejecuta.

**Simplificación:** Cada NXE de red (`ipconfig.nxe`, `ping.nxe`) es un binario
independiente que carga net.nxl via `loadlib()` al arrancar. No hay integración
en el shell — el shell solo dispatching a PATH como ahora.

---

## 3. Herramientas NXE de Red

### 3.1 ipconfig.nxe

```
IPCONFIG [/ALL]

  /ALL   Muestra información detallada de todas las interfaces
```

**Código (esquema):**

```rust
fn main() {
    let _ = loadlib("C:\\System\\Libraries\\net.nxl");
    let net = *(0x1e0c0000 as *const NetAbiTable);

    let count = (net.net_interface_count)();
    if count <= 0 { print("No network interfaces found"); return; }

    for i in 0..count {
        let mut info = [0u8; 100];
        (net.net_get_interface_info)(i, info.as_mut_ptr(), info.len());
        let info: &NetInterfaceInfo = unsafe { &*(info.as_ptr() as *const NetInterfaceInfo) };

        print("Adapter {}:", i);
        print("  Driver:     {}", driver_name_str(&info.driver_name));
        print("  MAC:        {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", ...);
        print("  IPv4:       {}.{}.{}.{}", info.ipv4[0], ...);
        print("  Mask:       {}.{}.{}.{}", ...);
        print("  Gateway:    {}.{}.{}.{}", ...);
        print("  Status:     {}", if link_up != 0 { "Up" } else { "Down" });
        print("  RX/TX:      {} / {} packets", info.rx_packets, info.tx_packets);
    }
}
```

**Ubicación:** `userbin/ipconfig/` → `C:\Programs\ipconfig.nxe`

**Dependencias:**
- libneodos (sys_write, loadlib)
- net.nxl (net_interface_count, net_get_interface_info)

### 3.2 ping.nxe

```
PING <host> [/n count] [/w timeout_ms] [/4]

  <host>       Dirección IPv4 (x.x.x.x)
  /n count     Número de pings (por defecto 4)
  /w timeout   Timeout en ms (por defecto 1000)
  /4           Forzar IPv4
```

**Implementación:**

ping.nxe usa un socket raw (type 3) para enviar ICMP echo request.

```rust
fn ping(ip: [u8; 4], count: u32, timeout_ms: u32) {
    let net = load_net_nxl();

    // 1. Crear socket raw
    let fd = (net.net_socket_create)(b"\\Ping\0" as *const u8, 3 /* RAW */);

    // 2. Construir ICMP echo request
    // ICMP header: type(8) code(0) checksum(2) id(2) seq(2) payload
    let mut pkt = [0u8; 64];
    pkt[0] = 8;  // Echo Request
    pkt[1] = 0;  // Code
    // checksum en pkt[2..4] se calcula después
    let id = 0x1234u16;
    pkt[4..6].copy_from_slice(&id.to_be_bytes());
    pkt[6..8].copy_from_slice(&seq.to_be_bytes());
    // payload opcional (timestamp)
    pkt[8..16].copy_from_slice(b"NeoDOS!");
    // Calcular checksum ICMP
    let checksum = icmp_checksum(&pkt[..64]);
    pkt[2..4].copy_from_slice(&checksum.to_be_bytes());

    // 3. Conectar raw socket al destino
    // Para raw, "conectar" significa fijar la IP destino en el socket
    let addr = NetSocketAddr { ip, port: 0, _pad: [0;2] };
    (net.net_socket_connect)(fd, &addr as *const _ as *const u8)?;

    // 4. Enviar ping
    let t1 = get_ticks();
    (net.net_socket_send)(fd, &pkt)?;

    // 5. Recibir respuesta
    let mut reply = [0u8; 256];
    match (net.net_socket_recv)(fd, &mut reply) {
        Ok(n) if n >= 20 => {
            // Parsear respuesta IP+ICMP
            let t2 = get_ticks();
            let rtt = t2 - t1;
            print("Reply from {}.{}.{}.{}: bytes={} time={}ms", ip[0], ..., n, rtt);
        }
        Ok(0) => print("Request timed out."),
        Err(e) => print("Ping failed: {}", e),
    }

    // 6. Cerrar
    (net.net_socket_close)(fd);
}
```

**Nota sobre raw sockets:** El kernel debe aceptar que un socket raw (SocketType::Raw)
pueda enviar datos que se encapsulan directamente en Ethernet (con type 0x0800 para
IPv4). El kernel construye el header Ethernet + IP si el socket tiene tipo Raw y
se ha "conectado" a una IP destino. Es decir, para Raw:
- `SocketSend` → construir Ethernet + IPv4 con proto=1 (ICMP) + payload
- No construir header TCP/UDP
- El checksum IP lo calcula el kernel

**Si raw socket es demasiado complejo (primera iteración):**

**Alternativa simplificada:** Añadir `ObSetInfoClass::Ping(23)` que toma
una IP destino y un payload, y el kernel construye ICMP echo request completa,
lo envía, espera reply y devuelve el RTT. Esto sería un atajo para tener ping
rápido sin implementar raw sockets. **Pero rompe la arquitectura limpia.**
No se recomienda.

**Decisión:** Implementar raw sockets correctamente. El kernel:
1. `socket_send` con tipo Raw → construir Ethernet + IPv4 con proto del payload ICMP
2. `net_handle_incoming_packet` con proto ICMP → mirar si hay socket Raw escuchando
   en esa IP y entregar payload

### 3.3 dhcp.nxe

```
DHCP [/RENEW] [/RELEASE]

  Sin args: Obtiene configuración DHCP y la aplica
  /RENEW: Renueva concesión
  /RELEASE: Libera concesión
```

**Formato DHCP (RFC 2131):**

Un paquete DHCP se transmite sobre UDP (puerto 67 server, 68 cliente).
Formato BOOTP extendido (236 bytes fijos + opciones variables):

```
Byte 0:     op (1=request, 2=reply)
Byte 1:     htype (1=ethernet)
Byte 2:     hlen (6)
Byte 3:     hops (0)
Byte 4-7:   xid (transaction id, random)
Byte 8-9:   secs (0)
Byte 10-11: flags (0x8000 = broadcast)
Byte 12-15: ciaddr (client IP, 0.0.0.0 en Discover)
Byte 16-19: yiaddr (your IP, filled by server)
Byte 20-23: siaddr (server IP)
Byte 24-27: giaddr (gateway IP)
Byte 28-43: chaddr (client MAC, 16 bytes)
Byte 44-235: padding (ceros)
Byte 236+:  magic cookie (0x63825363) + opciones DHCP
```

**Código esquemático:**

```rust
fn dhcp_discover(net: &NetAbiTable) -> Result<DhcpOffer, NetError> {
    let fd = (net.net_socket_create)(b"\\DhcpClient\0", 2 /* UDP */)?;

    // Bind al puerto 68 (cliente DHCP)
    let local = NetSocketAddr { ip: [0,0,0,0], port: 68u16.to_be(), _pad: [0;2] };
    (net.net_socket_bind)(fd, &local as *const _ as *const u8)?;

    // Conectar al broadcast (255.255.255.255:67)
    let broadcast = NetSocketAddr { ip: [255,255,255,255], port: 67u16.to_be(), _pad: [0;2] };
    (net.net_socket_connect)(fd, &broadcast as *const _ as *const u8)?;

    // Construir DHCP Discover
    let mut pkt = [0u8; 300];
    pkt[0] = 1;  // op = BOOTREQUEST
    pkt[1] = 1;  // htype = ethernet
    pkt[2] = 6;  // hlen = 6
    // xid = random
    let xid = rdrand();
    pkt[4..8].copy_from_slice(&xid.to_be_bytes());
    pkt[10] = 0x80; pkt[11] = 0x00;  // flags = broadcast
    // chaddr = nuestra MAC
    let mac = get_our_mac(net);
    pkt[28..34].copy_from_slice(&mac);
    // magic cookie + DHCP option 53 = Discover (1)
    pkt[236..240].copy_from_slice(&[0x63, 0x82, 0x53, 0x63]);  // magic
    pkt[240] = 53; pkt[241] = 1; pkt[242] = 1;  // DHCP Discover
    pkt[243] = 255;  // end option
    let pkt_len = 244;

    // Enviar Discover
    (net.net_socket_send)(fd, &pkt[..pkt_len])?;

    // Esperar DHCP Offer
    let mut reply = [0u8; 300];
    let n = (net.net_socket_recv)(fd, &mut reply)?;

    // Parsear Offer: yiaddr en bytes 16-19, server IP en bytes 20-23
    // Opciones DHCP: buscar option 53 = Offer (2), option 1 = subnet mask, option 3 = gateway, option 6 = DNS

    // Enviar DHCP Request...
    // Esperar DHCP ACK...

    // Devolver configuración obtenida
}

fn dhcp_apply(net: &NetAbiTable, config: &DhcpConfig) {
    // Guardar en Registry
    // cm_open_key + cm_set_value para IP, Gateway, DNS, SubnetMask

    // Aplicar IP
    (net.net_set_ip)(0, &config.ip)?;
    (net.net_set_gateway)(0, &config.gateway)?;
}
```

**Limitación:** DHCP requiere que el kernel tenga:
1. UDP socket funcional (bind puerto 68, enviar a 255.255.255.255:67)
2. Broadcast ethernet (dest FF:FF:FF:FF:FF:FF)
3. El kernel construya header UDP+IP+Ethernet desde `socket_send`
4. Receive path: UDP paquetes entrantes dispatch al socket bind al puerto 68

### 3.4 dnsresv.nxe (futuro)

```
DNSRESV <hostname> [/s dns_server]

  Resuelve hostname a IPv4 via DNS server configurado o especificado.
```

Usa `net_dns_resolve()` de net.nxl, que implementa consulta DNS sobre UDP
(puerto 53). Formato DNS: header de 12 bytes + query section.

```rust
pub fn net_dns_resolve(hostname: &str, result_ip: &mut [u8; 4]) -> Result<(), NetError> {
    // 1. Obtener DNS server de Registry o argumento
    // 2. Construir consulta DNS:
    //    Header: ID(2), flags(0x0100), QDCOUNT=1, resto=0
    //    Question: nombre encoded + type(1=AAAA→IPv4) + class(1=IN)
    // 3. net_socket_create(UDP) → bind to any → connect to DNS:53
    // 4. net_socket_send(query)
    // 5. net_socket_recv(response)
    // 6. Parsear respuesta: extract IP de answer section
    // 7. (futuro) cachear resultado
}
```

---

## 4. NeoInit System Configuration

### 4.1 Estado actual

NeoInit (`userbin/neoinit/src/main.rs`, 79 líneas) es un supervisor minimalista:

```rust
fn spawn() -> Result<u32, i64> {
    let path_str = "\\Global\\FileSystem\\C:\\Programs\\NeoShell.nxe";
    let attrs = 0xFFu64 | (0xFFu64 << 8) | (0xFFu64 << 16);
    let fd = syscall::sys_ob_create(path_str, 1, None, attrs)?;
    let _ = syscall::sys_ob_wait(fd);
    Ok(0)
}
```

**Problemas:**
- Path `C:\Programs\NeoShell.nxe` hardcoded
- No usa Registry
- No inicia servicios (net, logger, etc.)
- No se puede configurar sin recompilar

### 4.2 Migración a Registry

NeoInit debe leer su configuración de `\Registry\Machine\System\CurrentControlSet\Services\NeoInit`.

**Jerarquía Registry:**

```
\Registry\Machine\System\CurrentControlSet\Services\NeoInit
├── DefaultShell        REG_SZ   "C:\Programs\NeoShell.nxe"
├── AutoStartServices   REG_MULTI_SZ   "netcfg"
├── EnableVT            REG_DWORD  1
├── VTCount             REG_DWORD  4
└── ShellArgs           REG_SZ   ""
```

**Claves de soporte para servicios:**

```
\Registry\Machine\System\CurrentControlSet\Services
├── netcfg
│   ├── Path            REG_SZ   "C:\Programs\netcfg.nxe"
│   ├── AutoStart       REG_DWORD  1
│   └── Description     REG_SZ   "Network configuration service"
│
├── logger
│   ├── Path            REG_SZ   "C:\Programs\logger.nxe"
│   ├── AutoStart       REG_DWORD  1
│   └── ...
│
└── <future_services>
```

### 4.3 Nueva implementación de NeoInit

```rust
use libneodos::syscall;

const REG_ROOT: &str = "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\NeoInit";

fn cm_open_key(path: &str) -> Result<u8, i64> {
    syscall::sys_cm_open_key(path)
}

fn cm_query_str(fd: u8, name: &str, default: &str) -> [u8; 260] {
    let mut buf = [0u8; 260];
    if let Ok(n) = syscall::sys_cm_query_value(fd, name, &mut buf) {
        let mut result = [0u8; 260];
        let len = n.min(259);
        result[..len].copy_from_slice(&buf[..len]);
        result
    } else {
        let bytes = default.as_bytes();
        let len = bytes.len().min(259);
        result[..len].copy_from_slice(&bytes[..len]);
        result
    }
}

fn cm_query_dword(fd: u8, name: &str, default: u32) -> u32 {
    let mut buf = [0u8; 4];
    if let Ok(_) = syscall::sys_cm_query_value(fd, name, &mut buf) {
        u32::from_le_bytes(buf)
    } else {
        default
    }
}

/// Hacer spawn de un proceso y no esperarlo (detach).
fn spawn_detached(path: &str) -> Result<u32, i64> {
    let ob_path = alloc::format!("\\Global\\FileSystem\\{}", path);
    let attrs = 0xFFu64 | (0xFFu64 << 8) | (0xFFu64 << 16);
    let fd = syscall::sys_ob_create(&ob_path, 1, None, attrs)?;
    // No hacer ob_wait — proceso en background
    syscall::sys_close(fd)?;
    Ok(0)  // no devolvemos PID real sin ob_wait
}

/// Esperar a que la red esté lista.
fn wait_for_network(timeout_ms: u32) -> bool {
    // Cargar net.nxl
    let _ = syscall::sys_loadlib("C:\\System\\Libraries\\net.nxl");
    let net = unsafe { &*(0x1e0c0000 as *const NetAbiTable) };

    let start = get_ticks_ms();
    loop {
        let count = (net.net_interface_count)();
        if count > 0 {
            // Hay NIC — comprobar si tiene IP válida
            let mut info = [0u8; 100];
            (net.net_get_interface_info)(0, info.as_mut_ptr(), info.len());
            let info: &NetInterfaceInfo = unsafe { &*(info.as_ptr() as *const NetInterfaceInfo) };
            // IP 0.0.0.0 significa no configurado → esperar DHCP
            if info.ipv4 != [0,0,0,0] {
                return true;
            }
            // Si IP sigue siendo 0.0.0.0, esperar
        }
        if get_ticks_ms() - start > timeout_ms {
            return false;  // timeout
        }
        syscall::sys_sleep_ex();  // yield
    }
}

fn get_ticks_ms() -> u64 {
    // Leer timer tick count via ob_open + ob_query_info
    // Simplicidad: usar aproximación
    0
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Banner
    write_str(b"NeoInit v0.2.0 (PID 1)\r\n");

    // 2. Abrir Registry
    let reg_fd = match cm_open_key(REG_ROOT) {
        Ok(fd) => fd,
        Err(_) => {
            write_str(b"[neoinit] WARNING: Registry not available, using defaults\r\n");
            0xFF  // fd inválido
        }
    };

    // 3. Leer DefaultShell
    let shell_path = if reg_fd != 0xFF {
        cm_query_str(reg_fd, "DefaultShell", "C:\\Programs\\NeoShell.nxe")
    } else {
        *b"C:\\Programs\\NeoShell.nxe\0"
    };

    // 4. Leer auto-start services
    let services_str = if reg_fd != 0xFF {
        cm_query_str(reg_fd, "AutoStartServices", "")
    } else {
        [0u8; 260]
    };

    // 5. Leer EnableVT
    let enable_vt = if reg_fd != 0xFF {
        cm_query_dword(reg_fd, "EnableVT", 1)
    } else {
        1
    };

    // 6. Iniciar servicios de auto-arranque
    if services_str[0] != 0 {
        write_str(b"[neoinit] Starting services...\r\n");
        let svc_list = core::str::from_utf8(&services_str).unwrap_or("");
        for svc_name in svc_list.split(';') {
            let svc_name = svc_name.trim();
            if svc_name.is_empty() { continue; }

            // Leer Path del servicio desde Registry
            let svc_reg_path = alloc::format!(
                "\\Registry\\Machine\\System\\CurrentControlSet\\Services\\{}", svc_name
            );
            if let Ok(svc_fd) = cm_open_key(&svc_reg_path) {
                let svc_path = cm_query_str(svc_fd, "Path", "");
                if svc_path[0] != 0 {
                    let path_str = core::str::from_utf8(&svc_path).unwrap_or("").trim_end_matches('\0');
                    write_str(b"[neoinit] Starting service: ");
                    write_str(path_str.as_bytes());
                    write_str(b"\r\n");

                    match spawn_detached(path_str) {
                        Ok(_) => write_str(b"  -> started\r\n"),
                        Err(e) => {
                            write_str(b"  -> FAILED: ");
                            // print errno
                            write_str(b"\r\n");
                        }
                    }
                }
                let _ = syscall::sys_close(svc_fd);
            }
        }
    }

    // 7. Cargar net.nxl si hay servicios de red
    // (netcfg lo hará por su cuenta, pero podemos precargar)
    if services_str.iter().any(|&b| b == b'n' || b == b'N') {
        let _ = syscall::sys_loadlib("C:\\System\\Libraries\\net.nxl");
    }

    // 8. Esperar red si configurado
    let wait_net = if reg_fd != 0xFF {
        cm_query_dword(reg_fd, "WaitForNetwork", 0)
    } else {
        0
    };
    if wait_net != 0 {
        write_str(b"[neoinit] Waiting for network...\r\n");
        let timeout = cm_query_dword(reg_fd, "BootTimeout", 30) * 1000;
        if wait_for_network(timeout) {
            write_str(b"[neoinit] Network ready.\r\n");
        } else {
            write_str(b"[neoinit] WARNING: Network timeout, continuing.\r\n");
        }
    }

    // 9. Spawn shell (loop supervisor)
    let shell_path_cstr = core::str::from_utf8(&shell_path).unwrap_or("C:\\Programs\\NeoShell.nxe");
    write_str(b"[neoinit] DefaultShell: ");
    write_str(shell_path_cstr.as_bytes());
    write_str(b"\r\n");

    if enable_vt != 0 {
        set_vt(0);
    }

    loop {
        match spawn(shell_path_cstr) {
            Ok(pid) => {
                write_str(b"[neoinit] shell exited, respawning...\r\n");
            }
            Err(e) => {
                write_str(b"[neoinit] spawn failed, retrying...\r\n");
                // Esperar antes de reintentar
                for _ in 0..100_000_000 { core::hint::spin_loop(); }
            }
        }
    }
}
```

### 4.4 Servicio netcfg.nxe

`netcfg.nxe` es un servicio auto-iniciado que configura la red al boot.

```
netcfg.nxe
  │
  ├── Cargar net.nxl
  ├── Leer configuración de Registry:
  │   \Registry\Machine\System\Network\Interfaces\0
  │     ├── DHCPEnabled     REG_DWORD  1
  │     ├── IP              REG_SZ     "0.0.0.0"
  │     ├── Gateway         REG_SZ     "0.0.0.0"
  │     └── DNS             REG_SZ     "0.0.0.0"
  │
  ├── Si DHCPEnabled == 1:
  │   ├── Ejecutar dhcp.nxe (obtener IP dinámica)
  │   └── Guardar resultado en Registry
  │
  ├── Si DHCPEnabled == 0:
  │   ├── net_set_ip(IP, SubnetMask)
  │   └── net_set_gateway(Gateway)
  │
  └── Marcar red como disponible:
      (crear Event en Ob, o escribir flag en Registry)
```

netcfg termina después de configurar. NeoInit no espera a netcfg a menos que
`WaitForNetwork=1`.

### 4.5 Integración con la shell: servicios en background

Los servicios iniciados por NeoInit son procesos independientes que corren en
background. No hay interacción directa con la shell. Cuando el shell está activo,
los servicios simplemente existen como procesos.

Para comunicación shell↔servicio:
- Pipe nombrado vía Ob (future)
- Registry (lectura de estado)
- EventBus (eventos de sistema)

En la primera versión, los servicios son fire-and-forget.

---

## 5. Registry Usage Model

### 5.1 Separación Registry vs Filesystem

| Almacén | Registry | NeoFS |
|---------|----------|-------|
| Config estructural del sistema | ✅ DefaultShell, servicios, interfaces de red, paquetes instalados | ❌ |
| Hardware info | ✅ DeviceMap, driver bindings | ❌ |
| Perfiles de usuario | ✅ SID, grupos, paths de perfil (referencias) | ✅ Datos reales |
| Preferencias de usuario | ❌ | ✅ `C:\Users\<name>\*.cfg` |
| Config de aplicaciones | ❌ (solo claves small) | ✅ `C:\Programs\<app>\*.cfg` |
| Documentos | ❌ | ✅ `C:\Data\`, `C:\Users\<name>\` |
| Logs | ❌ | ✅ `C:\Logs\` |
| Cache temporal | ❌ | ✅ `C:\Temp\` |
| Binarios | ❌ | ✅ NXE, NEM, NXL |
| Estado de paquetes | ✅ Versiones, dependencias, archivos instalados | ✅ Archivos reales (.npkg) |
| Config de red | ✅ IP, gateway, DNS, DHCP flag | ❌ |

### 5.2 Regla de oro

> **Registry = qué es el sistema (estructura).**
> **NeoFS = con qué trabaja el sistema (contenido).**

Registry para valores pequeños (<4 KB), estructura jerárquica, y referencias.
NeoFS para datos, logs, binarios, configuraciones editables.

### 5.3 Jerarquía completa propuesta

```
\Registry
├── Machine
│   ├── Hardware
│   │   ├── Description
│   │   │   └── System           REG_SZ  "NeoDOS x86_64 QEMU"
│   │   ├── DeviceMap
│   │   │   ├── Keyboard         REG_SZ  "\\Device\\Keyboard0"
│   │   │   ├── Serial           REG_SZ  "\\Device\\Serial0"
│   │   │   └── Framebuffer      REG_SZ  "\\Device\\Video0"
│   │   └── ResourceMap
│   │       └── PnPManager       REG_SZ  (lista de dispositivos detectados)
│   │
│   ├── System
│   │   ├── CurrentControlSet
│   │   │   ├── Services
│   │   │   │   ├── NeoInit      (ver sección 4.2)
│   │   │   │   ├── netcfg       (Path, AutoStart, Description)
│   │   │   │   └── ...
│   │   │   │
│   │   │   ├── Control
│   │   │   │   ├── WaitForNetwork    REG_DWORD  0/1
│   │   │   │   ├── BootTimeout       REG_DWORD  30
│   │   │   │   └── CrashBehavior     REG_DWORD  0=panic, 1=reboot
│   │   │   │
│   │   │   └── Environment
│   │   │       ├── Path              REG_SZ  "C:\\Programs;C:\\System\\Bin"
│   │   │       ├── Temp              REG_SZ  "C:\\Temp"
│   │   │       └── Home              REG_SZ  "C:\\Users\\Default"
│   │   │
│   │   └── Network
│   │       └── Interfaces
│   │           └── 0
│   │               ├── IP            REG_SZ  "10.0.2.15"
│   │               ├── SubnetMask    REG_SZ  "255.255.255.0"
│   │               ├── Gateway       REG_SZ  "10.0.2.1"
│   │               ├── DNS1          REG_SZ  "10.0.2.3"
│   │               ├── DNS2          REG_SZ  ""
│   │               ├── DHCPEnabled   REG_DWORD  1
│   │               ├── MACAddress    REG_SZ  "52:54:00:12:34:56"
│   │               └── DriverBinding REG_SZ  "\\Device\\Nic\\0"
│   │
│   └── Packages                    (ver sección 6)
│       └── ...
│
└── User
    └── S-1-5-21-0-0-0-1000        (SID del usuario por defecto)
        ├── ProfilePath            REG_SZ  "C:\\Users\\Default"
        └── Groups                 REG_MULTI_SZ  "Users;Network"
```

### 5.4 Persistencia a disco — cm_flush_key

`cm_flush_key` actualmente es no-op. La persistencia debe implementarse así:

```rust
pub fn cm_flush_key(key_native_id: u64) -> Result<(), ()> {
    let (hive_idx, _cell_idx) = decode_cell(key_native_id);
    let cm = CM_MANAGER.lock();
    if (hive_idx as usize) >= cm.hives.len() {
        return Err(());
    }
    let hm = &cm.hives[hive_idx as usize];

    // Serializar hive a bytes
    let data = hm.hive.serialize();

    // Escribir a archivo en NeoFS
    // Path: C:\System\Registry\<hive_name>.hiv
    let file_path = alloc::format!("C:\\System\\Registry\\{}.hiv", hm.name);
    globals::with_vfs(|vfs| {
        let (drive_idx, node) = vfs.resolve_path(&file_path)?;
        vfs.write(drive_idx, node.inode, 0, &data)
    }).ok();

    Ok(())
}
```

**Formato de serialización del hive:**

```
┌──────────────────────────────────┐
│ Magic: "NEOH" (4 bytes)          │
│ Version: u32 (1)                 │
│ Cell count: u32                  │
│ Cell data: variable              │
│   Por cada celda:                │
│   ├── Cell type: u32 (0=free,    │
│   │   1=key, 2=value, 3=security)│
│   ├── Cell payload (variable)    │
│   └── Padding to align           │
└──────────────────────────────────┘
```

**¿Cuándo se persiste?** No en cada set_value. Opciones:

1. **Síncrono:** Cada `cm_set_value` → marcar hive dirty.
   `cm_flush_key` se llama desde el shell (`FLUSHREG` command), o desde
   NeoInit antes de spawn shell, o en shutdown.
2. **Periódico:** Un demonio de kernel (work queue) cada N segundos si dirty.
3. **A petición:** Solo cuando se llama `cm_flush_key` (syscall RAX=74).

**Decisión:** Opción 1 + 3. `cm_set_value` marca el hive como dirty.
Antes de spawn NeoShell, NeoInit llama `cm_flush_key` para guardar.
El kernel llama flush automático en shutdown (sys_poweroff).
El usuario puede llamar `FLUSHREG` manualmente.

### 5.5 Creación de valores por defecto

En `main.rs`, después de Phase 3.881 (init_cm), crear valores por defecto:

```rust
#[link_section = ".text.boot"]
pub fn create_default_registry_values() {
    use crate::cm::{encode_cell, cm_create_key, cm_set_value};
    use crate::cm::hive::{REG_SZ, REG_DWORD, REG_MULTI_SZ};

    // Abrir raíz del SYSTEM hive
    let root_native = encode_cell(0, 0);  // hive 0, cell 0

    // CurrentControlSet
    let ccs = match cm_create_key(root_native, "CurrentControlSet") {
        Ok(id) => id,
        Err(_) => return,  // ya existe
    };

    // Services\NeoInit
    let services = cm_create_key(ccs, "Services").unwrap_or(ccs);
    let neoinit = cm_create_key(services, "NeoInit").unwrap_or(services);
    cm_set_value(neoinit, "DefaultShell", REG_SZ, b"C:\\Programs\\NeoShell.nxe").ok();
    cm_set_value(neoinit, "AutoStartServices", REG_MULTI_SZ, b"netcfg").ok();
    cm_set_value(neoinit, "EnableVT", REG_DWORD, &1u32.to_le_bytes()).ok();
    cm_set_value(neoinit, "VTCount", REG_DWORD, &4u32.to_le_bytes()).ok();

    // Services\netcfg
    let netcfg = cm_create_key(services, "netcfg").unwrap_or(services);
    cm_set_value(netcfg, "Path", REG_SZ, b"C:\\Programs\\netcfg.nxe").ok();
    cm_set_value(netcfg, "AutoStart", REG_DWORD, &1u32.to_le_bytes()).ok();
    cm_set_value(netcfg, "Description", REG_SZ, b"Network configuration service").ok();

    // Control
    let control = cm_create_key(ccs, "Control").unwrap_or(ccs);
    cm_set_value(control, "WaitForNetwork", REG_DWORD, &0u32.to_le_bytes()).ok();
    cm_set_value(control, "BootTimeout", REG_DWORD, &30u32.to_le_bytes()).ok();

    // Environment
    let env = cm_create_key(ccs, "Environment").unwrap_or(ccs);
    cm_set_value(env, "Path", REG_SZ, b"C:\\Programs;C:\\System\\Bin").ok();
    cm_set_value(env, "Temp", REG_SZ, b"C:\\Temp").ok();

    // Network\Interfaces\0
    let network = cm_create_key(root_native, "Network").unwrap_or(root_native);
    let ifaces = cm_create_key(network, "Interfaces").unwrap_or(network);
    let if0 = cm_create_key(ifaces, "0").unwrap_or(ifaces);
    cm_set_value(if0, "IP", REG_SZ, b"0.0.0.0").ok();
    cm_set_value(if0, "SubnetMask", REG_SZ, b"0.0.0.0").ok();
    cm_set_value(if0, "Gateway", REG_SZ, b"0.0.0.0").ok();
    cm_set_value(if0, "DNS1", REG_SZ, b"0.0.0.0").ok();
    cm_set_value(if0, "DHCPEnabled", REG_DWORD, &1u32.to_le_bytes()).ok();
}
```

**Protección:** Solo se crean si no existen (el `cm_create_key` ya retorna error
si existe). Se llaman una vez en el primer boot.

---

## 6. NeoPkg — Package System

### 6.1 Visión general

NeoPkg es el sistema de gestión de paquetes de NeoDOS. Opera a nivel userland
(`pkg.nxe`), con validación del kernel para componentes CORE.

### 6.2 Formato .npkg (binario)

```
┌──────────────────────────────────────────────────────────┐
│ Magic: 4 bytes "NPKG"                                    │
│ Header version: u16 = 1                                  │
│ Manifest offset: u32                                     │
│ Manifest size: u32                                       │
│ File entry count: u32                                    │
│ Data offset: u32 (start of file data section)            │
│ Padding to 64 bytes                                      │
├──────────────────────────────────────────────────────────┤
│ Manifest (JSON-like, UTF-8, null-terminated)             │
│   name=netutils                                          │
│   version=1.0.0                                          │
│   type=User                                              │
│   arch=x86_64                                            │
│   description=Networking utilities                       │
│   depends=libneodos>=0.46;net.nxl>=0.1                   │
│   installed_size=131072                                  │
├──────────────────────────────────────────────────────────┤
│ File entries (20 bytes each):                            │
│   [0] dest_path_offset=4, dest_path_len=20,              │
│       file_size=8192, file_data_offset=1024               │
│   [1] dest_path_offset=24, dest_path_len=16,             │
│       file_size=6144, file_data_offset=9216               │
│   ...                                                     │
├──────────────────────────────────────────────────────────┤
│ File data (raw, concatenated)                             │
│   [data for entry 0]                                     │
│   [data for entry 1]                                     │
│   ...                                                     │
└──────────────────────────────────────────────────────────┘
```

**Manifest detallado:**

```
name=netutils
version=1.0.0
type=User           # Core | System | User
arch=x86_64
description=Networking utilities (ping, ipconfig, dhcp)
depends=libneodos>=0.46;net.nxl>=0.1
conflicts=          # nombre de paquete con el que conflictúa
installed_size=131072
post_install=       # comando opcional post-instalación (ej: "REGSET ...")
maintainer=NeoDOS Team
homepage=https://neodos.io/packages/netutils
checksum_alg=SHA256  # algoritmo usado en file entries
```

**File entry (20 bytes cada uno):**

```rust
#[repr(C)]
struct NpkgFileEntry {
    dest_relative_path_offset: u32,  // offset en string table
    dest_relative_path_len: u16,
    file_size: u32,
    file_data_offset: u32,           // desde start of data section
    checksum: [u8; 4],              // CRC32 de los datos
}
```

### 6.3 Herramientas pkg.nxe

```
PKG INSTALL <file.npkg> [/F] [/S]

  Instala un paquete. /F = force (ignora dependencias),
  /S = silent (no preguntar confirmación)

PKG REMOVE <name> [/F]

  Elimina un paquete. /F = force (elimina incluso System).

PKG LIST [/ALL]

  Lista paquetes instalados. Sin /ALL solo muestra User.
  Con /ALL muestra todos (Core, System, User).

PKG INFO <name>

  Información detallada de un paquete instalado.

PKG VERIFY <name>

  Verifica checksums de archivos instalados.
```

**PKG INSTALL — algoritmo:**

```
1. Parsear .npkg (validar magic, manifest, estructura)
2. Verificar dependencias contra Registry\Machine\Packages
3. Si falta dependencia → error (o /F para ignorar)
4. Si conflictos → error
5. Para cada file entry:
   a. Construir path destino: C:\Programs\<relative_path>
   b. Verificar que no existe (o preguntar sobrescribir)
   c. Crear directorios si necesario (ob_create Directory)
   d. Escribir archivo (ob_create File + ob_set_info WriteContent)
   e. Verificar checksum
6. Si post_install no vacío: ejecutar comando
7. Registrar en Registry:
   \Registry\Machine\Packages\<name>
     Version = "1.0.0"
     Type = "User"
     InstalledFiles = REg_multi_sz "..."
     Dependencies = "..."
     InstalledAt = "2026-07-01 12:00:00"
     Status = "Active"
```

**PKG REMOVE — algoritmo:**

```
1. Abrir \Registry\Machine\Packages\<name>
2. Leer "Type" — si Core → ERROR (protegido)
3. Leer "Type" — si System y no /F → preguntar confirmación
4. Leer InstalledFiles → lista de archivos
5. Para cada archivo:
   a. ob_destroy(file_fd) o ob_set_info(FileDelete)
   b. Verificar que se eliminó
6. Eliminar clave Registry:
   \Registry\Machine\Packages\<name>
7. Si hay post_remove en el paquete original: ejecutar
```

### 6.4 Protección CORE

**Mecanismo 1 — Registry flag:**

El kernel, al crear la clave `\Registry\Machine\Packages\*`, valida que el tipo
Core no se pueda eliminar via `cm_delete_key`. Si un proceso userland intenta
borrar un paquete Core:

```rust
// En syscall cm_delete_key handler (RAX=73):
let (hive_idx, cell_idx) = decode_cell(key_native_id);
let cm = CM_MANAGER.lock();
let hm = &cm.hives[hive_idx as usize];

// Verificar si esta clave es un paquete Core
if let Some(val) = hm.hive.query_value(cell_idx, "Type") {
    if val.as_str() == Ok("Core") {
        return Err(());  // denegado
    }
}
```

**Mecanismo 2 — MODE_CORE en NeoFS:**

Los archivos instalados como Core tienen el flag `MODE_CORE` en el inodo.
El kernel deniega `ob_set_info(FileDelete)` y `ob_destroy` sobre archivos con
MODE_CORE. Esto funciona aunque el Registry esté corrupto.

```rust
// En handler_ob_destroy:
let node = vfs.resolve(...)?;
if node.mode & MODE_CORE != 0 {
    return Err(SyscallError::AccessDenied);
}
```

**Mecanismo 3 — Lista hardcoded en kernel:**

El kernel mantiene una lista de componentes Core que no se pueden eliminar
bajo ninguna circunstancia:

```rust
const CORE_COMPONENTS: &[&str] = &[
    "NeoInit", "NeoShell", "NeoFS",
    "libneodos", "kernel.elf",
];
```

### 6.5 Seguridad

- Solo procesos con token admin pueden ejecutar `pkg.nxe`
- `pkg.nxe` verifica `is_current_admin()` al arrancar
- Paquetes System requieren flag `/F` para REMOVE
- Paquetes Core no se pueden REMOVE ni con `/F`
- Verificación de checksum post-instalación
- (Futuro) Firmas digitales con ed25519

### 6.6 Ubicaciones en NeoFS

```
C:\Packages\                ← archivos .npkg almacenados
C:\Programs\                ← NXEs instalados
C:\System\Libraries\       ← NXLs instalados
C:\System\Drivers\         ← NEMs instalados
C:\System\Registry\        ← hives serializados (.hiv)
C:\System\Config\          ← config post-instalación
C:\Logs\pkg.log            ← log de operaciones pkg
```

---

## 7. Necesidades del Kernel

### 7.1 Estado actual del kernel (TCP/IP)

| Componente | Archivo | Estado | Detalle |
|-----------|---------|--------|---------|
| Ethernet | `src/net/ethernet.rs` | ✅ | Header 14B, type ETH_TYPE_ARP/IPV4, FCS computation |
| ARP | `src/net/arp.rs` | ✅ | Cache 64 entradas, 300s timeout, request/reply, static entries |
| IPv4 | `src/net/ipv4.rs` | ✅ | Header 20B, checksum, build_ipv4_header(), sin fragmentación |
| ICMP | `src/net/icmp.rs` | ✅ | Echo request/reply, build_echo_reply(), checksum |
| UDP | `src/net/udp.rs` | ⚠️ | Header 8B, checksum, **sin dispatch de paquetes** |
| TCP | `src/net/tcp.rs` | ⚠️ | State machine (11 estados), buffers, **sin handshake real** |
| TCP send/recv | `src/net/tcp.rs` | ⚠️ | `tcp_send()` escribe en send_buf local, no transmite |
| e1000 | `src/net/e1000.rs` | ✅ | Probe, MMIO, RX/TX rings, poll_packet, send_packet |
| Socket manager | `src/net/socket.rs` | ✅ | Alloc/bind/connect/listen/send/recv/close/wake |
| Socket→NIC TX | `src/net/socket.rs` | ❌ | `socket_send()` no llama a NIC, solo escribe en send_buf local |
| NIC→Socket RX | `src/net/mod.rs` | ❌ | `net_handle_incoming_packet` no rutea TCP/UDP a sockets |
| ObType::Socket | `src/object/types.rs` | ✅ | type=18, ObInfoClass 17-20, ObSetInfoClass 18-22 |
| ObSocket handler | `src/syscall/ob.rs` | ✅ | handlers para create/set/query de sockets |

### 7.2 Gaps críticos para red userland

#### Gap 1: Transmit path (socket → NIC)

**Estado:** `socket_send()` en `src/net/socket.rs:178` escribe datos en
`send_buf` pero nunca construye headers Ethernet/IP/UDP/TCP ni llama a
`nic.send_packet()`.

**Código actual:**

```rust
pub fn socket_send(id: u32, data: &[u8]) -> Result<usize, ()> {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = mgr.get_socket_mut(id).ok_or(())?;
    if socket.direction != SocketDirection::Connected { return Err(()); }
    if socket.socket_type == SocketType::Tcp {
        if let Some(tcp_id) = socket.tcp_conn_id {
            return crate::net::tcp::tcp_send(tcp_id, data);
        }
    }
    socket.send_buf.extend_from_slice(data);  // ← solo buffer local
    Ok(data.len())
}
```

**Código necesario:**

```rust
pub fn socket_send(id: u32, data: &[u8]) -> Result<usize, ()> {
    let mut mgr = SOCKET_MANAGER.lock();
    let socket = mgr.get_socket_mut(id).ok_or(())?;
    if socket.direction != SocketDirection::Connected { return Err(()); }

    // 1. Obtener NIC para este socket (la primera NIC disponible)
    let nic_id = socket.nic_id.unwrap_or(0);
    let nic_mac = get_nic_mac(nic_id);
    let nic_ip = get_nic_ip(nic_id);

    // 2. Construir Ethernet header
    let dst_mac = resolve_dest_mac(socket.remote.ip, nic_id);  // ARP lookup
    let eth = EthernetHeader::new(dst_mac, nic_mac, ETH_TYPE_IPV4);

    // 3. Construir paquete según tipo
    let packet: Vec<u8> = match socket.socket_type {
        SocketType::Tcp => {
            let tcp_id = socket.tcp_conn_id.ok_or(())?;
            let tcp_data = build_tcp_segment(tcp_id, data)?;
            // tcp_data ya incluye header TCP
            let ipv4 = build_ipv4_header(nic_ip, socket.remote.ip, IPV4_PROTO_TCP,
                tcp_data.len(), 0);
            build_ethernet_frame(&eth, &ipv4, &tcp_data)
        }
        SocketType::Udp => {
            let udp_data = build_udp_datagram(socket.local.port, socket.remote.port, data);
            let ipv4 = build_ipv4_header(nic_ip, socket.remote.ip, IPV4_PROTO_UDP,
                udp_data.len(), 0);
            build_ethernet_frame(&eth, &ipv4, &udp_data)
        }
        SocketType::Raw => {
            // Raw: el payload ya incluye el header IP + protocolo
            // Pero necesitamos Ethernet header (MAC destino)
            let mut frame = Vec::with_capacity(ETH_HDR_LEN + data.len());
            frame.extend_from_slice(eth.as_bytes());
            frame.extend_from_slice(data);
            frame
        }
    };

    // 4. Enviar por NIC
    let mut registry = NIC_REGISTRY.lock();
    if let Some(nic) = registry.get_mut(nic_id) {
        nic.send_packet(&packet).ok();
    }

    Ok(data.len())
}
```

**Funciones helper necesarias:**

```rust
// En src/net/ethernet.rs:
pub fn build_ethernet_frame(eth: &EthernetHeader, ip: &Ipv4Header, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(ETH_HDR_LEN + IPV4_HDR_MIN_LEN + payload.len());
    frame.extend_from_slice(eth.as_bytes());
    frame.extend_from_slice(ip.as_bytes());
    frame.extend_from_slice(payload);
    frame
}

// En src/net/udp.rs:
pub fn build_udp_datagram(src_port: u16, dst_port: u16, payload: &[u8]) -> Vec<u8> {
    let udp_len = UDP_HDR_LEN + payload.len();
    let mut pkt = Vec::with_capacity(udp_len);
    let hdr = UdpHeader::new(src_port, dst_port, udp_len as u16);
    pkt.extend_from_slice(hdr.as_bytes());
    pkt.extend_from_slice(payload);
    // Calcular checksum UDP (pseudo-header IPv4)
    pkt
}

// En src/net/arp.rs o nic.rs:
pub fn resolve_dest_mac(ip: Ipv4Addr, nic_id: u32) -> MacAddr {
    arp_lookup(ip).unwrap_or_else(|| {
        // Si no está en cache, enviar ARP request (síncrono = bloqueante)
        arp_request(ip, nic_id);
        // Esperar respuesta (timeout 1s) o devolver broadcast
        arp_lookup(ip).unwrap_or(MacAddr::broadcast())
    })
}
```

#### Gap 2: Receive path (NIC → socket)

**Estado:** `net_handle_incoming_packet()` maneja ARP e ICMP echo request,
pero no rutea paquetes TCP/UDP a sockets.

**Código actual (en `src/net/mod.rs:72-191`):**

```
net_handle_incoming_packet(nic_id, packet)
  → parse Ethernet header
  → if ARP: handle arp request/reply
  → if IPv4:
      → parse IPv4 header
      → if ICMP: handle echo request → send reply
      → (TCP/UDP: no dispatch)
```

**Código necesario:**

```rust
// Después de parsear IPv4 y verificar protocolo:
if ip_hdr.protocol() == IPV4_PROTO_TCP {
    let tcp_payload = &payload[..];
    // Parse TCP header
    let tcp_hdr: &TcpHeader = unsafe { &*(tcp_payload.as_ptr() as *const TcpHeader) };
    let tcp_data = &tcp_payload[tcp_hdr.data_offset()..];

    // Buscar socket por (dst_ip, dst_port)
    let mut mgr = SOCKET_MANAGER.lock();
    let socket_id = find_socket_by_port(mgr, ip_hdr.dst_ip(), tcp_hdr.dst_port());

    if let Some(socket_id) = socket_id {
        let socket = mgr.get_socket_mut(socket_id).unwrap();

        if socket.direction == SocketDirection::Connected {
            // Push data a recv_buf
            socket.recv_buf.extend_from_slice(tcp_data);
            // Wake readers
            mgr.wake_socket_readers(socket_id);
        }
        // Para Listen → accept (future)
    }
    // No socket → enviar RST (future)
}

if ip_hdr.protocol() == IPV4_PROTO_UDP {
    let udp_payload = &payload[..];
    let udp_hdr: &UdpHeader = unsafe { &*(udp_payload.as_ptr() as *const UdpHeader) };
    let udp_data = &udp_payload[UDP_HDR_LEN..];

    // Buscar socket por (dst_port)
    let mut mgr = SOCKET_MANAGER.lock();
    let socket_id = find_socket_by_port_raw(mgr, udp_hdr.dst_port());

    if let Some(socket_id) = socket_id {
        let socket = mgr.get_socket_mut(socket_id).unwrap();
        socket.recv_buf.extend_from_slice(udp_data);
        mgr.wake_socket_readers(socket_id);
    } else {
        // No UDP socket en ese puerto → enviar ICMP Port Unreachable
        send_icmp_port_unreachable(nic_id, ip_hdr, udp_hdr);
    }
}
```

**Funciones helper:**

```rust
fn find_socket_by_port(mgr: &SocketManager, dst_ip: Ipv4Addr, dst_port: u16) -> Option<u32> {
    mgr.sockets.iter().flatten().find(|s| {
        s.direction == SocketDirection::Connected
        && s.socket_type == SocketType::Tcp
        && s.local.port() == dst_port
        && s.remote.ip == dst_ip  // opcional para filtrado estricto
    }).map(|s| s.id)
}

fn find_socket_by_port_raw(_mgr: &SocketManager, _port: u16) -> Option<u32> {
    // Búsqueda más permisiva (cualquier IP, puerto match)
}
```

#### Gap 3: ARP para transmisión

Cuando construimos un paquete para enviar, necesitamos la MAC destino.
Si no está en cache ARP, debemos hacer ARP request síncrono.

```rust
pub fn resolve_mac_with_arp(ip: Ipv4Addr, nic_id: u32) -> MacAddr {
    // 1. Buscar en cache ARP
    if let Some(mac) = arp_lookup(ip) {
        return mac;
    }

    // 2. No en cache → enviar ARP request
    let mac = get_nic_mac(nic_id);
    let broadcast = MacAddr::broadcast();
    let arp_req = arp_make_packet(ARP_OP_REQUEST, mac, get_nic_ip(nic_id), broadcast, ip);

    // Enviar
    let mut registry = NIC_REGISTRY.lock();
    let _ = registry.get_mut(nic_id).map(|nic| nic.send_packet(&arp_req));
    drop(registry);

    // 3. Esperar respuesta (spinning o yield con timeout)
    for _ in 0..100_000 {
        if let Some(mac) = arp_lookup(ip) {
            return mac;
        }
        core::hint::spin_loop();
        // Alternativa: yield y check in loop con time limit
    }

    // 4. Timeout: broadcast
    MacAddr::broadcast()
}
```

#### Gap 4: Socket creation con nic_id

Cuando se crea un socket (`sys_ob_create` con ObType::Socket), el kernel debe
asignar automáticamente una NIC al socket (la primera NIC disponible):

```rust
// En handler_ob_create, case ObType::Socket:
let socket_type = ((attrs & 0xFF) as u8).into();
let port = ((attrs >> 8) & 0xFFFF) as u16;

let mut mgr = SOCKET_MANAGER.lock();
let id = mgr.alloc_socket(socket_type).ok_or(Err(...))?;
let socket = mgr.get_socket_mut(id).unwrap();

// Asignar NIC por defecto
let nic_id = {
    let registry = NIC_REGISTRY.lock();
    if registry.nic_count() > 0 {
        Some(0u32)
    } else {
        None
    }
};
socket.nic_id = nic_id;

if port != 0 {
    socket.local = SocketAddrV4::new(get_nic_ip(0), port);
}
```

### 7.3 Plan de implementación por fases

**Fase 1 — Transmit path (socket → NIC):**

Archivos a modificar:
- `src/net/socket.rs` — `socket_send()` construye headers y transmite
- `src/net/udp.rs` — función `build_udp_datagram()` + `calc_udp_checksum()`
- `src/net/tcp.rs` — función `build_tcp_segment()` (segmento TCP con seq/ack)
- `src/net/arp.rs` — función `resolve_mac_with_arp()` (síncrona con timeout)
- `src/net/ethernet.rs` — función `build_ethernet_frame()`

**Fase 2 — UDP dispatch (NIC → socket UDP):**

Archivos a modificar:
- `src/net/mod.rs` — `net_handle_incoming_packet()` añadir rama UDP
- `src/net/socket.rs` — `find_socket_by_port()` para UDP
- `src/net/icmp.rs` — `build_port_unreachable()` para ICMP Port Unreachable

**Fase 3 — TCP dispatch (NIC → socket TCP):**

Archivos a modificar:
- `src/net/mod.rs` — `net_handle_incoming_packet()` añadir rama TCP
- `src/net/tcp.rs` — manejar paquetes entrantes (SYN, ACK, data, FIN, RST)
- `src/net/socket.rs` — `find_socket_by_port()` para TCP

**Fase 4 — TCP real (three-way handshake):**

Arquitectura completa de la conexión TCP:
1. `socket_connect()` → envía SYN → estado SYN_SENT
2. NIC recibe SYN+ACK → kernel pone socket en estado Established → wake connect waiter
3. `socket_send()` → envía segmentos TCP con seq++ y ACK
4. NIC recibe ACK → actualiza estado TCP
5. `socket_close()` → envía FIN → estado FIN_WAIT

### 7.4 Timeline de dependencias

```
Fase 1 (kernel): Transmit path (socket_send → NIC)
  ├── socket_send construye Ethernet+IP+protocolo
  ├── UDP datagram builder
  └── ARP resolve síncrono
  ↓
Fase 2 (kernel): UDP receive (NIC → socket recv_buf)
  ├── net_handle_incoming_packet UDP dispatch
  ├── UDP socket lookup por puerto
  └── wake_socket_readers
  ↓
Fase 3 (kernel): TCP receive (NIC → socket)
  ├── TCP packet dispatch in net_handle_incoming_packet
  └── TCP state machine real (handshake)
  ↓
Fase 4 (libneodos): Exponer ob_type::SOCKET, info classes, wrappers
  ↓
Fase 5 (net.nxl): API userland completa
  ↓
Fase 6 (aplicaciones): ipconfig.nxe, ping.nxe, dhcp.nxe
  ↓
Fase 7 (NeoInit): Migración a Registry, servicios auto-start
  ↓
Fase 8 (pkg.nxe): Sistema de paquetes v1
```

---

## 8. Roadmap

### 8.1 Antes de v1.0 (orden de implementación)

| # | Fase | Tarea | Archivos | Esfuerzo |
|---|------|-------|----------|----------|
| 1 | F1 | Kernel: Ethernet frame builder | `ethernet.rs` | Pequeño |
| 2 | F1 | Kernel: UDP datagram builder | `udp.rs` | Pequeño |
| 3 | F1 | Kernel: ARP resolve síncrono para TX | `arp.rs` | Medio |
| 4 | F1 | Kernel: socket_send transmite por NIC | `socket.rs` | Medio |
| 5 | F2 | Kernel: UDP dispatch en receive path | `mod.rs`, `socket.rs` | Medio |
| 6 | F2 | Kernel: ICMP Port Unreachable | `icmp.rs` | Pequeño |
| 7 | F3 | Kernel: TCP receive dispatch | `mod.rs`, `tcp.rs` | Medio |
| 8 | F4 | Kernel: TCP real three-way handshake | `tcp.rs`, `socket.rs` | Grande |
| 9 | F4 | libneodos: SOCKET constant + wrappers | `libneodos/syscall.rs` | Pequeño |
| 10 | F5 | net.nxl: librería userland de red | `libnet/` (nuevo) | Medio |
| 11 | F6 | ipconfig.nxe: herramienta userland | `userbin/ipconfig/` | Pequeño |
| 12 | F6 | ping.nxe: ICMP userland | `userbin/ping/` | Medio |
| 13 | F6 | dhcp.nxe: DHCP client | `userbin/dhcp/` | Grande |
| 14 | F7 | Registry: crear valores por defecto en boot | `main.rs`, `cm/mod.rs` | Pequeño |
| 15 | F7 | NeoInit: leer Registry para DefaultShell | `userbin/neoinit/` | Pequeño |
| 16 | F7 | NeoInit: auto-start de servicios | `userbin/neoinit/` | Medio |
| 17 | F7 | netcfg.nxe: servicio de configuración de red | `userbin/netcfg/` | Medio |
| 18 | F7 | Registry: persistencia a disco (cm_flush_key) | `cm/mod.rs`, `cm/hive.rs` | Medio |
| 19 | F8 | pkg.nxe: sistema de paquetes v1 | `userbin/pkg/` | Grande |

### 8.2 Después de v1.0

| # | Tarea | Descripción |
|---|-------|-------------|
| 20 | DNS: resolución en net.nxl | Consulta DNS sobre UDP |
| 21 | dnsresv.nxe | Herramienta de resolución DNS |
| 22 | TCP accept() | Aceptar conexiones entrantes |
| 23 | NeoPkg repositorio | Servidor de paquetes oficial |
| 24 | Firmas digitales | Ed25519 en .npkg |
| 25 | NeoSetup | Instalador en modo texto/gráfico |
| 26 | netstat.nxe | Estadísticas de red |
| 27 | traceroute.nxe | Traza de ruta ICMP |
| 28 | HTTP client library | Futura `http.nxl` |

---

## 9. Diagramas de Flujo

### 9.1 Boot completo con red

```
Bootloader (UEFI)
  ↓
Kernel
  ├── Phase 1-2: GDT, IDT, paging, heap, APIC, HPET, ACPI
  ├── Phase 3.0-3.85: Drivers, PCI, AHCI, GPT, FS mount
  ├── Phase 3.88: init_networking()
  │   ├── create \Device\Tcp, \Device\Udp, \Device\Nic
  │   ├── probe_e1000() → NIC 0 found
  │   ├── set NIC 0 IP = 10.0.1.80 (temporal)
  │   ├── arp_tick starts
  │   └── network_poll_all starts (in idle loop)
  │
  ├── Phase 3.881: init_cm()
  │   ├── create \Registry\Machine, \Registry\User
  │   ├── mount SYSTEM hive
  │   └── create_default_registry_values()
  │       ├── CurrentControlSet\Services\NeoInit\DefaultShell = "C:\Programs\NeoShell.nxe"
  │       ├── CurrentControlSet\Services\NeoInit\AutoStartServices = "netcfg"
  │       ├── CurrentControlSet\Services\netcfg\Path = "C:\Programs\netcfg.nxe"
  │       ├── Network\Interfaces\0\DHCPEnabled = 1
  │       └── CurrentControlSet\Control\WaitForNetwork = 0
  │
  ├── Phase 3.9: ABI freeze, validate syscalls
  │
  └── Phase 4: Spawn NeoInit (PID 1)
       │
       ▼
  NeoInit (PID 1)
       │
       ├── [kernel API: cm_open_key, cm_query_value]
       │
       ├── Abrir \Registry\Machine\System\CurrentControlSet\Services\NeoInit
       │
       ├── Leer DefaultShell → "C:\Programs\NeoShell.nxe"
       │
       ├── Leer AutoStartServices → "netcfg"
       │
       ├── Para cada servicio en AutoStartServices:
       │   │
       │   ├── Abrir \Registry\...\Services\netcfg
       │   ├── Leer Path → "C:\Programs\netcfg.nxe"
       │   │
       │   └── spawn_detached("C:\Programs\netcfg.nxe")
       │        │
       │        ▼
       │   netcfg.nxe (PID 2)
       │       │
       │       ├── load_net() = sys_loadlib("C:\System\Libraries\net.nxl")
       │       │   └── kernel carga net.nxl en slot 3 (0x1e0c0000)
       │       │
       │       ├── net_interface_count() → 1
       │       │
       │       ├── Leer Registry: DHCPEnabled=1
       │       │
       │       ├── dhcp_discover():
       │       │   ├── net_socket_create(UDP, port 68)
       │       │   ├── net_socket_bind(0.0.0.0:68)
       │       │   ├── net_socket_connect(255.255.255.255:67)
       │       │   ├── Construir DHCP Discover
       │       │   ├── net_socket_send() → kernel: build UDP+IP+Ethernet → e1000 TX
       │       │   ├── (espera DHCP Offer)
       │       │   ├── kernel: e1000 RX → parse UDP → find socket(port 68) → recv_buf
       │       │   ├── net_socket_recv() → DHCP Offer
       │       │   ├── Construir DHCP Request
       │       │   ├── net_socket_send() → e1000 TX
       │       │   ├── net_socket_recv() → DHCP ACK
       │       │   └── Config: IP=10.0.2.15, Gateway=10.0.2.1, DNS=10.0.2.3
       │       │
       │       ├── Guardar en Registry:
       │       │   \Registry\Machine\System\Network\Interfaces\0\IP = "10.0.2.15"
       │       │   \Registry\Machine\System\Network\Interfaces\0\Gateway = "10.0.2.1"
       │       │   \Registry\Machine\System\Network\Interfaces\0\DNS = "10.0.2.3"
       │       │
       │       ├── net_set_ip(0, 10.0.2.15) → kernel actualiza NIC IP
       │       ├── net_set_gateway(0, 10.0.2.1)
       │       └── exit (netcfg termina)
       │
       ├── Leer WaitForNetwork=0 → no esperar
       │
       └── Loop: spawn NeoShell.nxe → wait → respawn
```

### 9.2 Flujo detallado: net_socket_send (UDP)

```
user: net_socket_send(fd=3, data=DHCP_Discover)

  1. net.nxl (Ring 3):
     ob_set_info(3, SOCKET_SEND=21, &data, len)

       ↓ INT 0x80

  2. Kernel: handler_ob_set_info (src/syscall/ob.rs)
     → info_class == SocketSend(21)
     → buscar ObObject por fd → native_id = socket_id
     → llamar socket_send(socket_id, data)

       ↓

  3. Kernel: socket_send (src/net/socket.rs)
     → lock SOCKET_MANAGER
     → get socket by id
     → socket.nic_id = 0 (primera NIC)
     → socket.remote = 255.255.255.255:67
     → socket.local = 0.0.0.0:68
     → socket_type = UDP (2)
     → direction = Connected

     → 3a. Construir UDP datagram:
         → udp_build_datagram(src_port=68, dst_port=67, data)
         → UDP header src=68 dst=67 len=... checksum=0

     → 3b. Construir IPv4 header:
         → src IP = 0.0.0.0 (o IP de NIC)
         → dst IP = 255.255.255.255
         → proto = UDP (17)
         → length = UDP_hdr + data
         → checksum

     → 3c. Resolver MAC destino:
         → dst IP = 255.255.255.255
         → ARP lookup: no encontrado (broadcast)
         → MAC destino = FF:FF:FF:FF:FF:FF

     → 3d. Construir Ethernet frame:
         → src MAC = NIC MAC
         → dst MAC = FF:FF:FF:FF:FF:FF
         → type = 0x0800 (IPv4)
         → payload = IPv4 header + UDP header + data

     → 3e. Transmitir:
         → lock NIC_REGISTRY
         → nic[0].send_packet(&ethernet_frame)
         → unlock NIC_REGISTRY

     → 3f. Liberar:
         → unlock SOCKET_MANAGER
         → return Ok(len)

       ↓

  4. Kernel: e1000 send_packet (src/net/e1000.rs)
     → Wait for available TX descriptor
     → Copy packet data to DMA buffer
     → Update TX descriptor (addr, len, cmd)
     → Ring doorbell (TDT register)
     → Return Ok

       ↓

  5. Hardware: e1000 NIC transmite por cable Ethernet
```

### 9.3 Flujo detallado: net_socket_recv (UDP)

```
  Hardware: e1000 NIC recibe paquete Ethernet

       ↓

  1. Kernel: network_poll_all (src/net/mod.rs)
     → lock NIC_REGISTRY
     → for each NIC:
         → nic.poll_packet(&mut buf) → Some(len)
         → net_handle_incoming_packet(nic_id, &buf[..len])

       ↓

  2. Kernel: net_handle_incoming_packet
     → Parse Ethernet header → type = 0x0800 (IPv4)
     → Parse IPv4 header → proto = 17 (UDP)
     → UDP dispatch:

       → Parse UDP header
         → src_port = 67, dst_port = 68
         → udp_data = payload after UDP header

       → Buscar socket UDP con local.port == 68
         → SOCKET_MANAGER.lock()
         → sockets.iter().find(|s| s.socket_type == UDP && s.local.port == 68)
         → Socket found! id = 1
         → socket.recv_buf.extend_from_slice(udp_data)
         → mgr.wake_socket_readers(1)
         → magic = 0x0009_1000 | 1
         → scheduler.wake_blocked_on_magic(magic)
         → unlock SOCKET_MANAGER

       ↓

  3. Kernel: scheduler
     → Thread T (PID 2, dhcp.nxe) está Blocked con magic = 0x0009_1001
     → scheduler encuentra thread bloqueado
     → thread.state = Ready
     → set NEED_RESCHED

       ↓  (próximo timer tick o syscall return)

  4. Kernel: schedule()
     → Selecciona Thread T (dhcp.nxe) como next
     → Context switch to Ring 3

       ↓

  5. user: dhcp.nxe re-ejecuta net_socket_recv()
     → net.nxl:
       → ob_query_info(3, SocketRecv(23), &buf)
         → kernel: socket_recv() → copy recv_buf → return len
       → return Ok(len)

     → dhcp.nxe recibe DHCP Offer payload
```

---

## 10. Consideraciones de Implementación

### 10.1 net.nxl como NXL independiente vs linked static

**Decisión:** NXL independiente en slot 3 (`0x1e0c0000`).

| Aspecto | NXL separado | Linked static en libneodos |
|---------|-------------|---------------------------|
| Tamaño de libneodos | Pequeño (solo base) | Crece con net |
| Carga | Bajo demanda | Siempre cargado |
| Actualización | Independiente | Requiere recompilar libneodos |
| Aplicaciones sin red | No cargan net.nxl | Pagan memoria aunque no usen red |
| Complejidad | Mayor (ABI entre NXLs) | Menor (todo en una lib) |

net.nxl se carga bajo demanda:

```rust
// En el stub de libneodos:
pub fn load_net() -> Result<u64, i64> {
    loadlib("C:\\System\\Libraries\\net.nxl")
}
```

O lazy desde cada NXE:

```rust
// En ipconfig.nxe:
let net_base = sys_loadlib("C:\\System\\Libraries\\net.nxl").unwrap();
let net = unsafe { &*(net_base as *const NetAbiTable) };
```

### 10.2 net.nxl acceso a syscalls sin libneodos

net.nxl no debe depender de libneodos NXL. En su lugar, usa inline asm
directamente para las syscalls que necesita:

```rust
// libnet/src/syscall.rs — wrappers locales
fn ob_create(path: *const u8, obj_type: u32, fds: *mut u64, attrs: u64) -> i64 {
    let r: i64;
    unsafe {
        core::arch::asm!(
            "push rbx", "push rcx", "push rdx", "push r8",
            "mov rbx, {p}", "mov rcx, {t}", "mov rdx, {f}", "mov r8, {a}",
            "mov rax, 61", "int 0x80",
            "pop r8", "pop rdx", "pop rcx", "pop rbx",
            p = in(reg) path as u64,
            t = in(reg) obj_type as u64,
            f = in(reg) fds as u64,
            a = in(reg) attrs,
            out("rax") r,
            options(nostack),
        );
    }
    r
}
// ... similar para ob_set_info (63), ob_query_info (62), ob_wait (65), ob_close (13)
```

### 10.3 Nombrado de objetos Ob para sockets

Los sockets se crean con paths únicos en el namespace Ob:

```
\Ob\Socket\<pid>\<name>
```

Donde `<name>` es el path proporcionado por `net_socket_create()`.

Ejemplos:
- `\Ob\Socket\2\DhcpClient` — socket DHCP de netcfg (PID 2)
- `\Ob\Socket\3\Ping` — socket ping de ping.nxe (PID 3)

Si `net_socket_create()` recibe un path relativo, net.nxl lo completa:

```rust
pub fn net_socket_create(path: &str, socket_type: u32) -> Result<u8, NetError> {
    let pid = syscall::sys_getpid();
    let full_path = alloc::format!("\\Ob\\Socket\\{}\\{}", pid, path.trim_start_matches('\\'));
    // ...
    syscall::sys_ob_create(&full_path, 18, None, attrs as u64)
}
```

### 10.4 Recepción de datos: SocketRecv vs SocketInfo

Para la recepción de datos, el diseño usa `ObInfoClass::SocketRecv = 23`.
Este nuevo info class no existe actualmente en el kernel.

**Implementación en el kernel:**

```rust
// En src/syscall/ob.rs, handler_ob_query_info:
_ if info_class == 23 /* SocketRecv */ => {
    let ob = ob_table.lock().get(fd).ok_or(SyscallError::BadF)?;
    if ob.obj_type != ObType::Socket {
        return Err(SyscallError::InvalidParam);
    }
    let socket_id = ob.native_id as u32;

    let mut mgr = SOCKET_MANAGER.lock();
    let socket = mgr.get_socket_mut(socket_id).ok_or(SyscallError::NotFound)?;

    let available = socket.recv_buf.len().min(user_buf.len());
    if available == 0 {
        return Err(SyscallError::Again);  // -EAGAIN
    }

    user_buf[..available].copy_from_slice(&socket.recv_buf[..available]);
    socket.recv_buf.drain(..available);

    Ok(available as i64)
}
```

**ObInfoClass::SocketRecv debe añadirse al enum:**

```rust
// src/object/types.rs
pub enum ObInfoClass {
    // ... existentes hasta NicInfo=20 ...
    RegistryKey = 21,
    RegistryValue = 22,
    SocketRecv = 23,       // ← nuevo
}
```

### 10.5 Puerto efímero para sockets

Cuando un socket UDP/TCP se crea sin bind explícito, el kernel debe asignar
un puerto efímero automáticamente.

```rust
const EPHEMERAL_PORT_START: u16 = 49152;
const EPHEMERAL_PORT_END: u16 = 65535;
static NEXT_EPHEMERAL_PORT: AtomicU16 = AtomicU16::new(EPHEMERAL_PORT_START);

fn alloc_ephemeral_port() -> u16 {
    loop {
        let port = NEXT_EPHEMERAL_PORT.fetch_add(1, Ordering::Relaxed);
        let port = if port < EPHEMERAL_PORT_START { EPHEMERAL_PORT_START } else { port };
        // Verificar que no esté en uso (simplificado)
        if port <= EPHEMERAL_PORT_END {
            return port;
        }
    }
}
```

### 10.6 netcfg.nxe — implementación

```
netcfg.nxe
  │
  ├── loadlib("C:\\System\\Libraries\\net.nxl")
  │
  ├── net_interface_count()  → si 0, exit
  │
  ├── Abrir Registry:
  │   fd = cm_open_key("\\Registry\\Machine\\System\\Network\\Interfaces\\0")
  │   dhcp = cm_query_dword(fd, "DHCPEnabled")
  │   ip    = cm_query_str(fd, "IP")
  │   gw    = cm_query_str(fd, "Gateway")
  │   dns   = cm_query_str(fd, "DNS1")
  │
  ├── if dhcp == 1:
  │   ✓ Ejecutar dhcp como subproceso
  │     (o llamar funciones DHCP de net.nxl si existen)
  │   dhcp_result = dhcp_discover_and_configure()
  │   ip = dhcp_result.ip
  │   gw = dhcp_result.gateway
  │   dns = dhcp_result.dns
  │   ✓ Guardar en Registry
  │
  ├── if ip != "0.0.0.0":
  │   ✓ net_set_ip(0, parse_ip(ip))
  │   ✓ net_set_gateway(0, parse_ip(gw))
  │
  └── exit(0)
```

### 10.7 Persistencia del Registry

El hive SYSTEM se serializa a disco en:
- `C:\System\Registry\SYSTEM.hiv`

En boot, antes de `init_cm()`, el kernel intenta cargar el hive desde disco:

```rust
fn init_cm() {
    // Intentar cargar hive desde disco
    let data = read_file("C:\\System\\Registry\\SYSTEM.hiv");
    if let Ok(data) = data {
        // Parsear y reconstruir Hive desde bytes
        let hive = Hive::deserialize(&data);
        mount_hive("SYSTEM", "\\Registry\\Machine\\System", hive);
    } else {
        // No existe → crear hive nuevo
        let hive = Hive::new("SYSTEM");
        mount_hive("SYSTEM", "\\Registry\\Machine\\System", hive);
        create_default_registry_values();
    }
}
```

### 10.8 Estrategia de test

| Componente | Tests | Herramienta |
|-----------|-------|-------------|
| Kernel transmit path | Tests unitarios de socket_send con mock NIC | kernel test framework |
| Kernel receive path | Tests con paquetes Ethernet sintéticos | kernel test framework |
| Socket UDP dispatch | Crear socket UDP, enviar paquete sintético, verificar recv_buf | kernel test framework |
| libneodos wrappers | Tests de compilación (no ejecución) | cargo test (host) |
| net.nxl | Tests unitarios de parsing (no requieren NIC) | cargo test (host) con mock de syscall |
| ipconfig.nxe | Test de integración: ejecutar ipconfig y verificar salida | auto_test.py |
| ping.nxe | Enviar ping a 127.0.0.1 (loopback futura) o QEMU host | auto_test.py |
| dhcp.nxe | Simular servidor DHCP, verificar client | auto_test.py |
| NeoInit Registry | Test unitario de cm_open_key + cm_query_value | kernel test framework |
| pkg.nxe | Test de instalación/remoción con paquete de prueba | auto_test.py |

---

## 11. Apéndice: Cambios por Archivo

### 11.1 Kernel source

| Archivo | Cambio | Fase |
|---------|--------|------|
| `src/net/ethernet.rs` | Añadir `build_ethernet_frame()` | F1 |
| `src/net/arp.rs` | Añadir `resolve_mac_with_arp()` síncrono | F1 |
| `src/net/udp.rs` | Añadir `build_udp_datagram()`, `calc_udp_checksum()` | F1 |
| `src/net/tcp.rs` | Añadir `build_tcp_segment()`, implementar handshake real | F3/F4 |
| `src/net/socket.rs` | Modificar `socket_send()` para transmitir por NIC | F1 |
| `src/net/socket.rs` | Añadir `socket_set_nic()`, `alloc_ephemeral_port()` | F1 |
| `src/net/mod.rs` | Añadir dispatch UDP en `net_handle_incoming_packet()` | F2 |
| `src/net/mod.rs` | Añadir dispatch TCP en `net_handle_incoming_packet()` | F3 |
| `src/net/icmp.rs` | Añadir `build_port_unreachable()`, `build_echo_request()` | F2 |
| `src/object/types.rs` | Añadir `ObInfoClass::SocketRecv = 23` | F1 |
| `src/syscall/ob.rs` | Añadir handler para SocketRecv (class 23) en query_info | F1 |
| `src/syscall/ob.rs` | Verificar que SocketSend/SocketClose usan ruta kernel correcta | F0 |
| `src/syscall/ob.rs` | Asignar nic_id automático en creación de socket | F1 |
| `src/cm/mod.rs` | Añadir `create_default_registry_values()` | F7 |
| `src/cm/mod.rs` | Implementar `cm_flush_key()` con serialización | F7 |
| `src/cm/hive.rs` | Añadir `serialize()`, `deserialize()` | F7 |
| `src/main.rs` | Llamar `create_default_registry_values()` en boot | F7 |

### 11.2 libneodos

| Archivo | Cambio | Fase |
|---------|--------|------|
| `src/syscall.rs` | Añadir `ob_type::SOCKET = 18` | F4 |
| `src/syscall.rs` | Añadir `ObInfoClass::SocketInfo(17)` .. `SocketRecv(23)` | F4 |
| `src/syscall.rs` | Añadir `ob_set_info_class::SOCKET_CONNECT(18)` .. `SOCKET_CLOSE(22)` | F4 |
| `src/syscall.rs` | Añadir `ob_socket_create()`, `ob_socket_connect()`, etc. | F4 |

### 11.3 Nuevos proyectos

| Proyecto | Path | Produce | Fase |
|----------|------|---------|------|
| libnet | `libnet/` | `net.nxl` → `C:\System\Libraries\net.nxl` | F5 |
| ipconfig | `userbin/ipconfig/` | `ipconfig.nxe` | F6 |
| ping | `userbin/ping/` | `ping.nxe` | F6 |
| dhcp | `userbin/dhcp/` | `dhcp.nxe` | F6 |
| netcfg | `userbin/netcfg/` | `netcfg.nxe` | F7 |
| pkg | `userbin/pkg/` | `pkg.nxe` | F8 |

### 11.4 Scripts

| Script | Propósito | Fase |
|--------|-----------|------|
| `scripts/create_neodos_image.py` | Añadir net.nxl, ipconfig.nxe, ping.nxe, dhcp.nxe a la imagen | F6 |
| `scripts/init_registry.py` | Generar SYSTEM.hiv inicial con valores por defecto | F7 |
| `scripts/create_test_package.py` | Crear paquete .npkg de prueba | F8 |

---

*Este documento es un diseño arquitectónico. Ver `docs/IMPROVEMENTS.md` para el
estado actual de cada tarea.*
