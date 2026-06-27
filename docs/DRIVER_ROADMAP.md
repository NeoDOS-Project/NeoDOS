# Driver Migration Roadmap — NeoDOS v0.42 → v1.0

Plan de migración, mejora y nuevos drivers .nem alineado con
[ARCHITECTURAL_VISION.md](ARCHITECTURAL_VISION.md).

## Fase 1: Consolidación (v0.43–v0.45) — Sin nuevos drivers

**Objetivo**: Estabilizar la infraestructura .nem existente antes de añadir nuevos drivers.

### 1.1 Auditoría de ABI del Kernel Export Table

- [ ] Revisar todas las funciones `hst_*` expuestas a drivers .nem
- [ ] Documentar qué `hst_*` usa cada driver .nem
- [ ] Detectar funciones `hst_*` no usadas por ningún driver (candidatas a deprecación)
- [ ] Detectar necesidades de `hst_*` no cubiertas que los drivers .nem resuelven con workarounds

**Drivers afectados**: Todos los 7 .nem existentes.

### 1.2 Normalizar Categorías de Drivers

Verificar que cada driver tiene la categoría correcta:

| Driver | Categoría actual | ¿Correcta? | Nota |
|--------|-----------------|------------|------|
| `ps2kbd.nem` | BOOT | ✅ | Necesario para input en boot |
| `serial.nem` | BOOT | ✅ | Log de arranque vía serial |
| `rtc.nem` | BOOT | ✅ | Fecha/hora disponible en boot |
| `acpi.nem` | SYSTEM | ⚠️ | ¿Debería ser BOOT? Se usa en shutdown que puede ocurrir antes de cargar SYSTEM |
| `pci.nem` | SYSTEM | ✅ | Enumeración post-boot |
| `ata.nem` | SYSTEM | ✅ | Reemplaza boot stub |
| `ahci.nem` | SYSTEM | ✅ | Reemplaza boot stub |

**Acción**: Evaluar si `acpi.nem` debe subir a BOOT. Si el sistema hace poweroff
antes de Phase 3.85, no hay driver ACPI cargado. El shutdown actual usa QEMU
debug port + PS/2 reset como fallback, así que no es crítico.

### 1.3 Eliminar `driver_loader.rs` Legacy ✅

- [x] Migrar comandos `LOADNEM`/`UNLOADNEM`/`NEMLIST` a Ring 3 (.NXE) — `loadnem.nxe` + `ndreg.nxe`
- [x] Mover la lógica de carga manual al `boot_loader/` o un nuevo `driver_manager` — ya en `nem/loader.rs` + `boot_loader/` + `hotreload.rs`
- [x] Eliminar `driver_loader.rs` (128 líneas) del kernel

**Completado en v0.46.2**: `driver_loader.rs` eliminado. LOADNEM/UNLOADNEM via
`loadnem.nxe` (Ring 3, `ob_create(Driver)`/`sys_driver_unload`). NEMLIST via
`ndreg.nxe LIST` (Ring 3, `ob_query_info(Drivers)`).

### 1.4 Tests de Integración para Drivers .nem

- [ ] Añadir tests que verifiquen que cada .nem pasa el pipeline de certificación
- [ ] Test de que todos los BOOT drivers son obligatorios (el sistema no arranca sin ellos)
- [ ] Test de hot reload para cada driver que lo soporte

---

## Fase 2: Mejora de Drivers Existentes (v0.47–v0.50)

### 2.1 `ps2kbd.nem` — Soporte para Mouse PS/2

- [ ] Añadir init de puerto 2 del controlador PS/2
- [ ] Handler de IRQ12 (mouse)
- [ ] Protocolo PS/2 mouse (3-byte packets, movimiento + botones)
- [ ] Nuevo tipo de evento: `EVENT_MOUSE_INPUT`

**Complejidad**: Media (~150 líneas nuevas). El controlador ya está inicializado
por `ps2.rs`, solo falta añadir IRQ12 y el protocolo.

### 2.2 `ahci.nem` — Modo IRQ (interrupciones)

Actualmente usa DMA polling. Migrar a interrupciones MSI:

- [ ] Detectar soporte MSI en el controlador AHCI
- [ ] Configurar vector MSI para cada port
- [ ] Handler de IRQ que complete IRPs pendientes
- [ ] Mantener polling como fallback

**Complejidad**: Alta (~300 líneas). Requiere coordinar con el subsistema MSI-X
del kernel.

### 2.3 `pci.nem` — Soporte ECAM

~~Actualmente usa legacy PIO (0xCF8/0xCFC). Migrar a ECAM:~~

- [x] Recibir la dirección ECAM del kernel (ya configurada en Phase 2.3)
- [x] Usar `hst_*` MMIO para leer config space vía ECAM (`hst_ecam_read_dword` / `hst_ecam_write_dword`)
- [x] Mayor velocidad de enumeración (MMIO en lugar de PIO)

**Completado en v0.42.0** — 3 nuevas exportaciones `hst_ecam_*`, ECAM detectado en `driver_init()`, fallback PIO transparente. QEMU con `-machine q35` para ECAM real.

### 2.4 `ata.nem` — Soporte ATAPI

- [ ] Detectar dispositivos ATAPI en los canales
- [ ] Implementar PACKET command (0xA0)
- [ ] READ_10 CDB para lectura de CD/DVD
- [ ] Registrar como block device con sector size 2048

**Complejidad**: Alta (~400 líneas). El driver AHCI ya tiene soporte ATAPI como
referencia.

---

## Fase 3: Nuevos Drivers .nem (v0.50–v0.60)

### 3.1 `framebuffer.nem` — Driver de Framebuffer

**Motivación**: El framebuffer se configura en el bootloader y se pasa al kernel
vía `BootInfo`. Un driver .nem permitiría:
- Cambio de modo de video en runtime
- Aceleración 2D básica (blit, fill)
- Multi-framebuffer (varias ventanas)
- Desacoplar la consola del framebuffer físico

**Dependencias**: PCI (detectar GPU), `hst_map_mmio` para mapear BARs de GPU.

**Complejidad**: Muy alta (~600 líneas). Requiere nuevos `hst_*` para mapeo MMIO
de GPU y acceso a PCI BARs desde drivers.

### 3.2 `xhci.nem` — Driver USB (XHCI)

**Motivación**: Sustituir el dead code `usb_hid/`. Soporte para teclados/ratones
USB modernos.

- [ ] Reemplazar `usb_hid/` con enfoque XHCI (no UHCI, que no funciona en PIIX3)
- [ ] XHCI init: mapear MMIO, reset controller, configurar command ring
- [ ] Event ring para interrupciones
- [ ] USB HID: teclado + ratón
- [ ] Emitir `EVENT_KEYBOARD_INPUT` y `EVENT_MOUSE_INPUT` para integrar con input

**Dependencias**: PCI (detectar XHCI), MSI-X, `hst_map_mmio`.

**Complejidad**: Extrema (~1500+ líneas). Es el driver más complejo de todos.
XHCI es un controlador complicado (spec de ~600 páginas). Probablemente
necesitará ser SYSTEM o incluso dividirse en múltiples .nem (uno para el
controlador, otro para HID).

### 3.3 `networking.nem` — Driver de Red (virtio-net / e1000)

**Motivación**: Habilitar TCP/IP stack (v0.47 en ARCHITECTURAL_VISION.md).

- [ ] Detectar NIC vía PCI (virtio-net o e1000 como primera opción)
- [ ] Inicializar colas de RX/TX
- [ ] Interrupciones MSI-X
- [ ] Registrar como dispositivo de red en el kernel
- [ ] Emitir `EVENT_NETWORK_PACKET` con paquetes recibidos

**Dependencias**: PCI, MSI-X, `hst_map_mmio`, `hst_alloc_page` (DMA buffers).

**Complejidad**: Muy alta (~800 líneas). Requiere un subsistema de red en el
kernel que reciba paquetes del driver.

---

## Fase 4: Drivers de Filesystem como .nem (v0.70–v1.0)

Esta fase depende de que exista un modelo de **pluggable filesystem drivers**
en el VFS (tipo FSD en Windows NT o VFS en Linux).

### 4.1 `neodos-fs.nem` — NeoDOS FS como driver

Actualmente `neodos_fs.rs` está en el kernel (~1,200 líneas). Si se externaliza:
- El kernel solo necesita un mini-stub para leer el FS raíz en boot
- El driver completo se carga como SYSTEM

**Beneficio**: Separación limpia, hot reload del FS driver.

### 4.2 `fat32.nem` — FAT32 como driver

Ídem. Actualmente `fat32.rs` en kernel (523 líneas).

### 4.3 `iso9660.nem` — ISO 9660 como driver

Actualmente `iso9660.rs` (352 líneas) no se usa. Externalizar como .nem lo haría
disponible para montaje de CDs en runtime.

---

## Resumen del Roadmap

```
v0.42 ─── v0.45 ─── v0.47 ─── v0.50 ─── v0.60 ─── v1.0
  │          │          │          │          │          │
  │  Fase 1  │  Fase 2  │  Fase 3  │  Fase 3  │  Fase 4  │
  │  ─────   │  ─────   │  ─────   │  ─────   │  ─────   │
  │  ABI     │  ps2kbd  │  fb.nem  │  xhci    │  FS .nem │
  │  audit   │  mouse    │          │  .nem    │  plugins │
  │  cats    │  ahci     │  net     │          │          │
  │  legacy  │  IRQ      │  .nem    │          │          │
  │  tests   │  pci ECAM │          │          │          │
  │          │  ata ATAPI│          │          │          │
```

## Drivers Futuros (no priorizados)

| Driver | Propósito | Dependencias |
|--------|-----------|-------------|
| `nvme.nem` | NVMe como .nem | Demasiado acoplado al kernel. Baja prioridad. |
| `sound.nem` | Audio (Intel HDA / AC97) | PCI, MMIO, interrupciones |
| `gpu.nem` | GPU básica (modo VESA/VGA) | Framebuffer .nem |
| `smbus.nem` | SMBus / I2C | PCI, ACPI |

## Métricas de Éxito por Fase

| Fase | KPI |
|------|-----|
| Fase 1 | 0 regresiones en tests, ABI documentado |
| Fase 2 | +4 features en drivers existentes, tests nuevos pasando |
| Fase 3 | +2/3 drivers nuevos funcionales |
| Fase 4 | Al menos 1 FS driver externalizado como .nem |
