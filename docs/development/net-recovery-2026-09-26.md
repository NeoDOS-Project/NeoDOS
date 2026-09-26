# Recuperación de red de `netd` (SMP) — Informe forense

**Fecha:** 2026-09-26
**Rama:** `develop` @ `00e61e1` (v0.50.5)
**Entorno validado:** QEMU q35 + OVMF + e1000, `smp-ap-sched`, `-netdev user` (SLiRP)
**Objetivo:** recuperar/verificar NIC → driver → netd → stack → conectividad.

---

## 1. Estado actual

| Capa | Antes | Después |
|------|-------|---------|
| PCI discovery | PASS | PASS |
| NIC driver (`e1000.nem`) detectado/cargado | PASS (cargaba un binario obsoleto) | PASS (binario correcto) |
| ABI driver↔kernel `hst_register_network_device` | **FAIL** (5 args vs 9) | PASS |
| DMA descriptor rings alineados | **FAIL** (base +1 byte) | PASS (`0x...000`) |
| `send_fn`/`poll_fn` | basura (`0x405ef10`/`0x3`) | `0x3010008a`/`0x30100002` |
| Link | UP | UP |
| TX | 0 paquetes en el netdev | DHCP DISCOVER/REQUEST/ARP salen |
| RX | 0 paquetes | OFFER/ACK `rx_pkt len=590` |
| DHCP (DORA) | nunca | `IP=10.0.1.80 mask=255.255.255.0 gw=10.0.1.1` |
| Servicio `Dhcpc` | hilo `Suspended`, nunca ejecuta | ejecuta y configura en boot |
| `netd` CPU | CPU0 (con AP-sched off inunda el BSP) | CPU1 con `smp-ap-sched` |
| ICMP end-to-end | no medido | handler + test unitario OK; `ping.nxe` no se pudo conducir por la automatización (ver §9) |

---

## 2. Arquitectura real (archivos concretos)

```
NIC (QEMU e1000 / VBox 82540EM)
  └─ drivers/e1000/src/lib.rs                 (NEM binario e1000.nem)
       registro vía hst_register_network_device
  └─ neodos-kernel/src/drivers/nem/net_bridge.rs   (NemNetworkDevice: send/poll)
  └─ neodos-kernel/src/net/nic.rs                    (NIC_REGISTRY, NetworkInterface)
  └─ neodos-kernel/src/net/mod.rs                    (netd_entry, net_tick,
                                                      network_poll_all,
                                                      net_handle_incoming_packet)
  └─ neodos-kernel/src/net/{ethernet,arp,ipv4,icmp,udp,tcp,dns,socket}.rs
  └─ syscalls Ob (ObType::Socket) → userbin/{dhcpd,ipconfig,ping,netcfg}
```

`netd` es un **kthread del kernel** (`netd_entry`), no un proceso de usuario.
Con `smp-ap-sched` corre en un AP (CPU1) y hace `network_poll_all()` (polling, sin IRQ de RX).

---

## 3. Root cause (primer punto de ruptura, en orden de cadena)

### RC-1 — L4/Bridge: `.nem` obsoleto con ABI incompatible
- El disco embebía **byte a byte** el `e1000.nem` antiguo (5608 B) en el offset
  `107089920`; el binario nuevo (6051 B) **no** estaba. No contenía las cadenas
  de descripción del ABI nuevo (`Intel 82540EM...`).
- El ABI cambió en `f28cb72` (+vendor/device/desc). El packer/loader no fuerza a
  reconstruir el `.nem` en builds `--quick`, así que se seguía cargando el binario
  de julio (5 args) contra un kernel de 9 args.
- Evidencia: `[NET] NEM callbacks: send=0x405ef10 poll=0x3`.
  `valid_callback_address(0x3)==false` ⇒ `poll_packet()` siempre `None` ⇒ **RX muerto**.
- Reproducido idéntico en VBox y QEMU, 1/2/4 CPUs.

### RC-2 — L3/DMA: el packer NEM no respetaba la alineación de secciones
- `RX_DESCS`/`TX_DESCS` se declaran `#[repr(align(4096))]`, pero `tools/nem-pack.py`
  concatenaba las `.bss.<sym>` ignorando `sh_addralign`, y el loader coloca las
  secciones sin alinear la base → anillo TX en `0x1BD35E61` (`& 0xF == 1`).
- El e1000 ignora los 4 bits bajos de `TDBAL`/`RDBAL`: QEMU leía el descriptor
  desplazado 1 byte y obtenía `length = 0`. El `filter-dump` mostró paquetes de
  longitud 0; el volcado físico del anillo lo confirmó.
- `netd` no recibía nada porque el dispositivo no transmitía correctamente.

### RC-3 — L4/SM: el Service Manager dejaba el hilo del servicio `Suspended`
- `spawn_usermode()` crea el hilo inicial en `Suspended`; el camino `ObCreate`/`ObWait`
  lo activa en el hand-off, pero el SM no pasa por ahí ⇒ `dhcpd` nunca era
  planificable ⇒ **sin DHCP automático en boot**.

---

## 4. VirtualBox

- NIC1 efectiva: **bridged** a `enp0s31f6`, `nictype1="82540EM"` (Intel e1000),
  MAC `000F4BDE9DE1`, cable on (`VBoxManage showvminfo NeoDOS --machinereadable`).
- Usa el **mismo** driver (`82540EM` = `8086:100e`) y el mismo stack, por lo que
  RC-1 y RC-2 rompían también VBox. El modo bridged exige DHCP/ARP de la LAN física;
  QEMU usa SLiRP (`10.0.1.0/24`, DHCP `10.0.1.1`).
- Con los fixes, la ruta VBox queda desbloqueada a nivel de código/driver; la
  validación VBox extremo a extremo queda pendiente de arrancar la VM con la imagen
  regenerada (ver §9).

---

## 5. QEMU

- Emula `e1000` en `00:03.0` (`v8086:100e`); ECAM activo; MMIO `0x81040000`.
- **No** era un problema de NIC/backend: antes del fix, `tx` del kernel subía pero
  el `netdev` no veía un solo paquete (anillo desalineado); después del fix, la
  captura pcap muestra la secuencia DORA completa + gratuitous ARP.

---

## 6. Interacción con SMP

- Con `--features smp-ap-sched`, `netd` se ejecuta en **CPU1** y el BSP continúa a
  NeoInit/shell. Se observó migración (`netd migrated cpu=1 -> 0` en una corrida).
- **Sin** `smp-ap-sched`, `netd` (PRIORITY_NORMAL) se queda en el BSP y, durante el
  `sleep_hint(300ms)` del boot thread, lo inunda ⇒ el boot no llega a NeoInit.
  No es una regresión de networking: es comportamiento del build por defecto.
- La red estaba rota de forma **idéntica en 1, 2 y 4 CPUs** ⇒ no era un fallo SMP.

---

## 7. Cambios realizados (mínimos)

| Archivo | Cambio |
|---------|--------|
| `tools/nem-pack.py` | parsear `sh_addralign`; alinear sub-secciones concatenadas; alinear a página `text/rodata/data/bss` |
| `neodos-kernel/src/services/manager.rs` | tras `spawn_usermode`, publicar el hilo del servicio `Ready` (misma primitiva que `ObWait`) |
| `neodos-kernel/src/net/mod.rs` | contadores reales `rx_packets/rx_bytes`; snapshot de `netd` (cpu + counters) de baja frecuencia y aviso de migración |
| `neodos-kernel/src/drivers/nem/net_bridge.rs` | contadores reales `tx_packets/tx_bytes` |
| Artefactos (gitignored) | `data/nem_bin/**` y `disk_image.img` regenerados con el packer corregido |

Sin cambios en el ABI de syscalls ni en el scheduler.

---

## 8. Validación

Comandos:
```bash
# Kernel con el feature SMP validado
cd neodos-kernel && cargo +nightly build --target x86_64-unknown-none --release --features smp-ap-sched
cp target/x86_64-unknown-none/release/neodos_kernel ../kernel.elf
# NEMs + userbin + NXL al día con el packer corregido
neodev build --userbin --nxl --nem --image
# QEMU e1000 + pcap
qemu-system-x86_64 -machine q35,accel=tcg -smp 2 ... \
  -netdev user,id=net0,net=10.0.1.0/24,dhcpstart=10.0.1.80,host=10.0.1.1 \
  -object filter-dump,id=f1,netdev=net0,file=net.pcap -device e1000,netdev=net0
```

Resultados:
```text
[NET] NEM callbacks: send=0x3010008a poll=0x30100002      # RC-1 corregido
[E1000TX] ring_phys=0x1BD39000 tdbal=0x1BD39000           # RC-2 corregido (alineado)
[NET] netd cpu=1 rx=2 tx=6 arp_rx=0 arp_tx=0 icmp_rx=0 icmp_tx=0
[dhcpd] OFFER from 10.0.1.1: IP=10.0.1.80
[dhcpd] DORA complete, IP=10.0.1.80 mask=255.255.255.0 gw=10.0.1.1 lease=86400s
[dhcpd] Network configured
```
pcap:
```text
0.0.0.0.68 > 255.255.255.255.67: BOOTP/DHCP, Request
10.0.1.1.67 > 255.255.255.255.68: BOOTP/DHCP, Reply
0.0.0.0.68 > 255.255.255.255.67: BOOTP/DHCP, Request
10.0.1.1.67 > 255.255.255.255.68: BOOTP/DHCP, Reply
ARP, Request who-has 10.0.1.80 tell 10.0.1.80
```
`ipconfig`:
```text
Driver: e1000_0003   PCI: 8086:100E   Link: Activa
MAC: 52:54:00:12:34:56
IPv4: 10.0.1.80   Mask: 255.255.255.0
```

Tests: 723 kernel + userbin/shell PASS.

---

## 9. Riesgos pendientes

1. **ICMP end-to-end no demostrado**: el handler ICMP y `build_echo_reply` existen y
   el test `net_icmp_echo_reply_build` pasa, pero `ping.nxe` no se pudo conducir de
   forma fiable porque la automatización de teclado (`neodev shell send`) no inyecta
   correctamente el **espacio** (`ping 10.0.1.1`, `echo A B`, `coredir C:\...` fallan;
   comandos sin args funcionan). No hay evidencia de fallo ICMP; es limitación de
   herramienta. Recomendado: teclear manualmente en VBox/serial interactivo y
   comprobar `ping 10.0.1.1`.
2. **Binarios pregenerados gitignored**: `data/nem_bin/**` y los `.nxe`/`.nxl` pueden
   quedar obsoletos respecto a la fuente. `neodev build --quick --image` **no** los
   reconstruye y usa el fallback. Acción: ejecutar `neodev build --userbin --nxl --nem --image`
   tras cambios de ABI, o forzar reconstrucción en `--quick`.
3. **`SYSCALL_CORRUPT` ruidoso**: aparece repetidamente durante cambios de contexto de
   usuario (p.ej. `pid=3 tid=5`). No impide la conectividad pero contamina el serial.
   Pre-existente; candidato a investigar aparte.
4. **Comportamiento del build por defecto (sin `smp-ap-sched`)**: `netd` inunda el BSP
   y el boot no progresa. No se tocó el scheduler; el feature validado es la ruta.
5. **Interbloqueos de `ipconfig`/daemons en foreground**: `ipconfig`/`dhcpd` lanzados
   desde el shell pueden dejar el shell en `ob_wait`. El servicio de boot evita esto.
6. **Alineación en formato NEM**: el fix alinea a página en el packer; el formato NEM
   no transporta `sh_addralign`. Es robusto para los drivers actuales, pero convendría
   añadir el dato de alineación al header NEM v3 en el futuro.

---

# RC-4 — Desbordamiento de hoja de directorio NE2 (image builder)

**Fecha:** 2026-09-26 · **Repo:** `neodev` v0.2.1 (externo) · **Sin cambios en kernel/red.

## Root cause

`neodev/src/image.rs::make_btree_leaf()` volcaba las entradas de un directorio en un
único bloque de 4096 B y, si no cabían, hacía `break` **dejando `count` con el total
solicitado**. Producía una hoja estructuralmente inconsistente:

```text
declared_count = 34
serialized_count = 28   (el resto quedaba a cero)
```

## Leaf capacity (derivada del propio formato)

```text
block size          = 4096 B
header              = 8 B   (u16 type + u16 count + u32 crc)
per-entry           = 4 + len(name) + 128 (direntry)
payload disponible  = 4088 B
capacidad real      = ~28 entradas (depende de la longitud de los nombres)
```

## Afectados

- `/Programs` tenía **34** entradas → se perdían `reboot, shtest, stresscmd, tree,
  ver, vol`; `ping.nxe` y `ps.nxe` existían físicamente pero la búsqueda por nombre
  fallaba (hoja corrupta).
- `/System/Locale/<lang>` tenía **47** NLTs → se perdían 19, **incluido
  `neoshell.nlt`**. Por eso el shell mostraba `"?"`: `i18n_get_id()` devuelve `"?"`
  cuando falta la cadena. **El `"?"` era un síntoma secundario; i18n no era la causa.**

Evidencia (VBox): `type C:\Programs\colors.nxe` funcionaba, `type C:\Programs\ping.nxe`
devolvía `Archivo no encontrado`; ambos ficheros existían byte a byte en la partición.

## Cambios (A + C)

**C — fail-fast (invariante):** `make_btree_leaf()` ahora devuelve `Err` si no caben
todas las entradas; nunca declara más de las que serializa. Diagnóstico con directorio,
entradas solicitadas, capacidad y bytes necesarios.

**A — rebalanceo:**
- `programs_nxe` 34 → 28; se movieron `reboot, shtest, stresscmd, tree, ver, vol` a
  `tools_nxe` (19). El PATH del shell incluye `System/Tools`, así que siguen visibles.
  Ningún programa se elimina, renombra ni modifica.
- Los NLTs no se pueden partir entre directorios nuevos (el loader i18n usa
  `System\Locale\{lang}\{app}.nlt`), así que se rebalancean **usando el fallback que ya
  existe** en el loader: `{lang}` → `{lang-only}` → `en-US`. Se reparten 47 NLTs en
  `es-ES` (28) + `es` (19); se conservan los 141 NLTs (47×3 idiomas). No se toca i18n.

**Correcciones de `neodev` descubiertas por la validación:**
- `build --userbin --nxl --nem --image` usaba `2560` bloques fijos (10 MB) en vez de
  `neodos_blocks` (25600 = 100 MB): imagen demasiado pequeña → `KERNEL PANIC
  (PAGE_TABLE_CORRUPTION)` al arrancar en QEMU. Corregido.
- El backend QEMU (`run` y `start_headless`) **ignoraba `cpus`**: no pasaba `-smp`, así
  que `neodev run/test` arrancaban 1 vCPU y `netd` quedaba en el BSP. Corregido.
- `smp-ap-sched` pasa a ser **feature por defecto** del kernel, de modo que el build
  estándar (`neodev build --image`) ya produce el kernel validado sin flags extra.

## Validación

- **Estática** (`data/neodos_image.img`): 16/16 hojas con `count == serializadas`
  (ningún problema). `/Programs` 28 (con `ping.nxe`, `ps.nxe`), `/System/Tools` 19
  (con los 6 movidos), `/System/Locale/es-ES` 28 + `/System/Locale/es` 19,
  total 141 NLTs.
- **`neodev test`**: 723 kernel + Command + Shell **PASSED**; OVERALL PASSED.
- **VirtualBox** (bridged, kernel feature por defecto, 2 vCPUs): DHCP `10.0.1.46`,
  `netd cpu=1`, 0 panics; `type C:\Programs\ping.nxe` ya no devuelve
  `Archivo no encontrado`; el shell muestra mensajes i18n correctos
  (`Comando o nombre de archivo incorrecto`) en lugar de `?`.

## ICMP guest → gateway

`ping.nxe` ya **se lanza** (lookup arreglado), pero crashea en runtime con `#PF` en
user mode (`type=14`, offset `0x17b7`, ruta del allocator/`sbrk`). Es un defecto
separado de `ping`/userland, fuera del alcance permitido (no se modifica `ping` ni
ICMP). La dirección `gateway → guest` ya quedó probada (`icmp_rx=97 icmp_tx=97`).

> El stack de red **ya era funcional antes** de este fix del image-builder; este
> cambio solo recupera entradas de directorio perdidas (programas e i18n) y corrige
> la herramienta de construcción.

