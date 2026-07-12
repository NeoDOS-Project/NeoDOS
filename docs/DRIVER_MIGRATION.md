# Driver Migration Reference — NeoDOS v0.42

Documento de referencia sobre el estado actual de los drivers del kernel y su
relación con los drivers .nem.

## Taxonomía de Drivers

```text
┌─────────────────────────────────────────────────────────┐
│  Drivers .nem (7)     │  Kernel stubs (5)               │
│                       │                                  │
│  ps2kbd.nem  BOOT     │  ps2.rs        ← init HW        │
│  serial.nem  BOOT     │  ata.rs        ← fallback PIO   │
│  rtc.nem     BOOT     │  boot_ahci.rs  ← boot DMA       │
│  acpi.nem    SYSTEM   │  pci.rs        ← ECAM + acceso  │
│  pci.nem     SYSTEM   │  rtc_bridge.rs ← consumer       │
│  ata.nem     SYSTEM   │                                  │
│  ahci.nem    SYSTEM   │  Infraestructura (no drivers):   │
│                       │  block.rs, driver_runtime.rs,    │
│  Kernel-only (4)      │  isolation.rs, caps.rs,          │
│  ────────────────     │  boot_loader/, abi/, nem/,       │
│  nvme.rs              │  dependency/, hotreload.rs,      │
│  fat32.rs             │  mod.rs, driver_loader.rs,       │
│  gpt.rs               │  storage_manager.rs              │
│  iso9660.rs           │                                  │
│                       │  Dead code:                      │
│                       │  usb_hid/ (892L, no funcional)   │
└─────────────────────────────────────────────────────────┘
```

## 1. Drivers .nem Existentes

| # | Driver | Cat | Líneas | Hardware | Kernel stub | Entry points |
| --- | -------- | ----- | -------- | ---------- | ------------- | ------------- |
| 1 | `ps2kbd.nem` | BOOT | 268 | PS/2 keyboard: scan code → keycode, modifiers, dead keys, 2 layouts (US/SP) | `ps2.rs` (76L) | init, activate, on_event |
| 2 | `serial.nem` | BOOT | 103 | COM1 serial: 115200 8N1, FIFO, IRQ4, push bytes a input buffer | — | init, activate, on_event |
| 3 | `rtc.nem` | BOOT | 142 | CMOS RTC: BCD→bin, EVENT_RTC_READ → EVENT_RTC_DATA | `rtc_bridge.rs` (59L) | init, activate, on_event |
| 4 | `acpi.nem` | SYSTEM | 186 | ACPI power: PIIX3/4 PM1a_CNT, SLP_TYP5 shutdown, PS/2 reset | — | init, activate, on_event |
| 5 | `pci.nem` | SYSTEM | 407 | PCI bus enumerator: bus 0-255, bridge traversal, class/subclass, MSI caps | `pci.rs` (219L) | init, activate, on_event |
| 6 | `ata.nem` | SYSTEM | 616 | ATA full: IDE controller, DMA+PIO, primary+secondary, hst_register_block_device | `ata.rs` (133L) | init, activate, on_event |
| 7 | `ahci.nem` | SYSTEM | 794 | AHCI full: HBA init, multi-port, ATA+ATAPI, PRDT, SCTL reset | `boot_ahci.rs` (532L) | init, activate, on_event |

**Total .nem**: 2,516 líneas en 7 drivers.

## 2. Kernel Stubs (existen como .nem también)

Estos son drivers que tienen una versión mínima en el kernel para el arranque
temprano y una versión completa en .nem:

| Kernel stub | Líneas | .nem | Líneas | Relación |
| ------------- | -------- | ------ | -------- | ---------- |
| `ata.rs` | 133 | `ata.nem` | 616 | Stub PIO solo canal primario. .nem añade DMA + secundario |
| `boot_ahci.rs` | 532 | `ahci.nem` | 794 | Stub single-port DMA polling. .nem multi-port + ATAPI |
| `ps2.rs` | 76 | `ps2kbd.nem` | 268 | Stub = init HW (controlador). .nem = traducción scan codes |
| `rtc_bridge.rs` | 59 | `rtc.nem` | 142 | Stub = consumer del event bus. .nem = lector CMOS |
| `pci.rs` | 219 | `pci.nem` | 407 | Stub = ECAM + acceso config. .nem = enumeración de buses |

**Principio arquitectónico**: Los stubs del kernel son **deliberadamente
mínimos** y solo existen porque:

1. Se necesitan antes de que el boot loader de .nem se ejecute (Phase 3 vs 3.85)
2. Proveen primitivas de bajo nivel (ECAM, port I/O) que los .nem consumen vía `hst_*`
3. Sirven como fallback si el .nem equivalente falla (ATA PIO boot stub)

## 3. Drivers Solo en Kernel (sin .nem)

| Driver | Líneas | Boot-critical | ¿Migrable a .nem? | Razón |
| -------- | -------- | --------------- | ------------------- | ------- |
| `nvme.rs` | 837 | Sí (Phase 3) | No | 837 líneas, acoplado a PCI/MSI-X/PRP/MMIO. Migrar requeriría exponer demasiado del kernel vía hst_*. Sin beneficio. |
| `fat32.rs` | 523 | Sí (Phase 3) | No | Filesystem driver. El modelo NEM es para drivers de hardware, no filesystems. Además se necesita en Phase 3 antes del boot loader .nem. |
| `gpt.rs` | 178 | Sí (Phase 3) | No | Parser de particiones. Corre en Phase 3 antes que cualquier .nem. Sin él no se encuentran las particiones. |
| `iso9660.rs` | 352 | No | Teóricamente | Si existiera un modelo de filesystem drivers pluggable (tipo FSD en NT), podría ser .nem. Pero no es el modelo actual ni una prioridad. |
| `storage_manager.rs` | 38 | Sí (Phase 3) | No | Solo 38 líneas de orquestación. Sin sentido convertirlo a driver. |
| `usb_hid/` | 892 | No | Sí, si se revive | Código muerto (UHCI no funcional en PIIX3). Si se hiciera funcional (XHCI?), sería un candidato claro a BOOT .nem como `ps2kbd`. |

## 4. Infraestructura (no son drivers)

Estos módulos **no deben migrarse** porque son la infraestructura que sostiene
el ecosistema .nem:

| Módulo | Líneas | Rol |
| -------- | -------- | ----- |
| `block.rs` | 394 | Define el trait `BlockDevice`. Es la interfaz, no una implementación. |
| `driver_runtime.rs` | 944 | Máquina de estados del ciclo de vida de drivers .nem (8 estados) |
| `isolation.rs` | 795 | Sandbox de aislamiento para drivers .nem (16 MB, 16 slots) |
| `caps.rs` | 350 | Sistema de capacidades (12 flags), política de seguridad para .nem |
| `boot_loader/mod.rs` | 375 | Orquestador que carga .nem en Phase 3.85 |
| ~~`driver_loader.rs`~~ | ~~128~~ | ~~Cargador legacy manual~~ **Eliminado en v0.46.2** |
| `abi/` | — | Negociación de versión ABI kernel↔driver |
| `dependency/` | — | Grafo de dependencias y orden topológico para carga de .nem |
| `nem/` | — | Parser de formato NEM v3, v3loader, tabla de exports del kernel |
| `hotreload.rs` | 678 | Infraestructura de hot reload (unload/reload graceful) |
| `mod.rs` | 61 | Raíz del módulo + DeviceEvent flags |

## 5. Dead Code

| Módulo | Líneas | Estado | Potencial |
|--------|--------|--------|-----------|
| `usb_hid/` (3 archivos) | 892 | **No funcional** — PIIX3 no acepta escrituras FLBASEADD | Si se implementara XHCI o se arreglara UHCI, sería un driver BOOT .nem (equivalente a ps2kbd para USB) |

## 6. Mapa de Dependencias en Boot

```text
Phase 2:   ps2.rs (init controlador teclado)
Phase 2.3: pci.rs (ECAM init)
Phase 3:   storage_manager.rs → nvme.rs | boot_ahci.rs | ata.rs
Phase 3:   gpt.rs (descubre particiones)
Phase 3:   fat32.rs (monta ESP como A:)
Phase 3.85: boot_loader/ → carga todos los .nem (BOOT → SYSTEM)
```

## 7. Resumen

| Categoría | Count | Total líneas |
| ----------- | ------- | ------------- |
| Drivers .nem | 7 | 2,516 |
| Kernel stubs (con .nem) | 5 | 1,019 |
| Kernel-only drivers | 6 | 2,820 |
| Infraestructura .nem | 11 módulos | ~3,729 |
| Dead code | 1 | 892 |
| **Total** | | **~10,976** |

**Conclusión**: El ecosistema actual está bien balanceado. 7 drivers .nem cubren
todo el hardware esencial (teclado, serial, RTC, ACPI, PCI, ATA, AHCI). Los 5
stubs del kernel son mínimos y necesarios para el arranque. Los únicos drivers
que permanecen en kernel sin equivalente .nem (NVMe, FAT32, GPT, ISO9660) lo
hacen por razones arquitectónicas sólidas.

No hay drivers kernel que requieran migración urgente a .nem. El dead code de
USB HID es la única oportunidad clara de nuevo driver .nem, pero depende de
resolver el problema UHCI/XHCI subyacente.
