# Historia de NeoDOS

> **Propósito:** Este documento preserva la evolución de NeoDOS desde su inicio,
> complementando al CHANGELOG. Mientras que el CHANGELOG registra cambios por versión,
> esta historia narra el crecimiento arquitectónico del proyecto: hitos, decisiones
> de diseño, refactorizaciones y cambios de filosofía.
>
> No se registran correcciones menores ni parches.

---

## Inicio del proyecto

**Fecha:** 4 de mayo de 2026  
**Lugar:** 🌱 Girona

NeoDOS nace como un sistema operativo minimalista para x86-64 escrito en Rust,
heredero conceptual de MS-DOS pero con una arquitectura completamente moderna.
El primer commit (`94300ca`) incluye un bootloader UEFI, un kernel con GDT/IDT/PIC,
driver ATA PIO, teclado PS/2, consola VGA, un sistema de archivos propio con
superbloque e inodos, FAT32, y un shell en Ring 0 con comandos básicos (DIR, TYPE,
CD, DATE, TIME, HELP, CLS, ECHO, MEM, VOL, VER).

La versión inicial es **v0.5**, denominada *"The Rusty DOS Revival"*.

**Impacto:** Sienta las bases de todo el sistema: el boot flow UEFI→bootloader→kernel,
la tabla de particiones GPT, el sistema de archivos NeoDOS, y el shell como interfaz
principal.

---

## Grandes hitos

### 4–9 de mayo de 2026 — Primer kernel funcional

Tras los primeros commits, el kernel adquiere forma rápidamente:

- Shell con 18 comandos integrados (DIR, TYPE, CD, MD, RD, COPY, DEL, REN, DATE, TIME,
  CLS, ECHO, MEM, VOL, VER, HELP, PROMPT, SET).
- Sistema de archivos NeoDOS con inodos de 256 bytes y bloques de 4 KB.
- Controladores ATA, teclado PS/2, y RTC.
- Primeros tests con `SELFTEST` command.
- Soporte para dos layouts de teclado (Español y US) intercambiables en caliente.

**Impacto:** Demuestra que un SO funcional puede construirse desde cero en Rust
con una base de código manejable.

### 10 de mayo de 2026 — v0.8.0: Multitarea real

Primera versión con soporte real de procesos en Ring 3:

- `RUN` no bloqueante — ejecuta procesos en segundo plano.
- Slots de usuario por proceso (`user_slot`).
- `KILL` command para terminar procesos.
- Mecanismo de syscall via `INT 0x80`.

**Impacto:** Transición de un entorno DOS monoproceso a un sistema multitarea.

### 11 de mayo de 2026 — v0.9.0: ACPI y generación de imágenes

- Soporte ACPI para apagado del sistema.
- Script `create_gpt_image.py` para generar una imagen de disco unificada con GPT.
- Primeros tests de syscalls.
- Documentación `AGENTS.md` como contexto para asistentes de IA.

### 11–19 de mayo de 2026 — Drivers modulares y HAL

**v0.10.0 (11 mayo):** Arquitectura de drivers modulares. Primer paso hacia un
modelo de drivers aislados.

**v0.10.1–v0.10.2 (11 mayo):** Variable de entorno `SYSTEMDRIVE`, versionado
bootloader–kernel.

**14 de mayo:** Soporte USB HID para teclado. Refactorización del driver AHCI con
buffers por puerto y soporte ATAPI para CD/DVD.

**15–16 de mayo:** Refactorización importante del VFS con integración mejorada,
`sbrk`/`sys_brk` para gestión de heap, y comandos DEL/REN/RD.

**17 de mayo:** Mitigación del error intermitente `#GP` en `syscall iretq` mediante
context switch solo desde idle. Refactorización de drivers de FS para soportar
dispositivos de bloque (ISO9660).

**18 de mayo:** Abstracción `Platform` para desacoplar el kernel de la arquitectura.
Per-process kernel stacks para seguridad en syscalls. Infraestructura de tests de
stress y regression runner.

**19 de mayo — v0.11+v0.12:** Eliminación de todos los `unwrap()` del kernel
(13 calls reemplazados por `expect` o pattern matching). `BlockDeviceManager`
unificado para abstraer dispositivos de bloque.

**19 de mayo — HAL v0 (ABI v0.2):** Nacimiento de la **Hardware Abstraction Layer**.
14 primitivas de hardware (interrupciones, I/O, memoria, temporización) concentran
todo el `asm!()` del kernel. Eliminación del sistema de módulos NDM (legacy).

**Impacto:** HAL sienta las bases para la portabilidad. La eliminación de unwrap
mejora drásticamente la robustez.

### 20 de mayo de 2026 — v0.15: NeoFS madura

- `StorageManager` unificado para inicialización ATA/AHCI.
- Suite de validación de metadatos NeoFS (36 tests, 10 categorías).
- Visualización de permisos RWXSD en DIR.
- Historial de comandos con flechas ↑/↓ (32 entradas).
- Sistema HELP mejorado con ayuda por comando.
- **120 tests** en total.

**Decisión de diseño:** Los permisos NeoFS se representan como bits en el campo
`mode` del inodo (bits 0–4: R/W/X/S/D), coexistiendo con MODE_DIR/MODE_FILE.

### 21 de mayo de 2026 — NEM, Event Bus y ELF64

**NEM driver suite + NDREG CLI:** Framework completo para drivers NEM (NeoDOS
Module) con formato binario propio, tabla de exportación, y utilidad `NDREG`
para inspección de drivers.

**Device Model + HAL Binding Layer v0.3:** Abstracción para vincular drivers
con dispositivos hardware.

**Event Bus v1:** Sistema de enrutamiento de eventos con cola lock-free SPSC de
64 slots. 11 tipos de eventos. 9 tests. Transforma IRQs en eventos normalizados.

**ELF64 loader:** Soporte para binarios ELF64 además del formato plano `.nxe`.
7 tests de validación.

**Estabilización del ABI de syscalls (S1):** `SyscallNum` enum, `SyscallError`
enum con 16 códigos, macros de error.

**Impacto:** Los tres pilares del modelo de drivers NeoDOS —formato NEM,
Event Bus y certificación— se establecen esta semana.

### 23 de mayo de 2026 — NVMe, mmap y pipes

- Driver NVMe para almacenamiento sobre PCI Express.
- **sys_mmap/sys_munmap (A4):** Mapeo perezoso de archivos en memoria con
  demand paging. Región dedicada de 32 MB.
- **IPC/Pipes (S2):** Sistema de tuberías con 16 buffers de 4 KB, `sys_pipe`,
  `sys_dup2`, bloqueo en lectura.
- **FSCK (S5):** Utilidad de verificación de integridad del sistema de archivos
  con modo reparación.
- **Process exit cleanup (S7):** Liberación completa de recursos al salir
  (kernel stack, slots, tabla de archivos).

**Impacto:** El sistema adquiere capacidades de memoria virtual, comunicación
entre procesos y recuperación de fallos.

### 24–26 de mayo de 2026 — NEM v3 y libneodos

**24 de mayo:** NEM v3 — formato binario definitivo para drivers con header de
80 bytes, 4 secciones (text, rodata, data, bss), tabla de reubicaciones, tabla
de símbolos, y negociación ABI (min/target/max). Driver PS/2 teclado migrado a
NEM standalone. Consolidación de layouts de teclado en un driver generado.

**25 de mayo:** **libneodos** — biblioteca estándar para procesos Ring 3 en Rust.
Wrappers seguros para syscalls (`exit`, `write`, `read`, `open`, `brk`, `mmap`,
etc.), módulos IO/FS/Mem, macros `print!`/`println!`. Los binarios de usuario
se convierten en proyectos Cargo individuales.

**26 de mayo:**

- **Slab allocator (A3):** 9 clases de tamaño (8–2048 bytes), O(1) alloc/free
  mediante free list. Reemplaza al `linked_list_allocator` como `#[global_allocator]`.
- **Migración RTC a NEM v3:** Primer driver de sistema migrado a NEM standalone.
- **ABI negotiation (W1):** Formalización del contrato entre kernel y drivers NEM.
- **Driver dependency resolver (W4):** Grafo de dependencias con topological sort
  y detección de ciclos.

**Decisión de diseño:** Se elimina el Device Model v0.3 y el sistema TSR, reemplazados
completamente por el modelo NEM v3 + Event Bus + HAL ABI.

### 27 de mayo de 2026 — Handle table unificada y KOBJ

**X2. Unified Handle Table:** Reemplaza `FdEntry`/`FdTable` por un sistema de
handles unificado que abstrae archivos, pipes, dispositivos y eventos.
`sys_open` retorna un `fd` (handle index) en lugar de un `(drive<<32)|inode`.

**KOBJ v1 (Kernel Object Manager):** Sistema unificado de objetos del kernel
con 9 tipos, reference counting, y registro automático de recursos.
Precursor directo del Object Manager (Ob).

**Impacto:** Unificación de recursos del kernel bajo una sola abstracción,
simplificando la gestión de ciclo de vida.

### 28 de mayo de 2026 — Page cache, PCI NEM y ACPI poweroff

- **A5 Global Page Cache:** Caché LRU de 512 páginas de 4 KB (2 MB) para E/S
  de archivos.
- **ACPI NEM poweroff driver:** Driver standalone para apagado vía ACPI S5.
- **PCI NEM driver:** Driver standalone que escanea el bus PCI y ofrece servicio
  Event Bus a otros drivers.
- **A10 PCIe:** Enumeración completa de buses PCI Express mediante bridge traversal.
- Fix del bug de caracteres duplicados en PS/2.

### 29 de mayo de 2026 — ATA NEM y planificación prioritaria

**v0.22.0 ATA NEM standalone:** Migración del driver ATA completo a NEM standalone
con soporte DMA. El kernel conserva solo un `BootAta` PIO stub para early-boot.

**A2 Priority Scheduler:** Sistema de 4 niveles de prioridad (HIGH, ABOVE_NORMAL,
NORMAL, IDLE) con time-slicing dinámico (400/200/100/50 ticks), preemption desde
Ring 3, y aging para evitar starvation. Comando PRI, columna en PS.

**Impacto:** El scheduler deja de ser round-robin puro y adquiere capacidades
de planificación por prioridad con envejecimiento.

### 2–6 de junio de 2026 — Maduración del kernel

- **Bugfix crítico:** Corrupción de registros callee-saved en user-mode + race
  condition en sys_exit.
- **X5 Deferred work queues:** Sistema de bottom-half con dos prioridades.
- **AHCI NEM standalone:** Migración del driver AHCI a NEM v3.
- **Boot Benchmark:** Sistema de profiling de boot con precisión sub-milisegundo.
  Identifica que `hlt_once()` en AHCI alargaba el boot a ~15s → reducido a ~76ms.
- **V1 Global Page Cache (avanzado):** Reescritura con hash map O(1) + LRU doubly-linked
  list. Reducción de memoria: 512 KB vs 2 MB anteriores.
- **Event Bus v2:** Colas por prioridad (alta+normal), filtros estrictos, backpressure.

### 4 de junio de 2026 — Shared libraries (DLL/NXL)

**libneodos DLL system:** `libneodos.nxl` como biblioteca compartida cargada en
dirección fija `0x1e000000`. 8 slots de 256 KB. Tabla de exportación `AbiTable`.
`sys_loadlib` (RAX=21) para cargar NXLs adicionales.

**Impacto:** Permite compartir código entre procesos Ring 3 sin vinculación
estática, allanando el camino para una biblioteca del sistema.

### 5 de junio de 2026 — Capacidades y aislamiento

**X3 Capability System:** Control de acceso granular para drivers NEM.
64-bit bitmap por driver, 11 flags, defaults por categoría (BOOT=all,
SYSTEM=8 flags, DEMAND=3 flags). Verificación en cada `hst_*` call.

**X4 Driver Isolation Layer:** Aislamiento de memoria para drivers NEM en
16×1 MB slots @ `0x30000000`. Validación de punteros, modo sandbox.

**Multi-DLL system:** Soporte para múltiples NXLs simultáneos con `LOADLIB`
command y `libmath.nxl`.

**Hot reload system:** Descarga y recarga de drivers NEM en caliente.

**Impacto:** El modelo de drivers alcanza su madurez: formato, capacidades,
aislamiento y recarga en caliente.

### 6 de junio de 2026 — Timers avanzados y buddy allocator

**C3 HPET/APIC timers:** Sistema de temporización a 1 KHz reemplazando el PIT
de 18.2 Hz. Calibración HPET → APIC timer.

**A0 Memory Architecture Rewrite:** Buddy allocator con 11 niveles de orden
(4 KB → 4 MB), layout de memoria dinámico desde el mapa UEFI, sin límite
fijo de RAM. Manejo de handles ilimitado.

**Impacto:** El sistema de memoria se vuelve escalable y preciso.

### 7–10 de junio de 2026 — SMP, SSDT y NT-like architecture

- **A1.5 EPROCESS/KTHREAD split:** Separación de proceso y thread (modelo NT).
- **A4.2 SSDT:** Syscall dispatch table con 256 slots O(1).
- **A1.1/A1.2 Per-CPU + SMP:** Estructuras de datos por CPU, arranque SMP
  (INIT-SIPI-SIPI), colas de ejecución por CPU.
- **A1.3/A1.4 Per-CPU slab + IPI:** Slab allocator local a cada CPU con hot cache
  vía GS-segment. Infraestructura de IPI con TLB shootdown.
- **Renombrado:** `.bin` → `.nxe`, `.dll` → `.nxl`. Todos los binarios y módulos
  renombrados.
- **A2.4 IRQL framework:** Prioridad de interrupciones al estilo NT (PASSIVE_LEVEL,
  APC_LEVEL, DISPATCH_LEVEL, DIRQL).
- **A4.3 ELF address space validation:** Validación de segmentos ELF contra
  límites de usuario.
- **A2.5 DPC engine:** Deferred Procedure Calls por CPU.

**Impacto:** NeoDOS se convierte en un sistema SMP con planificación por CPU,
threads, y modelo de interrupciones NT-like.

### 11–13 de junio de 2026 — HAL raw/safe, APC y NeoInit

**v0.32.0 (11 junio):** Crash dump framework, `cpuinfo.nxe`, `sys_getcpuinfo`.

**HAL v0.4 raw/safe split (11 junio):** Todo el `asm!()` confinado a `hal/raw/`.
55 calls asm, cero fuera. Capa `hal/safe/` con tipos seguros (Msr trait) y
`hal/x64/` con superficie ABI extern "C" de 26 primitivas.

**A4.5 APC engine (12 junio):** Per-thread kernel/user APC queues, alertable wait,
IRP→APC completion. Máx 64 APCs por cola.

**v0.35.0 NeoInit (PID 1) (13 junio):** Primer proceso del sistema, lanza neoshell.
`sys_spawn` con save/restore, `sys_poweroff`, comando POWEROFF.

**Impacto:** HAL alcanza su forma definitiva. NeoInit establece el modelo de
inicio del sistema.

### 15–16 de junio de 2026 — neoshell a Ring 3

**v0.37.0 (15 junio):** **neoshell migra a Ring 3.** El shell abandona el kernel
y se ejecuta como un proceso de usuario. Nuevas syscalls A4.6. BOOT.CFG pasa
a ser configurable.

**v0.38.0 (15–16 junio):**

- `HELP.NXE` como binario Ring 3 que escanea `C:\BIN`.
- Reestructuración del sistema de archivos NeoDOS.
- `sys_get_version`, `sys_get_datetime`, `DATETIME.NXE`, `VER.NXE`.
- Global object namespace system + VFS partition management.

**Impacto:** Decisión arquitectónica fundamental: el kernel deja de ejecutar
comandos de shell. Todo comando interactivo es un binario .NXE en Ring 3.

### 20–21 de junio de 2026 — Objeto global y migración masiva

**v0.38.2 (20 junio):** CD, ECHO, MEM, VOL migrados a Ring 3. Nacen
`sys_get_meminfo`, `sys_chdir_parent`.

**v0.39.0 (20 junio):** **NT5 Object Namespace.** Sistema de nombres jerárquico
con `\Device`, `\DosDevices`, `\Global`, `\Driver`, etc. Mount points integrados.
`KOBJ.NXE` como binario Ring 3.

**v0.39.1–v0.39.2 (21 junio):** TREE, TYPE, LOAD, TEST migrados a Ring 3.
Terminal ANSI con glifos de box-drawing. Soporte `O_CREAT` para archivos.

**v0.39.5–v0.39.11 (21 junio):** Migración acelerada de comandos:
HELP, DRIVES, SET, EXIT, PS, KILL, PRI, KEYB, CALL — todos a Ring 3.
NeoDOS LSP (Language Server Protocol) para asistencia a IA.

**Impacto:** Migración masiva de funcionalidad del kernel al espacio de usuario,
reduciendo la superficie del kernel.

### 21 de junio de 2026 — NT6 Security Reference Monitor

Implementación del modelo de seguridad NT:

- **SID (Security Identifier):** Formato `S-R-I-S*`.
- **Token:** Identidad + grupos + privilegios.
- **ACL/ACE:** Listas de control de acceso con entradas deny/allow.
- **SeAccessCheck:** Algoritmo NT-compatible (deny primero, luego allow, admin bypass).
- 23 tests de seguridad.

**Impacto:** Base del modelo de seguridad que persiste hasta hoy.

### 22 de junio de 2026 — Watchdog, SEH y Object Manager

**A3.3 Watchdog subsystem + A3.4 SEH/exception dispatcher:** Manejo estructurado
de excepciones y watchdog del sistema.

**OB-001/002/003 — Object Manager base:** `ObObject`, `ObObjectTable`, `ObType`,
reference counting. Primer paso hacia la unificación de objetos.

**v0.41 — Slab\<T\>:** Contenedor de capacidad variable que combina array fijo
(para el caso común) con Vec dinámico (para overflow). Scheduler usa Vec en
lugar de array fijo. Pipes con buffers dinámicos.

**Impacto:** Se eliminan los límites fijos en los subsistemas críticos.

### 22 de junio de 2026 — KWait y ABI freeze

**v0.42.0 — Unified Wait Engine (KWait):** Abstracción única para toda espera
bloqueante con 7 variantes de `WaitReason` (PipeRead, IrpComplete, ThreadJoin,
ChildExit, Event, Timer, Alertable). Reemplaza mecanismos ad-hoc.

**ABI freeze:** Event types 0–15 congelados. Capability bits 1–2048 congelados.
IOAPIC API congelada. Validación al boot.

**HandleEntry full Ob:** Todos los tipos de handle se registran como objetos Ob
con `close()` cleanup.

**Impacto:** El kernel congela sus primeras interfaces. KWait unifica todo el
modelo de espera del sistema.

### 23–25 de junio de 2026 — Object Manager (Ob) y ASLR

**v0.44 (23 junio):** **ASLR v1** (Address Space Layout Randomization):
offset aleatorio en la base de carga ELF (PIE). Slot aleatorio para binarios.

**v0.44.1:** Ob API en libneodos. Migración de PS, KOBJ, PRI, KILL al Object
Manager. `sys_ob_wait` (RAX=65) integración con KWait.

**OB-015/018/020/025/030/031/041/046:** Namespace Ob completo. Migración de
todos los binarios de usuario a ObOpen. Eliminación de syscalls legacy (48,51,52).

**v0.44.3 (26 junio):** **Input Subsystem Redesign + Virtual Terminals.**
Sistema de terminales virtuales con VtManager, `\Device\Vt0`..`\Vt3`.
console.nxl como biblioteca compartida. neoshell refactorizado.

**v0.44.4 (26 junio):** Corrección de 3 bugs SMP-unsafe (`WAIT_PID` static mut,
`ISOLATED_REGIONS` static mut, `NXL_REGISTRY` static mut → `AtomicU32`/`Mutex`).

**26 de junio — ABI v7 cleanup:** `ObInfoClass`/`ObSetInfoClass` completados.
Thread Object (OBF-03..06b). `neotop.nxe`. libmath modularizado.

**Impacto:** Ob se convierte en la abstracción central del sistema. Todas las
syscalls nuevas deben ser `sys_ob_*`.

### 27 de junio de 2026 — Timer, Semaphore, Section objects

**v0.46 — Fase 2 Objectification:** Timer, Semaphore y Section Objects.
6 syscalls legacy eliminadas del SSDT.

**v0.46.2 — AHCI NCQ + NeoMem v0.1:** Comandos nativos AHCI (Native Command
Queuing). `driver_loader` eliminado del kernel (LOADNEM/UNLOADNEM pasan a Ring 3).
syscall cleanup.

**v0.46.7:** Auditoría de estabilidad. 7 bugs corregidos: handle leaks, fd leaks,
slab double-free, rdtsc workaround para QEMU TCG.

**v0.46.8:** Bugfix OB-046 (process lifecycle — cleanup_terminated_process no
destruye hijo prematuro).

### 28 de junio de 2026 — Networking TCP/IP

**v0.47.0 — Networking:** Pila TCP/IP completa en el kernel:

- Driver e1000 NEM para NIC Intel.
- Capas: Ethernet, ARP (64 entradas, timeout 300s), IPv4, ICMP, UDP, TCP
  (3-way handshake, sliding window 16 KB, FIN/RST).
- `\Device\Tcp` y `\Device\Udp` como objetos de dispositivo en el namespace NT.
- Soporte TAP networking con fallback SLiRP.
- VirtualBox bridged networking.
- 17 tests de red.

**Impacto:** NeoDOS sale al mundo. Adquiere capacidades de red completas.

### 30 de junio de 2026 — SAM y Registry

**USR-001 SAM database:** Base de datos de cuentas de usuario con 64 entradas
de prueba, formato de serialización, y soporte de grupo Administradores.

**USR-002 Token NT extendido:** Token con grupos, privilegios, session_id.

**B2.1 Registry hive (Cm):** Sistema de registro tipo Windows Registry con
celdas (cells), hive en memoria, y 10 syscalls (RAX 67–76) para creación,
lectura, escritura y enumeración de claves y valores. 8 tests.

**VFS-1.1 MountManager unificado:** API única `mount()`/`unmount()` que
sincroniza `Vfs.drives` + Ob MountPoint + DosDevices.

**Decisión de diseño:** El Registry sigue el modelo NT Cm (Configuration Manager)
con hives cell-based, paths separados por `\`, y 6 tipos de valor (SZ, DWORD,
BINARY, MULTI_SZ, EXPAND_SZ, QWORD).

### 11–12 de julio de 2026 — NeoKBD, ACPI Power Management y kbdcompile

**NeoKBD (Keyboard Manager):** Nuevo subsistema de kernel `src/kbd/` que reemplaza
la lógica de traducción de scancodes del driver PS/2. Proporciona:

- `ObType::KeyboardDevice = 22` en `\Device\Keyboard`.
- 3 nuevos `ObInfoClass` (35=KeyboardInfo, 36=KeyboardCaps, 37=KeyboardLayouts).
- 5 nuevos `ObSetInfoClass` (43=KeyboardSetLayout, 44=KeyboardSetRepeatDelay,
  45=KeyboardSetRepeatRate, 46=KeyboardSetLeds, 47=KeyboardSetModifier).
- Carga dinámica de layouts `.kbd` desde `C:\System\Keyboard\`.
- Motor de composición de teclas muertas con tablas de compose.
- Hotkey dispatch (Ctrl+Alt+Del → poweroff, Alt+F1-F8 → VT switch) — reemplaza
  checks hardcodeados en `idt.rs`.
- 5 nuevos eventos Event Bus: `EVENT_KEYDOWN=27`, `EVENT_KEYUP=28`,
  `EVENT_KEY_CHAR=29`, `EVENT_KBD_MODIFIER=30`, `EVENT_KBD_REPEAT=31`.
- Registry-backed config: Layout, RepeatDelay, RepeatRate, NumLockOnBoot,
  CapsLockOnBoot en `\Registry\Machine\System\Keyboard`.
- `libneodos/src/keyboard.rs`: API user-level para control de teclado.

**ACPI Power Management (`src/power/acpi.rs`):** Implementación completa de:

- RSDP discovery (EBDA, BIOS areas, bootloader pointer).
- RSDT/XSDT parsing → FADT extraction.
- S5 sleep (soft-off) via PM1a/b control registers.
- Reset register support (IO/MMIO).
- Integración en HAL: `poweroff()` intenta ACPI S5 primero, luego QEMU debug
  ports, luego PS/2. `reboot()` intenta ACPI reset register, luego 0xCF9, luego PS/2.
- 7 tests de ACPI power management.

**ps2kbd NEM driver simplificado:** Eliminada la lógica de traducción de layouts
(~150 líneas), ahora emite scancodes raw. NeoKBD hace la traducción.

**kbdcompile (`tools/kbdcompile/`):** Herramienta que convierte layouts `.klc`
(Microsoft KLC format) a `.kbd` binario. Compila US y Spanish.

**neokey (`userbin/neokey/`):** Nueva utilidad Ring 3 que reemplaza a `keyb.nxe`.
Comandos: `NEOKEY show`, `NEOKEY layout <name>`, `NEOKEY layouts`,
`NEOKEY repeat <cps>`, `NEOKEY delay <ms>`, `NEOKEY leds`.

### 1–5 de julio de 2026 — VirtIO, Registry persistente, networking userland

**A5.2 VirtIO Block driver:** Primer driver VirtIO para dispositivos de bloque.
Detección PCI legacy I/O y modern MMIO. Virtqueue split vring de 256 entradas.
Prioridad de almacenamiento: NVMe > VirtIO > AHCI > ATA.

**B2.7 Registry disk persistence:** El hive se persiste en disco en
`C:\System\Config\SAM` y `C:\System\Config\SYSTEM`. Carga al boot, salvado
periódico.

**NET-1.5–1.15 (5 julio):** Networking userland completa:

- `libneodos` wrappers SOCKET.
- `net.nxl` biblioteca de red para usuario.
- `netcfg.nxe` servicio de red con DHCP/APIPA.
- `ipconfig.nxe` herramienta de información de red.
- Registro de configuración de red en Registry.

**B4.10 NeoInit registry-driven:** NeoInit lee su configuración del Registry
(DefaultShell, AutoStartServices, EnableVT, WaitForNetwork).

**Auditoría arquitectónica (AUDIT-1..10):** Corrección de 10 inconsistencias
entre código y documentación. Sincronización de enums libneodos-kernel.

---

## Evolución de la filosofía del sistema

### Fase 1: DOS Revival (v0.5 – v0.9)

NeoDOS nace como un "DOS moderno" con sabor a retro. Shell en Ring 0, comandos
tipo DOS (DIR, COPY, DEL, REN), sistema de archivos propio, pantalla negra con
letras verdes. La prioridad es tener algo funcionando.

### Fase 2: Kernel multiproceso (v0.10 – v0.15)

El sistema adquiere capacidades de sistema operativo moderno: procesos en Ring 3,
syscalls, gestión de memoria dinámica. La arquitectura de drivers comienza a
tomar forma con el primer intento (NDM) que luego se descarta.

### Fase 3: Arquitectura de drivers madura (v0.16 – v0.24)

NEM v3, Event Bus, HAL, certificación, capacidades, aislamiento. El modelo de
drivers se estabiliza y se convierte en la seña de identidad de NeoDOS.
Decisión clave: los drivers no son kernel ni user-mode, son un **tercer espacio**.

### Fase 4: NT-like y Object Manager (v0.32 – v0.44)

Migración masiva a Ring 3. SMP, IRQL, SSDT, Security Reference Monitor, Ob.
NeoDOS abandona su herencia DOS y adopta una arquitectura NT-like con Object
Manager como abstracción central. El shell se convierte en un proceso de usuario más.

### Fase 5: Expansión (v0.46 – presente)

Networking TCP/IP, Registry persistente, VirtIO, SAM. El sistema se expande
horizontalmente añadiendo subsistemas completos mientras mantiene la coherencia
arquitectónica alrededor del Object Manager.

---

## Decisiones de diseño relevantes

| Decisión | Fecha | Contexto |
| ---------- | ------- | ---------- |
| Rust como único lenguaje | May 2026 | Todo el código, incluyendo drivers, en Rust. Sin C. |
| INT 0x80 para syscalls | May 2026 | Elección deliberada sobre `syscall`/`sysret` por simplicidad del trampoline. |
| HAL raw/safe split | Jun 2026 | Todo `asm!()` confinado a `hal/raw/`. Capa segura encima. |
| Procesos estilo NT (no fork) | May 2026 | Procesos creados por `sys_spawn`. Threads por `sys_thread_create`. Sin fork. |
| Object Manager como abstracción central | Jun 2026 | Todo recurso del sistema es un objeto Ob. |
| NEM como tercer espacio | May 2026 | Drivers no son kernel ni user-mode. Pipeline de certificación de 7 estados. |
| Driver isolation (X4) | Jun 2026 | Aislamiento de memoria a 1 MB slots. Validación de punteros en cada `hst_*`. |
| Shell en Ring 3 | Jun 2026 | El kernel no ejecuta comandos. Todos los comandos son .NXE. |
| Migration Ob de syscalls legacy | Jun 2026 | Todas las syscalls nuevas (RAX ≥ 60) deben ser `sys_ob_*`. |
| Registry persistente cell-based | Jun 2026 | Modelo NT Cm con hives en disco. |
| ABI freeze progresivo | Jun 2026 | Interfaces se congelan por versión: eventos, capacidades, IOAPIC en v0.42. |

---

## Cronología resumida

| Fecha | Hito | Versión |
| ------- | ------ | --------- |
| 2026-05-04 | Primer commit: bootloader + kernel | v0.5 |
| 2026-05-06 | "The Rusty DOS Revival" | v0.6 |
| 2026-05-09 | Tests, FAT32, RTC, layouts teclado | v0.7 |
| 2026-05-10 | Multitarea: RUN no bloqueante, KILL | v0.8 |
| 2026-05-11 | ACPI, syscall tests, AGENTS.md | v0.9 |
| 2026-05-11 | Drivers modulares | v0.10.0 |
| 2026-05-19 | HAL v0, eliminar unwrap() | v0.11+v0.12 |
| 2026-05-20 | StorageManager, 120 tests | v0.15 |
| 2026-05-21 | NEM suite, Event Bus v1, ELF64 | v0.14-v0.16 |
| 2026-05-23 | NVMe, mmap, pipes, FSCK | v0.16 |
| 2026-05-24 | NEM v3, PS/2 standalone | v0.16 |
| 2026-05-25 | libneodos, slab allocator | v0.16 |
| 2026-05-27 | Handle table, KOBJ v1 | v0.17 |
| 2026-05-28 | Page cache, PCI NEM, ACPI | v0.18-v0.21 |
| 2026-05-29 | ATA NEM, priority scheduler | v0.22-v0.23 |
| 2026-06-04 | Page cache advanced, DLL system | v0.24 |
| 2026-06-05 | Capabilities, isolation, hot reload | v0.24 |
| 2026-06-06 | HPET/APIC, buddy allocator rewrite | A0, C3 |
| 2026-06-07 | MCP server, EPROCESS/KTHREAD | A1.5 |
| 2026-06-08 | SSDT, SMP | A4.2, A1.1 |
| 2026-06-10 | IRQL, DPC engine | A2.4, A2.5 |
| 2026-06-11 | HAL v0.4 raw/safe, crash dump | v0.32 |
| 2026-06-12 | APC engine | A4.5 |
| 2026-06-13 | NeoInit (PID 1) | v0.35 |
| 2026-06-15 | **neoshell a Ring 3** | v0.37 |
| 2026-06-20 | NT5 Object Namespace | v0.39 |
| 2026-06-21 | NT6 Security (SID, Token, ACL) | v0.39 |
| 2026-06-22 | Object Manager, Slab\<T\>, KWait | v0.40-v0.42 |
| 2026-06-23 | ASLR v1, Ob migration | v0.44 |
| 2026-06-26 | Virtual terminals, console.nxl | v0.44.3 |
| 2026-06-27 | Timer/Semaphore/Section objects | v0.46 |
| 2026-06-28 | **TCP/IP networking** | v0.47 |
| 2026-06-30 | SAM, Registry hive (Cm) | v0.48 |
| 2026-07-01 | VirtIO Block, DHCP, networking stack | v0.48 |
| 2026-07-04 | Registry disk persistence | v0.48.7 |
| 2026-07-05 | Networking userland, auditorías | v0.48.8 |
| 2026-07-12 | **NeoKBD + ACPI Power Management** | v0.49.2 |

---

## Componentes nacidos en cada etapa

| Componente | Fecha | Descripción |
| ----------- | ------- | ------------- |
| Bootloader UEFI | May 4 | `neodos-bootloader/` — Carga kernel.elf y NeoDOS FS |
| Kernel base | May 4 | GDT, IDT, PIC, serial, paging, ATA, keyboard, VGA |
| NeoDOS FS | May 4 | Sistema de archivos propio con inodos y bloques |
| Shell (Ring 0) | May 4 | Comandos DOS-like, migrado a Ring 3 en v0.37 |
| ACPI | May 11 | Soporte para apagado del sistema |
| HAL | May 19 | `src/hal/` — Hardware Abstraction Layer |
| BlockDeviceManager | May 19 | Abstracción de dispositivos de bloque |
| NEM v3 | May 24 | Formato de drivers NeoDOS Module v3 |
| Event Bus | May 21 | Sistema de enrutamiento de eventos |
| ELF64 loader | May 21 | Carga de binarios ELF64 |
| libneodos | May 25 | Biblioteca estándar para Ring 3 |
| Slab allocator | May 26 | `src/slab.rs` — 9 clases de tamaño |
| KOBJ | May 27 | Kernel Object Manager (precursor de Ob) |
| Handle table | May 27 | Unified Handle Table |
| Page cache | May 28 | `src/buffer/page_cache.rs` |
| Priority scheduler | May 29 | 4 niveles, aging, preemption |
| NXL/DLL system | Jun 4 | Shared libraries |
| Capabilities | Jun 5 | X3 Capability System |
| Driver isolation | Jun 5 | X4 Isolation Layer |
| SMP | Jun 8 | Per-CPU, IPI, TLB shootdown |
| IRQL | Jun 10 | Interrupt Request Level framework |
| DPC engine | Jun 10 | Deferred Procedure Calls |
| APC engine | Jun 12 | Asynchronous Procedure Calls |
| NeoInit | Jun 13 | PID 1, proceso de inicio |
| Virtual terminals | Jun 26 | `\Device\Vt0..3` |
| console.nxl | Jun 26 | Biblioteca de terminal compartida |
| Object Manager (Ob) | Jun 22 | `src/object/` — Abstracción central |
| KWait | Jun 22 | Unified Wait Engine |
| ASLR | Jun 23 | Address Space Layout Randomization |
| Security RM | Jun 21 | SID, Token, ACL, SeAccessCheck |
| Networking | Jun 28 | TCP/IP, e1000, DHCP |
| Registry (Cm) | Jun 30 | `src/cm/` — Cell-based hives |
| SAM | Jun 30 | Security Account Manager |
| VirtIO Block | Jul 3 | `src/drivers/virtio_blk.rs` |
| net.nxl | Jul 5 | Biblioteca de red para Ring 3 |
| NeoKBD | Jul 12 | `src/kbd/` — Keyboard Manager con layouts dinámicos |
| ACPI Power | Jul 12 | `src/power/acpi.rs` — ACPI S5, reset register, FADT |
| kbdcompile | Jul 12 | `tools/kbdcompile/` — Layout .klc → .kbd compiler |
| neokey | Jul 12 | `userbin/neokey/` — Keyboard management CLI |

---

## Lugares donde ha crecido NeoDOS

### 🌱 Girona — Lugar de nacimiento

Todo el desarrollo hasta la fecha se ha realizado íntegramente en Girona.
Aquí nació NeoDOS el 4 de mayo de 2026 y aquí continúa su evolución.

### 🚀 Lleida — Pendiente de confirmar

Futura ubicación del desarrollo principal si el proyecto se traslada.
No marcar como completada hasta que ocurra el traslado.

---

## Métricas de crecimiento

| Fecha | Commits | Tests | Líneas kernel | Versión |
| ------- | --------- | ------- | --------------- | --------- |
| 2026-05-04 | 1 | 0 | ~2.000 | v0.5 |
| 2026-05-09 | ~15 | ~10 | ~5.000 | v0.7 |
| 2026-05-19 | ~40 | ~45 | ~10.000 | v0.12 |
| 2026-05-21 | ~50 | ~150 | ~15.000 | v0.16 |
| 2026-06-06 | ~100 | ~260 | ~25.000 | v0.24 |
| 2026-06-22 | ~180 | ~400 | ~35.000 | v0.42 |
| 2026-07-05 | ~280 | ~646 | ~45.580 | v0.48.8 |
| 2026-07-12 | ~310 | ~662 | ~47.200 | v0.49.2 |

---

## Notas sobre mantenimiento

Este documento debe actualizarse cuando ocurra un cambio arquitectónico importante.
No registrar pequeños cambios o correcciones menores.

**Criterios para añadir un hito:**

- Nuevo subsistema o componente importante.
- Refactorización que afecte a múltiples módulos.
- Cambio en la filosofía de diseño.
- Migración de funcionalidad entre capas (Ring 0 ↔ Ring 3).
- Congelación o descongelación de interfaces ABI/API.
- Hito de versión con impacto arquitectónico.
