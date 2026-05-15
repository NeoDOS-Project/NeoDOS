# NeoDOS — Propuestas de mejora pendientes

> Versión: 0.10.4 | Actualizado: Mayo 2026

> Items ya implementados han sido removidos. Ver `AGENTS.md` para funcionalidad existente.

---

## ⚡ Rendimiento

### 29. Paging: Page tables recreadas en cada boot

**Archivo:** `arch/x64/paging.rs`

Las page tables se crean desde cero en cada arranque. No hay reutilización de páginas del bootloader.

**Propuesta:** Reusar páginas del bootloader antes de crear nuevas o implementar un sistema de mapeo bajo demanda (on-demand paging) para el espacio de usuario.

---

### 30. Inode cache: sin escritura diferida

**Archivo:** `fs/neodos_fs.rs:72-108`

El `InodeCache` carga inodos pero nunca los escribe de vuelta cuando se modifican hasta que se hace un sync manual.

**Propuesta:** Implementar dirty flag y escritura diferida para inodos.

---

### 52. DMA: Uso de buffers estáticos limitados

**Archivo:** `drivers/ahci.rs`, `drivers/ata.rs`

Los buffers de DMA son estáticos y de tamaño fijo (4KB/8 sectores). Esto limita el rendimiento en transferencias grandes.

**Propuesta:** Implementar un pool de páginas para DMA o permitir transferencias multi-bloque más grandes usando scatter-gather (PRDT) de forma dinámica.

---

## 🧹 Calidad de código

### 9. [COMPLETADO] `static mut` global sin sincronización (Riesgo de Reentrancia)

**Archivos:** `globals.rs`, `main.rs`

```rust
pub static mut ATA_DRIVER: Option<AtaDriver> = None;
pub static mut BLOCK_CACHE: Option<BlockCache> = None;
pub static mut NEODOS_FS: Option<NeoDosFs> = None;
```

**Problema:** Actualmente se accede a estas estructuras mediante `static mut` sin ningún mecanismo de bloqueo. Aunque el sistema es mononúcleo, el planificador (scheduler) puede interrumpir una syscall en medio de una operación crítica (ej. modificando el bitmap del FS) y cambiar a otro proceso que realice otra syscall sobre el mismo objeto. Esto provocaría corrupción de datos difícil de depurar.

**Propuesta:** 
1. Envolver todos los drivers y el FS en `spin::Mutex<Option<T>>`.
2. Usar `lazy_static!` o `OnceLock` (si estuviera disponible) para la inicialización.
3. Asegurar que las secciones críticas deshabiliten interrupciones si es necesario para evitar deadlocks con los manejadores de IRQ.

---

### 10. ~30 `unwrap()`/`expect()` que paniquean

Repartidos por todo el kernel. Cualquier fallo (disco corrupto, DMA malformado, etc.) tira el sistema.

**Propuesta:** Reemplazar con `?` y propagar errores. Implementar un sistema de logging (`log` crate compatible con `no_std`) en lugar de `println!` para errores.

---

### 33. Serial output mezclado con output VGA

**Archivo:** `console.rs` y `serial.rs`

Las macros `print!` y `println!` escriben tanto a VGA como a serial.

**Propuesta:** Implementar un sistema de "Log Sinks" donde se pueda registrar dónde va el output (VGA, Serial, Archivo).

---

### 53. Unificación de Drivers de Bloque

**Archivo:** `main.rs`, `drivers/mod.rs`

Actualmente `AtaDriver` y `AhciDriver` se manejan de forma algo separada con fallbacks manuales.

**Propuesta:** Crear un trait `BlockDevice` y un sistema de registro de dispositivos para que el FS no dependa de si el disco es ATA, AHCI o un RAM disk.

---

## 🆕 Funcionalidades

### 17. `DIR /W`, `DIR /P`

**Archivo:** `shell/commands/dir.rs`

Solo listado vertical simple.

**Propuesta:** Añadir `DIR /W` (wide, columnas) y `DIR /P` (pausa cada pantalla).

---

### 18. Historial de comandos

No hay forma de recuperar comandos anteriores.

**Propuesta:** Buffer circular de ~16 comandos, navegación con ↑/↓ (scan codes 0x48/0x50).

---

### 41. Environment: sin PATH resolution completa

**Archivo:** `shell/shell.rs`

**Propuesta:** Implementar búsqueda en PATH para comandos no built-in de forma recursiva o siguiendo el estándar DOS.

---

### 42. Sin redirección de output

```bash
DIR > FILE.TXT
```

**Propuesta:** Implementar redirección básica de `stdout` a archivos en el shell.

---

### 54. Standard Library para User-mode (`libneodos`)

**Carpeta:** `userbin/`

Las aplicaciones de usuario llaman a `int 0x80` directamente.

**Propuesta:** Crear un crate `libneodos` que proporcione una interfaz segura y "Rústica" para las syscalls (ej. `neodos::println!`, `neodos::fs::open`).

---

### 55. Soporte para ejecutables ELF en User-mode

Actualmente solo soporta binarios planos (.BIN) cargados en una dirección fija.

**Propuesta:** Implementar un cargador ELF básico que soporte relocalización estática.

---

## 🏗️ Arquitectura

### 20. Interrupts habilitados/deshabilitados en cada `pop_byte()`

**Archivo:** `input.rs:52-61`

**Propuesta:** Usar un ring buffer lock-free (AtomicU8 para head/tail) para evitar deshabilitar interrupciones globalmente.

---

### 56. Capa VFS (Virtual File System)

**Carpeta:** `fs/`

La lógica de unidades (C:, A:) está mezclada en el shell y `DriveManager`.

**Propuesta:** Implementar un VFS real que abstraiga el acceso a archivos, permitiendo montar diferentes FS en cualquier punto y unificando el acceso a archivos de usuario y kernel.

---

### 57. Sistema de Eventos/Mensajería para Input

El shell hace pooling de `input::pop_byte()`.

**Propuesta:** Cambiar a un modelo basado en eventos o bloqueante (`sys_read` sobre stdin que bloquee el proceso hasta que haya datos).

---

### 58. USB HID: Soporte funcional

**Archivo:** `drivers/usb_hid/mod.rs`

Actualmente no funcional.

**Propuesta:** Corregir la inicialización de UHCI (especialmente alineación de Frame List y manejo de BARs) para soportar teclados USB en hardware real/QEMU PIIX3.

---

## 📦 Herramientas

### 37. No hay verificación de integridad del FS

**Propuesta:** Crear utilidad `FSCK.BIN` para verificar y reparar el NeoDOS FS.

---

### 59. Tests de integración en CI

Los tests se corren manualmente con `auto_test.py`.

**Propuesta:** Configurar GitHub Actions para ejecutar `auto_test.py` en cada push.

---

---

### 24. Gap entre inode table y data blocks

**Archivo:** `scripts/create_neodos_image.py`

```
DATA_START_SECTOR = 200
```

Inodes ocupan sectores 1-63. Sectores 64-199 (68 KB) no se usan para nada.

**Propuesta:** Mover `DATA_START_SECTOR` a 64 o 128.

---

---

### 51. Dependencias no versionadas

Los crates en `Cargo.toml` no tienen versiones fijas, puede haber breakages.

**Propuesta:** Usar `cargo lock` y revisar cambios.

---

## 🚀 Camino a v1.0

### 60. Gestión Dinámica de Memoria (v1.0)

**Archivo:** `memory.rs`, `arch/x64/paging.rs`

Actualmente los procesos usan "slots" fijos de 128 KB. Esto es ineficiente y limitante.

**Propuesta:** Implementar un gestor de memoria virtual que asigne páginas físicas bajo demanda. Permitir que los procesos crezcan (heap dinámico en Ring 3) mediante syscalls como `sys_brk` o `sys_mmap`.

---

### 61. IPC: Pipes y Redirección Real (v1.0)

**Archivo:** `syscall.rs`, `shell/shell.rs`

El shell no soporta tuberías (`|`) entre procesos independientes.

**Propuesta:** Implementar un sistema de tuberías anónimas en el kernel. Redirigir el `stdout` de un proceso al `stdin` de otro, permitiendo comandos complejos como `DIR | SORT | MORE`.

---

### 62. Soporte Completo de FAT32 (Escritura)

**Archivo:** `drivers/fat32.rs`

Actualmente el driver de FAT32 es mayoritariamente de lectura (usado para el ESP).

**Propuesta:** Implementar creación, escritura y borrado de archivos en particiones FAT32. Esto permitirá que NeoDOS interactúe mejor con otros sistemas operativos.

---

### 63. Scripting Avanzado en Batch

**Archivo:** `shell/batch.rs`

Los archivos `.BAT` son muy limitados (secuenciales simples).

**Propuesta:** Añadir soporte para variables de entorno locales, etiquetas y saltos (`GOTO`), condicionales (`IF`) y bucles simples (`FOR`). Implementar expansión de wildcards (`*`, `?`) en todos los comandos de archivo.

---

### 64. Integración de RTC y Timestamps

**Archivo:** `drivers/rtc.rs`, `fs/neodos_fs.rs`

El RTC se inicializa pero no se usa para marcar la fecha/hora de los archivos.

**Propuesta:** Usar la hora del RTC en las funciones `create_file` y `write_file` para mantener actualizados los campos `ctime` y `mtime` de los inodos.

---

### 65. Interfaz Gráfica de Usuario (GUI) Básica

**Carpeta:** `graphics/`

NeoDOS solo tiene una interfaz de línea de comandos sobre un framebuffer gráfico.

**Propuesta:** Implementar un gestor de ventanas básico (estilo Windows 1.x/3.x) que soporte ventanas solapadas, dibujo de primitivas (rectángulos, líneas) y soporte para ratón PS/2.

---


### 67. Pila de Red (TCP/IP) Mínima

**Propuesta:** Implementar drivers para E1000 (Intel) y una pila de red básica (ARP, IP, ICMP, UDP) para permitir funcionalidades como `PING` o transferencia de archivos por red (estilo TFTP).

---


---

### 68. Gestión de Energía (ACPI)

**Propuesta:** Implementar soporte básico de ACPI para permitir los comandos `SHUTDOWN` y `REBOOT` desde el shell.

---

### 69. Drivers Cargables Dinámicamente (.SYS)

**Archivo:** `drivers/mod.rs`, `config.rs`

Actualmente todos los drivers están compilados estáticamente en el kernel.

**Propuesta:** Implementar soporte para cargar drivers en tiempo de ejecución (archivos `.SYS` o `.DRV`). Definir una interfaz estándar para que el kernel pueda llamar a funciones de inicialización y manejo de interrupciones de drivers externos.

---

### 70. Soporte de Sonido (PC Speaker / SB16)

**Carpeta:** `drivers/audio/`

**Propuesta:** Implementar un driver básico para el PC Speaker (frecuencias cuadradas) y soporte inicial para SoundBlaster 16 (DMA de audio) para permitir la reproducción de archivos `.WAV` simples.

---

### 71. Soporte de CD-ROM (ISO9660)

**Archivo:** `drivers/atapi.rs`

**Propuesta:** Implementar el sistema de archivos ISO9660 para permitir la lectura de discos compactos (CD-ROM) a través de la interfaz ATAPI del driver IDE/SATA.

---

### 72. Autocompletado con TAB en el Shell

**Archivo:** `shell/shell.rs`

**Propuesta:** Mejorar la experiencia de usuario en el shell permitiendo completar nombres de archivos y comandos mediante la tecla TAB, escaneando el directorio actual y el PATH.

---

### 73. Monitor de Sistema/Kernel (KMonitor)

**Propuesta:** Implementar un monitor integrado (accesible mediante una combinación de teclas) que permita inspeccionar registros de la CPU, volcar memoria física, ver el estado de la tabla de procesos y realizar debugging básico sin depender de GDB externo.

---

### 74. Soporte SMP (Multi-core) Inicial

**Archivo:** `arch/x64/cpu.rs`

NeoDOS solo usa el core de arranque (BSP).

**Propuesta:** Usar el APIC local y el I/O APIC para despertar cores secundarios (APs) y permitir la ejecución de tareas en paralelo, mejorando la respuesta del sistema bajo carga.

---

### 75. USB Mass Storage (Pendrives)

**Archivo:** `drivers/usb/`

**Propuesta:** Una vez estabilizado el driver UHCI/EHCI, implementar la clase de dispositivo USB Mass Storage para poder montar pendrives como unidades de disco adicionales (D:, E:, etc.).

---

---

### 75. USB Mass Storage (Pendrives)

**Archivo:** `drivers/usb/`

**Propuesta:** Una vez estabilizado el driver UHCI/EHCI, implementar la clase de dispositivo USB Mass Storage para poder montar pendrives como unidades de disco adicionales (D:, E:, etc.).

---

### 76. NeoDOS SDK (Toolchain externa)

**Propuesta:** Desarrollar un SDK basado en LLVM que permita compilar aplicaciones en C y Rust para NeoDOS desde un sistema host (Linux/Windows), automatizando la generación de binarios compatibles con las syscalls de NeoDOS.

---

### 77. Journaling en NeoDOS FS (v2.0)

**Archivo:** `fs/neodos_fs.rs`

El sistema de archivos actual es vulnerable a la corrupción en caso de apagado inesperado.

**Propuesta:** Implementar un diario (journal) para las operaciones de metadatos. Esto garantizaría la integridad del sistema de archivos incluso tras un fallo de alimentación o un kernel panic.

---

### 78. Configuración Dinámica de Teclado (KEYB)

**Archivo:** `drivers/keyboard.rs`

Actualmente los layouts (US/SP) se eligen en tiempo de compilación.

**Propuesta:** Implementar el comando `KEYB` para cambiar el mapeo del teclado en tiempo de ejecución, cargando tablas de escaneo desde archivos de configuración en el disco.

---

### 79. Drivers VirtIO (Optimización en VM)

**Archivo:** `drivers/virtio/`

**Propuesta:** Implementar drivers `virtio-blk`, `virtio-net` y `virtio-gpu` para obtener el máximo rendimiento cuando NeoDOS se ejecuta como invitado en QEMU/KVM.

---

### 80. NeoEdit: Editor de Texto Integrado

**Propuesta:** Desarrollar un editor de texto básico basado en consola (similar al `EDIT.COM` de MS-DOS) para permitir la creación y modificación de archivos `.BAT` y `.SYS` directamente desde el sistema.

---

### 81. Soporte de Fuentes Vectoriales (TTF/OTF)

**Archivo:** `graphics/font.rs`

Actualmente el sistema usa una fuente bitmap de 8x16.

**Propuesta:** Implementar un rasterizador de fuentes básico para soportar fuentes TrueType o OpenType, mejorando drásticamente la legibilidad en el modo GUI.

---

---

### 82. Defragmentador de Disco (NEODEFRAG)

**Propuesta:** Debido a que el NeoDOS FS usa bloques directos, la fragmentación puede afectar al rendimiento. Implementar una utilidad que reorganice los bloques de los archivos de forma contigua en el disco.

---

### 83. Manejo de Excepciones en User-Mode (Signals)

**Archivo:** `arch/x64/interrupts.rs`

**Propuesta:** Permitir que los procesos de Ring 3 capturen excepciones de la CPU (como división por cero o fallos de página) mediante el registro de manejadores de señales, evitando la terminación abrupta del proceso.

---

### 84. Soporte de Swap (Memoria Virtual en Disco)

**Propuesta:** Implementar un mecanismo de swapping que use un archivo oculto (`NEOSWAP.SYS`) para volcar páginas de memoria inactivas al disco, permitiendo ejecutar aplicaciones que requieran más RAM de la físicamente disponible.

---

### 85. Cifrado de Disco (NeoCrypt)

**Propuesta:** Añadir soporte para particiones cifradas (AES-XTS) en el NeoDOS FS, integrando la solicitud de contraseña durante el proceso de arranque (boot sequence).

---

### 86. Cambio Dinámico de Resolución (VBE/GOP)

**Archivo:** `graphics/mod.rs`

**Propuesta:** Permitir cambiar la resolución y profundidad de color del framebuffer en tiempo de ejecución sin necesidad de reiniciar el sistema, mediante llamadas al driver de video.

---

### 87. Instalador Automatizado (SETUP.EXE)

**Propuesta:** Crear un programa de instalación que automatice el particionamiento (GPT), formateo y copia de los archivos base del sistema a un disco nuevo, facilitando la instalación en hardware real.

---

### 88. Soporte de Scroll en Ratón PS/2

**Propuesta:** Extender el driver de ratón para soportar el protocolo de 4 bytes (IntelliMouse), permitiendo usar la rueda de desplazamiento en aplicaciones y en el shell futuro.

---

### 89. Sistema de Ayuda Contextual (HELP /?)

**Propuesta:** Implementar una base de datos de ayuda comprimida y un motor de visualización que permita consultar la sintaxis de cualquier comando mediante `HELP <comando>` o el parámetro `/?`.

---

### 90. Terminales Virtuales (Alt+F1..F4)

**Archivo:** `console.rs`

**Propuesta:** Implementar múltiples buffers de consola independientes para permitir que el usuario alterne entre diferentes sesiones de terminal activas mediante combinaciones de teclas.

---

### 91. Driver de Disquetera (FDC 1.44MB)

**Propuesta:** Implementar el driver para el controlador de disquetera clásico (Intel 82077A) para permitir el acceso a discos de 3.5 pulgadas en hardware antiguo o emulado.

---

### 92. Utilidad de Compresión Nativa (NeoZip)

**Propuesta:** Desarrollar una utilidad integrada en el sistema para comprimir y descomprimir archivos en formato ZIP o DEFLATE, facilitando la distribución de software.

---

### 93. Carga de Microcódigo de CPU

**Propuesta:** Implementar la capacidad de cargar actualizaciones de microcódigo de Intel/AMD durante la fase temprana del arranque para corregir erratas del procesador.

---

### 94. Subsistema de Impresión (LPT/USB-Print)

**Propuesta:** Implementar drivers básicos para el puerto paralelo (LPT) y la clase de impresión USB para permitir el envío de texto plano a impresoras matriciales o láser.

---

### 95. Gestor de Paquetes (NEOPKG)

**Propuesta:** Desarrollar una herramienta que, mediante la pila de red, permita buscar, descargar e instalar software desde repositorios oficiales de NeoDOS.

---

### 96. Emulación de Binarios .COM (Legacy Mode)

**Propuesta:** Integrar un pequeño emulador de modo real de 16 bits que permita ejecutar utilidades clásicas de MS-DOS (archivos .COM) dentro del entorno protegido de NeoDOS.

---

### 97. Redirección Serial de Consola

**Propuesta:** Añadir una opción en `CONFIG.SYS` para redirigir toda la entrada y salida del shell al puerto COM1, permitiendo controlar NeoDOS desde una terminal serie externa.

---

### 98. Registro de Auditoría (Audit Log)

**Propuesta:** Implementar un sistema de logging persistente que registre eventos críticos como errores de hardware, intentos de acceso fallidos y cambios en la configuración del sistema.

---

### 99. Protector de Pantalla (NeoSaver)

**Propuesta:** Un protector de pantalla gráfico simple (estilo "Starfield" o "Marquee") que se active tras un periodo de inactividad para proteger monitores CRT.

---

### 100. Manual de Usuario "Anniversary Edition"

**Propuesta:** Compilar toda la documentación técnica, guía de comandos y tutoriales en un único manual maestro en formato PDF/Markdown para la versión 1.0.

---

## Resumen por prioridad (Actualizado Final 100 Items)

| Prioridad | Items |
|-----------|-------|
| **Bloqueante (v1.0)** | #56 (VFS), #60 (Dynamic Memory), #87 (Setup) |
| **Completado** | #9 (static mut), #66 (Syscall Stability) |
| **Alta** | #53 (Block Abstraction), #61 (IPC Pipes), #62 (FAT32 Write), #69 (Loadable Drivers), #76 (SDK), #80 (NeoEdit), #89 (Help) |
| **Media** | #63 (Batch), #64 (RTC), #68 (ACPI), #72 (TAB), #77 (Journaling), #78 (KEYB), #82 (Defrag), #90 (VT), #95 (Pkg) |
| **Baja** | #65 (GUI), #67 (Network), #70 (Sound), #71 (ISO9660), #74 (SMP), #79 (VirtIO), #81 (TTF), #91 (Floppy), #96 (COM) |
