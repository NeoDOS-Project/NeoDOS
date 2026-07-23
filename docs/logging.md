# Sistema de Logging de NeoDOS

Source: `src/log/mod.rs`, `neodos-kernel/build.rs`. Sistema centralizado de registro de
eventos con filtrado por nivel y subsistema, que sustituye los `serial_println!`
dispersos por macros tipificadas con umbrales configurables en tiempo de compilacion
y en tiempo de ejecucion.

---

## Objetivos

1. **Centralizar** todo el logging del kernel en una unica infraestructura.
2. **Tipificar** cada mensaje con nivel de severidad y subsistema de origen.
3. **Filtrar** por nivel y subsistema en compilacion (para eliminar codigo muerto en
   release) y en runtime (para depuracion selectiva).
4. **Eliminar** la dependencia de `serial_println!()` en codigo de subsistemas.
5. **Extensibilidad**: anadir un subsistema o un backend sin modificar el nucleo.

---

## Arquitectura general

```
  ┌─────────────────────────────────────────────────────────┐
  │  Macros publicas                                       │
  │  kerror!  kwarn!  kinfo!  kdebug!  ktrace!  klog_raw! │
  └───────────┬─────────────────────────────────────────────┘
              │ cada macro evalua log_enabled(subsys, level)
              ▼
  ┌─────────────────────────────────────────────────────────┐
  │  LogSubsys        → tag, idx                           │
  │  LogLevel         → Error|Warn|Info|Debug|Trace        │
  │  COMPILE_TIME_LEVELS[idx]  ← build.rs + env vars       │
  │  RUNTIME_LEVELS[idx]       ← AtomicU8 (0xFF = default) │
  └───────────┬─────────────────────────────────────────────┘
              │ si habilitado: _log / _log_simple / _log_raw
              ▼
  ┌─────────────────────────────────────────────────────────┐
  │  Backend actual: serial_print!  → COM1 (0x3F8)         │
  │  Backends futuros: console, ring buffer, file, network │
  └─────────────────────────────────────────────────────────┘
```

El modulo `log` se declara con `#[macro_use] pub mod log;` al inicio de `main.rs`,
antes que cualquier otro modulo, para que las macros esten disponibles en toda la
crate sin import explicito.

---

## Flujo de un mensaje de log

```
kinfo!(LogSubsys::Net, "NIC {} inicializada", nic_id);

1. La macro kinfo! expande a:
   if log_enabled(LogSubsys::Net, LogLevel::Info) {
       _log_simple(LogSubsys::Net, format_args!("NIC {} inicializada", nic_id));
   }

2. log_enabled() consulta:
   - RUNTIME_LEVELS[Net.idx] → si != 0xFF, usa el valor runtime
   - Si == 0xFF, usa COMPILE_TIME_LEVELS[Net.idx]

3. Si el nivel es suficiente, _log_simple() llama a:
   serial_print!("[NET] NIC 0 inicializada\r\n");

4. serial_print! → _print() → SERIAL1.lock().write_fmt() → outb(0x3F8, byte) × N
```

---

## Niveles de log

| Nivel   | Valor | Significado                                                          |
|---------|-------|----------------------------------------------------------------------|
| ERROR   | 0     | Fallo irrecuperable. El sistema no puede continuar.                  |
| WARN    | 1     | Condicion anomala. El sistema puede continuar degradado.             |
| INFO    | 2     | Hito significativo: inicializacion, descubrimiento, cambio de estado.|
| DEBUG   | 3     | Detalle util para depuracion: valores de registros, rutas de codigo. |
| TRACE   | 4     | Traza de muy bajo nivel: cada paquete, cada IO, cada page fault.     |

### ERROR

- **Proposito:** Condicion fatal que impide continuar. Suele preceder a `halt()` o panic.
- **Ejemplos:**
  ```rust
  kerror!(LogSubsys::Kernel, "Failed to read superblock: {:?}", e);
  kerror!(LogSubsys::Watchdog, "Timeout before storage init");
  kerror!(LogSubsys::Boot, "Failed to allocate stack for AP {}", cpu);
  ```
- **Cuando usarlo:** Fallos de hardware, datos corruptos, out-of-memory critico.
- **Cuando NO:** Recuperable (usar WARN). Esperable (usar INFO). Debug (usar DEBUG).

### WARN

- **Proposito:** Condicion anomala recuperable. El sistema sigue pero en estado degradado.
- **Ejemplos:**
  ```rust
  kwarn!(LogSubsys::Kernel, "Version mismatch: bootloader v{:x}, kernel v{:x}");
  kwarn!(LogSubsys::Boot, "No APs detected (single CPU mode)");
  kwarn!(LogSubsys::Driver, "Driver {} faulted, continuing without it", name);
  ```
- **Cuando usarlo:** Fallback a un camino menos optimo, timeout recuperable, recurso no encontrado con alternativa.
- **Cuando NO:** Fatal (ERROR). Normal (INFO). Detalle (DEBUG).

### INFO

- **Proposito:** Hitos normales de operacion. Inicializacion, descubrimiento, cambios de estado.
- **Ejemplos:**
  ```rust
  kinfo!(LogSubsys::Net, "Networking initialized ({} NIC(s))", nic_count);
  kinfo!(LogSubsys::Driver, "=== Driver Manager v1.0 ===");
  kinfo!(LogSubsys::Nvme, "Ready: {} sectors x {}B", nsze, block_size);
  ```
- **Cuando usarlo:** Inicio/fin de fase, dispositivo detectado, servicio iniciado, cambio de estado significativo.
- **Cuando NO:** Cada paquete o IO (TRACE). Valores de registro (DEBUG). Recuperable (WARN).

### DEBUG

- **Proposito:** Decisiones de rutas de codigo, transiciones de estado, configuraciones HW.
- **Ejemplos:**
  ```rust
  kdebug!(LogSubsys::Sched, "ctx switch: {} -> {}", current.tid, next.tid);
  kdebug!(LogSubsys::Memory, "Split 2MB page @ 0x{:x}", virt);
  kdebug!(LogSubsys::Boot, "Sending SIPI (vector=0x{:x})...", sipi_vector);
  kdebug!(LogSubsys::Interrupts, "Spurious interrupt on vector {}", vector);
  ```
- **Cuando usarlo:** Valores de registros HW, decisiones de routing, contenido de estructuras, transiciones de estado internas, timeouts notificados.
- **Cuando NO:** Produccion con `LOG_DEFAULT=WARN` (se elimina). Hitos de usuario (INFO). Per-paquete (TRACE).

### TRACE

- **Proposito:** Maximo nivel de detalle. Cada evento atomico de bajo nivel. Solo se activa
  para depuracion profunda de un subsistema concreto.
- **Ejemplos reales en el codigo actual:**
  ```rust
  // Scheduler: entrada a la funcion de planificacion
  ktrace!(LogSubsys::Sched, "schedule entry");

  // Red: cada tick del kernel thread de red
  ktrace!(LogSubsys::Net, "tick");

  // Red: cada paquete Ethernet recibido
  ktrace!(LogSubsys::Net, "RX {} bytes, src={} dst={} type=0x{:04x}", len, src, dst, etype);

  // ARP: cada entrada de cache
  ktrace!(LogSubsys::Arp, "Cache insert: {} -> {}", ip, mac);

  // ICMP: cada echo request enviado y reply recibido
  ktrace!(LogSubsys::Icmp, "Echo Request id={} seq={} {} -> {}", id, seq, src, dst);
  ktrace!(LogSubsys::Icmp, "Echo Reply id={} rtt={}us", id, rtt);

  // NVMe: cada escritura al doorbell de submission queue
  ktrace!(LogSubsys::Nvme, "SQ doorbell: qid={} tail={}", qid, tail);

  // Memoria: cada pagina demand-alloc
  ktrace!(LogSubsys::Memory, "Demand-alloc anon 4K @ 0x{:x} -> phys 0x{:x}", virt, phys);

  // Paginacion: cada split de huge page
  ktrace!(LogSubsys::Memory, "Split 2MB page @ 0x{:x}", virt);

  // Hot reload: carga/descarga individual de driver
  ktrace!(LogSubsys::Hotreload, "Driver {} ({}) transitioning to UNLOADING", name, id);
  ```
- **Cuando usarlo:** Cada paquete RX/TX, cada CQE NVMe, cada page fault, cada context switch,
  cada escritura a registro de dispositivo, cada entrada de cache.
- **Cuando NO:** Todo lo demas. TRACE produce un volumen extremo de salida. Solo activar
  para el subsistema concreto que se esta depurando.

---

## Subsistemas

Cada subsistema tiene un identificador (`LogSubsys`) con un tag de 2-6 caracteres
que aparece automaticamente en la salida entre corchetes.

| Subsistema          | Tag      | Constante                         | Env var            |
|---------------------|----------|-----------------------------------|--------------------|
| Kernel (general)    | `KERN`   | `LogSubsys::Kernel`              | `LOG_KERNEL`       |
| Scheduler           | `SCHED`  | `LogSubsys::Sched`               | `LOG_SCHED`        |
| Memory Manager      | `MEM`    | `LogSubsys::Memory`              | `LOG_MEMORY`       |
| Object Manager      | `OB`     | `LogSubsys::Object`              | `LOG_OBJECT`       |
| Driver Manager      | `DRV`    | `LogSubsys::Driver`              | `LOG_DRIVER`       |
| PCI                 | `PCI`    | `LogSubsys::Pci`                 | `LOG_PCI`          |
| VirtIO              | `VIO`    | `LogSubsys::Virtio`              | `LOG_VIRTIO`       |
| VFS                 | `VFS`    | `LogSubsys::Vfs`                 | `LOG_VFS`          |
| NeoFS / Filesystem  | `FS`     | `LogSubsys::Fs`                  | `LOG_FS`           |
| Networking (global) | `NET`    | `LogSubsys::Net`                 | `LOG_NET`          |
| DNS                 | `DNS`    | `LogSubsys::Dns`                 | `LOG_DNS`          |
| ARP                 | `ARP`    | `LogSubsys::Arp`                 | `LOG_ARP`          |
| ICMP                | `ICMP`   | `LogSubsys::Icmp`                | `LOG_ICMP`         |
| NEM Drivers         | `NEM`    | `LogSubsys::Nem`                 | `LOG_NEM`          |
| NVMe                | `NVMe`   | `LogSubsys::Nvme`                | `LOG_NVME`         |
| AHCI                | `AHCI`   | `LogSubsys::Ahci`                | `LOG_AHCI`         |
| ATA                 | `ATA`    | `LogSubsys::Ata`                 | `LOG_ATA`          |
| FAT32               | `FAT32`  | `LogSubsys::Fat32`               | `LOG_FAT32`        |
| Isolation           | `ISO`    | `LogSubsys::Isolation`           | `LOG_ISOLATION`    |
| Hot Reload          | `HOTRELOAD` | `LogSubsys::Hotreload`        | `LOG_HOTRELOAD`    |
| Storage             | `STORAGE` | `LogSubsys::Storage`            | `LOG_STORAGE`      |
| Power Manager       | `PM`     | `LogSubsys::Power`               | `LOG_POWER`        |
| Service Manager     | `SM`     | `LogSubsys::Services`            | `LOG_SERVICES`     |
| Config Manager (Cm) | `CM`     | `LogSubsys::Cm`                  | `LOG_CM`           |
| Syscall             | `SYS`    | `LogSubsys::Syscall`             | `LOG_SYSCALL`      |
| Keyboard            | `KBD`    | `LogSubsys::Kbd`                 | `LOG_KBD`          |
| Input               | `INPUT`  | `LogSubsys::Input`               | `LOG_INPUT`        |
| Timers              | `TIMER`  | `LogSubsys::Timers`              | `LOG_TIMERS`       |
| APIC                | `APIC`   | `LogSubsys::Apic`                | `LOG_APIC`         |
| HPET                | `HPET`   | `LogSubsys::Hpet`                | `LOG_HPET`         |
| IOAPIC              | `IOAPIC` | `LogSubsys::Ioapic`              | `LOG_IOAPIC`       |
| MSI                 | `MSI`    | `LogSubsys::Msi`                 | `LOG_MSI`          |
| Interrupts          | `IRQ`    | `LogSubsys::Interrupts`          | `LOG_INTERRUPTS`   |
| Exception/SEH       | `EXC`    | `LogSubsys::Exception`           | `LOG_EXCEPTION`    |
| SEH                 | `SEH`    | `LogSubsys::Seh`                 | `LOG_SEH`          |
| Security            | `SEC`    | `LogSubsys::Security`            | `LOG_SECURITY`     |
| Slab Allocator      | `SLAB`   | `LogSubsys::Slab`                | `LOG_SLAB`         |
| ELF Loader          | `ELF`    | `LogSubsys::Elf`                 | `LOG_ELF`          |
| NXL Loader          | `NXL`    | `LogSubsys::Nxl`                 | `LOG_NXL`          |
| Watchdog            | `WDT`    | `LogSubsys::Watchdog`            | `LOG_WATCHDOG`     |
| Boot                | `BOOT`   | `LogSubsys::Boot`                | `LOG_BOOT`         |
| Boot Benchmark      | `BENCH`  | `LogSubsys::Bench`               | `LOG_BENCH`        |
| Init (NeoInit)      | `INIT`   | `LogSubsys::Init`                | `LOG_INIT`         |
| User mode           | `USER`   | `LogSubsys::User`                | `LOG_USER`         |
| PS/2                | `PS2`    | `LogSubsys::Ps2`                 | `LOG_PS2`          |
| Test                | `TEST`   | `LogSubsys::Test`                | `LOG_TEST`         |

### Anadir un subsistema nuevo

1. Anadir entrada en el array `subsystems` de `build.rs`.
2. Anadir constante en `impl LogSubsys` de `log/mod.rs`.
3. Anadir entrada en `COMPILE_TIME_LEVELS`.

No se requiere modificar las macros ni la logica de filtrado.

---

## Backends

### Backend actual: Serial (COM1)

- **Archivo:** `src/arch/x64/serial.rs`
- **Puerto:** `0x3F8`, 38400 baud, 8N1.
- **Mecanismo:** `spin::Mutex<SerialPort>` con `write_fmt()` byte a byte via `outb`.
- **Limitacion:** Un solo backend. Si el mutex esta tomado (p.ej. en un handler de
  interrupcion reentrante), el log se bloquea.

### Backend: Console (framebuffer VGA)

- **Archivo:** `src/console.rs`
- El macro `println!` existe pero es independiente del sistema de logs.
- Escribe a la pantalla Y ademas llama a `serial_print!` internamente.
- **No es parte del sistema de logs** — es para salida de usuario (shell, boot
  messages visibles).

### Backends futuros (disenados, no implementados)

| Backend           | Proposito                                     |
|-------------------|-----------------------------------------------|
| Ring buffer       | Buffer circular en memoria (ya existe `trace.rs`). |
| File              | Volcado a `C:\System\Logs\kernel.log`.        |
| Network           | Envio UDP syslog a servidor remoto.           |
| Crash dump area   | Escritura directa a `0x0F00_0000` para volcado post-mortem. |

---

## API

### Macros publicas

```rust
kerror!(subsys, fmt, args...)   // ERROR: [TAG] ERROR: mensaje
kwarn!(subsys, fmt, args...)    // WARN:  [TAG] WARN: mensaje
kinfo!(subsys, fmt, args...)    // INFO:  [TAG] mensaje
kdebug!(subsys, fmt, args...)   // DEBUG: [TAG] DEBUG: mensaje
ktrace!(subsys, fmt, args...)   // TRACE: [TAG] TRACE: mensaje
klog_raw!(fmt, args...)         // Sin tag ni nivel: mensaje
```

- `subsys` debe ser una constante de `LogSubsys` (ej. `LogSubsys::Net`).
- `fmt` y `args` siguen la sintaxis de `format_args!()`.
- Las macros `kerror!`, `kwarn!`, `kdebug!` y `ktrace!` anaden `NIVEL: ` tras el tag.
- `kinfo!` omite el nivel por concision (es el caso mas frecuente).
- `klog_raw!` omite tag y nivel — util para continuar una linea empezada con
  `serial_print!`.

### Funciones publicas en `log::`

```rust
log::init()                              // Inicializa niveles runtime a default
log::set_level(subsys, LogLevel::Debug)  // Activa DEBUG para un subsistema
log::get_level(subsys) -> LogLevel       // Consulta nivel efectivo actual
log::reset_level(subsys)                 // Vuelve al default de compilacion
log::reset_all_levels()                  // Resetea todos los subsistemas
log::log_enabled(subsys, level) -> bool  // Consulta si un nivel esta activo
```

### Formato de salida

```
[KERN] ERROR: Failed to read superblock: IoError
[NET] NIC 0 inicializada
[DRV] MATCH: E1000 -> driver 'e1000'
[ARP] DEBUG: Cache insert: 192.168.1.1 -> 00:11:22:33:44:55
[NVMe] DEBUG: SQ doorbell: qid=1 tail=3
```

- `[TAG]` siempre presente (4-8 caracteres).
- `NIVEL: ` presente en ERROR, WARN, DEBUG, TRACE. Ausente en INFO.
- `\r\n` al final de cada mensaje.

---

## Configuracion

### Tiempo de compilacion (build.rs + env vars)

Cada subsistema tiene un umbral definido por una variable de entorno:

```bash
# Nivel por defecto para todos los subsistemas (default: DEBUG)
LOG_DEFAULT=DEBUG neodev build --image

# Sobrescribir subsistemas concretos
LOG_NET=TRACE LOG_DRIVER=INFO LOG_SCHED=WARN neodev build --image

# Release minimo (solo errores)
LOG_DEFAULT=ERROR neodev build --image
```

Si una variable no esta definida, hereda el valor de `LOG_DEFAULT`.
Si `LOG_DEFAULT` tampoco esta definida, el default es `DEBUG`.

El `build.rs` genera `log_config.rs` en `OUT_DIR` con constantes `BUILD_*_LEVEL`
que se incrustan en el binario. El compilador elimina como codigo muerto las
ramas `if log_enabled(...)` cuyo nivel de compilacion sea inferior al nivel de
la llamada.

Ejemplo: si compilas con `LOG_DEFAULT=WARN`, todas las llamadas a `kinfo!`,
`kdebug!` y `ktrace!` se eliminan del binario (zero cost).

### Tiempo de ejecucion

```rust
// Activar DEBUG para el driver manager durante ejecucion
log::set_level(LogSubsys::Driver, LogLevel::Debug);

// Silenciar subsistema ruidoso
log::set_level(LogSubsys::Net, LogLevel::Warn);

// Volver a defaults de compilacion
log::reset_all_levels();
```

El nivel runtime se almacena en `AtomicU8`. El valor `0xFF` significa "usar
default de compilacion". La consulta es lock-free (`Ordering::Relaxed`).

---

## Funcionalidades futuras (arquitectura preparada)

| Funcionalidad          | Estado          | Notas                                                |
|------------------------|-----------------|------------------------------------------------------|
| Filtrado por nivel     | Implementado    | Compilacion + runtime.                               |
| Filtrado por subsistema| Implementado    | 46 subsistemas independientes.                       |
| Activacion dinamica    | Implementado    | `set_level()` / `reset_level()` en runtime.          |
| Salida a fichero       | Previsto        | Requiere VFS operativo y escritura asincrona.        |
| Buffer circular        | Parcial         | `trace.rs` ya existe para eventos de scheduler/IRQ.  |
| Exportacion por red    | Previsto        | Syslog sobre UDP.                                    |
| Visor de logs          | Previsto        | Herramienta Ring 3 (`logview.nxe`).                  |
| Compresion de tags     | Previsto        | Tags de 1-2 bytes en buffer circular para ahorro.    |
| Timestamp              | Previsto        | Requiere RTC operativo en early boot.                |
| CPU/PID/TID contextual | Previsto        | `ktrace!` con prefix `[CPU0][PID1][TID2]`.           |
| Rate limiting          | Previsto        | Evitar flood de logs en bucles de polling.           |

---

## Buenas practicas

### Que SI hacer

```rust
// ✓ Usar la macro adecuada al nivel semantico
kerror!(LogSubsys::Kernel, "Failed to mount filesystem: {}", e);
kinfo!(LogSubsys::Nvme, "Ready: {} sectors x {}B", nsze, block_size);
kdebug!(LogSubsys::Memory, "Demand-alloc anon 4K @ 0x{:x} -> phys 0x{:x}", virt, phys);

// ✓ Mensajes claros, con valores relevantes, sin redundancia
kinfo!(LogSubsys::Driver, "Loading E1000 (1 device(s)) ...");

// ✓ Usar LogSubsys especifico
kdebug!(LogSubsys::Arp, "Cache insert: {} -> {}", ip, mac);
kdebug!(LogSubsys::Icmp, "Echo Request id={} seq={} {} -> {}", id, seq, src, dst);

// ✓ Para continuar una linea multi-parte sin duplicar el tag:
klog_raw!("    detail line without tag");
```

### Que NO hacer

```rust
// ✗ Nunca usar serial_println!() para depuracion permanente
serial_println!("[NET] packet received");  // INCORRECTO

// ✗ No incluir el tag manualmente (la macro lo anade)
kinfo!(LogSubsys::Net, "[NET] NIC ready");  // INCORRECTO → saldra [NET] [NET] NIC ready

// ✗ No usar kinfo! para errores
kinfo!(LogSubsys::Kernel, "Out of memory!");  // INCORRECTO → usar kerror!

// ✗ No usar kerror! para condiciones recuperables
kerror!(LogSubsys::Net, "DNS timeout, using cache");  // INCORRECTO → usar kwarn!

// ✗ Evitar logs dentro de bucles de polling de alta frecuencia
loop {
    kdebug!(LogSubsys::Net, "polling...");  // INCORRECTO → usar ktrace! o rate limiting
    network_poll_all();
}

// ✗ No loguear en handlers de interrupcion si el backend usa locks
//    (el backend serial toma un spin::Mutex, seguro en IRQ handlers
//     con interrupts deshabilitadas, pero no en NMI o contexto de IPI)
```

---

## Ejemplos por subsistema

### Driver Manager

```rust
fn load_matched_drivers(&mut self) {
    kinfo!(LogSubsys::Driver, "Phase 4: Loading matched drivers");

    for (desc, devices) in &self.matched_drivers {
        kinfo!(LogSubsys::Driver, "  Loading {} ({} device(s)) ...",
               desc.name, devices.len());

        match load_driver(desc, devices) {
            Ok(id) => kinfo!(LogSubsys::Driver, "  {} loaded (id={})", desc.name, id),
            Err(e) => kerror!(LogSubsys::Driver, "  {} FAILED: {}", desc.name, e),
        }
    }
}
```

### Scheduler

```rust
fn schedule() {
    ktrace!(LogSubsys::Sched, "schedule: prev_tid={} prev_prio={}", prev_tid, prev_prio);

    let next = pick_next_thread();
    if next.tid != current.tid {
        kdebug!(LogSubsys::Sched, "ctx switch: {} -> {}", current.tid, next.tid);
        switch_to(next);
    }
}
```

### Networking

```rust
fn net_handle_incoming_packet(nic: &mut dyn NetworkInterface, packet: &[u8]) {
    ktrace!(LogSubsys::Net, "RX {} bytes, type=0x{:04x}", packet.len(), eth_type);

    if eth_hdr.is_arp() {
        kdebug!(LogSubsys::Arp, "Request RX: sender={} target={}", sender_ip, target_ip);
        kdebug!(LogSubsys::Arp, "Reply TX: our_mac={} dst_mac={}", our_mac, dst_mac);
    }

    if let Err(e) = nic.send_packet(&reply) {
        kerror!(LogSubsys::Net, "send_packet failed: {:?}", e);
    }
}
```

---

## Estado actual (v0.50)

### Implementado

- 5 niveles: ERROR, WARN, INFO, DEBUG, TRACE con macros asociadas.
- 46 subsistemas con tags independientes.
- Filtrado en compilacion via `LOG_*` env vars + `build.rs`.
- Filtrado en runtime via `set_level()` / `reset_level()` (AtomicU8, lock-free).
- Backend serial (COM1, 38400 baud).
- Macro `klog_raw!` para salida sin tag.
- ~400 puntos de log migrados desde `serial_println!()`.
- `ktrace!` implementado en rutas calientes: scheduler, net poll, NVMe doorbell, ARP cache, ICMP echo, DNS cache, hot reload, demand paging, page splitting.

### Limitaciones actuales

- Solo backend serial. No hay salida a fichero ni red.
- Sin timestamp en los mensajes (RTC no disponible en early boot).
- Sin CPU/PID/TID contextual automatico.
- Sin rate limiting (riesgo de flood en bucles de polling con TRACE activo).
- `println!` de consola no esta integrado con el sistema de logs (es independiente).

### Previsto para v0.51+

- Backend de buffer circular compartido con `trace.rs`.
- Timestamp automatico (ticks desde boot).
- Prefix contextual (CPU/PID/TID) en modo TRACE.
- Rate limiting basico (max N mensajes/segundo por subsistema).
- Backend de fichero (`C:\System\Logs\kernel.log`).
