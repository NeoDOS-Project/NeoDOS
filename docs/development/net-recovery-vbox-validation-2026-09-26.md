# VirtualBox validation — regenerated image (bridged e1000)

**Fecha:** 2026-09-26
**VM:** `NeoDOS` (VirtualBox 7.2.20r175154), firmware EFI, chipset ICH9.
**NIC:** `nic1=bridged`, `bridgeadapter1=enp0s31f6`, `nictype1=82540EM` (Intel e1000),
MAC `00:0F:4B:DE:9D:E1`, cable on.
**LAN física:** `10.0.1.0/24`, host `10.0.1.10`, gateway/DHCP `10.0.1.1`, DNS `10.0.30.10`.
**Serial:** `uartmode1=file,/home/amartinper/rust-os/vbox_serial.log`.
**Disco:** `disk_image.vdi` regenerado desde `neodos/disk_image.img` (mismo GPT 212 MB).

Método de arranque/control:
```bash
VBoxManage storageattach NeoDOS --storagectl AHCI --port 0 --device 0 --type hdd --medium none
VBoxManage closemedium disk disk_image.vdi --delete
VBoxManage convertfromraw disk_image.img disk_image.vdi --format VDI
VBoxManage storageattach NeoDOS --storagectl AHCI --port 0 --device 0 --type hdd --medium disk_image.vdi
VBoxManage startvm NeoDOS --type headless
# teclado (sí maneja espacios, a diferencia de neodev shell):
VBoxManage controlvm NeoDOS keyboardputstring "ping 10.0.1.1"
VBoxManage controlvm NeoDOS keyboardputscancode 1c 9c
```

---

## Resultado: network stack PASS en VirtualBox

| Check | Resultado | Evidencia |
|-------|-----------|-----------|
| L1 PCI discovery | PASS | `[DRV] MATCH: 00:03.0 -> driver 'E1000' (v8086:100e, class=Network)` |
| L2 driver cargado | PASS | `[NEM] Loaded into isolated region @ 0x30000000` |
| RC-1 ABI callback | PASS | `[NET] NEM callbacks: send=0x3000008a poll=0x30000002` |
| Link | PASS | `Estado del enlace . . . : Activa` |
| RC-3 servicio DHCP | PASS | `[SM] started: Dhcpc` → `[dhcpd] DISCOVER` → `OFFER` → `ACK` → `Network configured` |
| L7 IPv4 config | PASS | `Direccin IPv4 10.0.1.46  Mscara 255.255.255.0  gw 10.0.1.1` (DHCP, lease 4000 s, DNS 10.0.30.10) |
| RC-2 anillos DMA | PASS (funcional) | TX=104 paquetes (DISCOVER/REQUEST/ARP/ICMP reply) y RX activo ⇒ anillos alineados |
| netd en AP | PASS | `[SMP] AP scheduling enabled` · `[NET] netd cpu=1` |
| L8 ICMP | PASS (reverso) | `icmp_rx=97 icmp_tx=97` (el guest recibe echo requests y responde echo replies) |
| Sin GPF/PF/panic | PASS | 0 ocurrencias de `KERNEL PANIC/DOUBLE FAULT/GPF/[EXC]` |

Evidencia literal:
```text
[DRV] MATCH: 00:03.0 -> driver 'E1000' (v8086:100e, class=Network)
[NEM] Loaded into isolated region @ 0x30000000 (124 KB, mode=basic)
[NET] NEM callbacks: send=0x3000008a poll=0x30000002
[NET] Registered NEM NIC 2147483648 as id 0
[SMP] AP scheduling enabled (smp-ap-sched)
[NET] netd running
[NET] netd cpu=1 rx=0 tx=0 ...
[dhcpd] OFFER from 10.0.1.1: IP=10.0.1.46
[dhcpd] ACK: IP=10.0.1.46 mask=255.255.255.0 gw=10.0.1.1 lease=4000s
[dhcpd] DORA complete, IP=10.0.1.46 ...
[dhcpd] Network configured
[NET] netd cpu=1 rx=375000 tx=104 arp_rx=1500 arp_tx=3 icmp_rx=97 icmp_tx=97
```

`ipconfig` en VBox:
```text
Adaptador Ethernet 0:
    Descripcin . . . . . . : Intel 82540EM Gigabit Ethernet
    Driver . . . . . . . . .: e1000_0003
    Dispositivo PCI . . . . : 8086:100E
    Estado del enlace . . . : Activa
    Direccin MAC . . . . . : 00:0F:4B:DE:9D:E1
    Direccin IPv4 . . . . .: 10.0.1.46
    Mscara de subred . . . : 255.255.255.0
    Puerta de enlace . . . .: 10.0.1.1
    Servidor DNS . . . . . .: 10.0.30.10
    Origen configuracin . .: DHCP
    Tiempo de concesin . . : 4000 s
```

Conclusión: **RC-1, RC-2 y RC-3 se validan también en VirtualBox sobre el path bridged real.**
El stack de red no necesitó ningún cambio adicional.

---

## Defecto nuevo, reproducible, NO relacionado con la red

Al intentar lanzar `ping` desde el shell, el comando responde `?` (la cadena
`IDS_BAD_COMMAND` del shell no se localiza y cae al fallback `"?"` de `i18n_get_id`),
y **no se crea el proceso** (no aparece `handler_ob_create Process ... ping.nxe`).

No es un problema de red ni del `ping`: afecta también a otros binarios.

Conjunto observado (determinista, reprod. varias veces):

| Binario | En `/Programs` | Lanza |
|---------|----------------|-------|
| cd, cmdtest, colors, core*, datetime, drives, echo, hostname, keyb, label, neoinit, neokey, neomem, neoshell, nxlocale, nxres, nxverify | sí | PASS |
| **ping**, **ps** | sí (listados) | **FAIL** (no se encuentra al abrir) |
| **reboot, shtest, stresscmd, tree, ver, vol** | **no** (ausentes del listado) | FAIL |

### Root cause: overflow de una hoja NE2 en `neodev` (herramienta externa)

`/Programs` recibe 34 nombres (`programs_nxe` en `neodev/src/image.rs`), pero
`make_btree_leaf()` vuelca en **un solo bloque de 4096 B** y, si no caben,
hace `break` **dejando el campo `count` con el total y el resto a cero**:

- Entrada de btree = `2 + len(nombre) + 2 + 128 (direntry)` ≈ 138–145 B.
- Caben 28 entradas (hasta `ps.nxe`, offset 3992 B). La 29ª (`reboot.nxe`)
  necesitaría 4134 B > 4096 ⇒ `break`.
- Resultado: hoja con `count=34`, 28 entradas reales y 6 vacías.
- La búsqueda del kernel por nombre falla para claves próximas al hueco
  (`ping`, `ps`), y los 6 ficheros descartados quedan inaccesibles
  (`reboot, shtest, stresscmd, tree, ver, vol`).

Verificación local (imagen NE2 y partición 2 del GPT, byte a byte):
```text
/Programs count=34
  ... nxverify.nxe (idx24)
  ping.nxe (idx25)   <- presente, direntry válida, datos == userbin/ping.nxe
  poweroff.nxe (idx26)
  ps.nxe (idx27)     <- presente, direntry válida
  idx28..33 = entradas vacías  <- 6 ficheros descartados
```
`type C:\Programs\colors.nxe` y `type C:\System\Tools\ipconfig.nxe` funcionan;
`type C:\Programs\ping.nxe` → `Archivo no encontrado`.

### Fix propuesto (en `neodev`, no en NeoDOS)

Opción A (mínima, sin tocar el formato): **rebalancear** `programs_nxe`/`tools_nxe`
en `neodev/src/image.rs` para que ningún directorio supere ~28 entradas
(p.ej. mover varios `core*`/`nx*` a `System/Tools`, que ahora tiene 13).

Opción B (correcta): soportar hojas de directorio multi-bloque o un nodo interno
en `build_ne2_image`/`make_btree_leaf` (y el lookup correspondiente en el kernel).

Opción C (defensiva): que `make_btree_leaf` **falle de forma ruidosa** si
`entries` no caben, en lugar de emitir una hoja inconsistente.

Ninguna de estas cambia el comportamiento de red. No se aplicó ningún cambio
de código en este paso (solo se regeneró el artefacto `disk_image.vdi`).

---

## Notas de riesgo

- La red bridged de la LAN de laboratorio está muy activa (`rx≈375k`, `arp_rx≈1500`),
  lo que valida RX bajo carga real.
- `ping`/`ps` no lanzan por el defecto NE2 anterior; por eso la validación ICMP
  `guest → gateway` no se pudo ejecutar con `ping.nxe`. La dirección
  `gateway → guest` sí queda probada (`icmp_rx/icmp_tx`).
- El serial de VBox se escribe en `/home/amartinper/rust-os/vbox_serial.log`
  (el log anterior se preservó como `vbox_serial_prev.log`).
- La VM `NeoDOS` quedó apuntando al VDI regenerado con la imagen corregida.

---

## Actualización — tras el fix del image builder (RC-4)

- `make_btree_leaf()` ya no trunca hojas en silencio (`count == serializadas`).
- `/Programs` rebalanceado (28) y `/System/Tools` (19); los 47 NLTs por idioma se
  reparten en `{lang}` + `{lang-only}` (fallback i18n existente), sin perder ninguno.
- Imagen reconstruida (100 MB) con `smp-ap-sched` como feature **por defecto** y los
  fixes de `neodev` (bloques 2560→25600, `-smp`).
- VBox (bridged, 2 vCPUs): DHCP `10.0.1.46`, `netd cpu=1`, 0 panics;
  `type C:\Programs\ping.nxe` **ya no devuelve `Archivo no encontrado`**; el shell
  muestra i18n correcto (`Comando o nombre de archivo incorrecto`) en vez de `?`.
- `ping.nxe` se lanza pero crashea en runtime (`#PF` en la ruta del allocator): defecto
  aparte de `ping`/userland, no del lookup ni de la red.
