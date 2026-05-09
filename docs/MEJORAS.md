# NeoDOS - Mejoras y Mejoras Adicionales

> Documento de revisión de código - Mayo 2026

Este documento complementa el existente `IMPROVEMENTS.md` con hallazgos adicionales de la revisión del código.

---

## Estado de mejoras previamente documentadas

### ✓ YA IMPLEMENTADAS

1. **`execute_batch()` en batch.rs** - Ya implementado en `src/shell/batch.rs`
2. **Frame allocator con `allocate_frame()`** - Ya implementado en `src/memory.rs:134-156`
3. **Shell busy-wait parcialmente mejorado** - Ahora usa `hlt` en el loop (`shell.rs:264`)

---

## Nuevas mejoras identificadas

## 🐛 Bugs adicionales

### 25. Keyboard: pérdida de scan codes en buffer pequeño

**Archivo:** `input.rs:6`

```rust
pub struct InputBuffer {
    buffer: [u8; 256],  // Solo 255 bytes útiles
```

El buffer de input es solo 256 bytes. En uso intensivo, se pueden perder scan codes.

**Propuesta:** Aumentar a 1024 bytes o implementar backpressure visual.

---

### 26. VGA scroll: pérdida de contenido al hacer scroll manual

**Archivo:** `vga.rs`

Cuando el usuario presiona `Enter` en la última fila, el contenido anterior se pierde porque no hay buffer de scroll.

**Propuesta:** Implementar scrolling real como se sugiere en IMPROVEMENTS.md #1.

---

### 27. TSR: Sin verificación de conflictos de vectores de interrupción

**Archivo:** `tsr/mod.rs`

Los programas TSR registran handlers de interrupción sin verificar si ya hay un handler activo en ese vector.

**Propuesta:** Agregar tabla de vectores registrados y verificar conflictos.

---

## ⚡ Rendimiento adicional

### 28. Block cache: solo 64 entradas

**Archivo:** `buffer/block_cache.rs:5`

```rust
const CACHE_SIZE: usize = 64;
```

64 entradas = 32 KB de cache. Para un FS de múltiples MB, es muy pequeño.

**Propuesta:** Aumentar a 256 o 512 entradas (128-256 KB).

---

### 29. Paging: Page tables recreadas en cada boot

**Archivo:** `arch/x64/paging.rs`

Las page tables se crean desde cero en cada arranque. No hay reutilización de páginas del bootloader.

**Propuesta:** Reusar páginas del bootloader antes de crear nuevas.

---

### 30. Inode cache: sin escritura diferida

**Archivo:** `fs/neodos_fs.rs:72-108`

El `InodeCache` carga inodos pero nunca los escribe de vuelta cuando se modifican.

**Propuesta:** Implementar dirty flag y escritura diferida para inodos.

---

## 🧹 Calidad de código adicional

### 31. Hardcoded 'C' drive en inicialización

**Archivo:** `shell/shell.rs:76`

```rust
let _ = drive_manager.mount('C', FsInstanceId::PRIMARY);
```

Solo se monta la unidad C. No hay soporte para A:, B:, D:, etc.

**Propuesta:** Hacer configurable el drive primario o auto-detectar.

---

### 32. Sin validación de tamaño de archivo en COPY

**Archivo:** `shell/commands/copy.rs:16`

```rust
let mut buf = [0u8; 16384];
```

Buffer fijo de 16KB. Archivos más grandes no se pueden copiar completamente.

**Propuesta:** Implementar lectura/escritura en chunks o usar heap dinámico.

---

### 33. Serial output mezclado con output VGA

**Archivo:** `vga.rs` y `serial.rs`

Las macros `print!` y `println!` escriben tanto a VGA como a serial, pero el código serie incluye caracteres de control del framebuffer.

**Propuesta:** Separar en macros distintas o sanitizar output serie.

---

### 34. Sin graceful shutdown

**Archivos:** `shell/commands/shutdown.rs`, `main.rs`

El comando `SHUTDOWN` no hace flush del cache ni desmonta el FS干净的.

**Propuesta:** Agregar:
1. Flush de block cache
2. Sync del FS (marcar clean)
3. Apagado limpio del hardware

---

### 35. Scheduler: solo 4 procesos máximos

**Archivo:** `scheduler.rs:6`

```rust
const MAX_PROCESSES: usize = 4;
```

Límite muy bajo para cualquier uso práctico.

**Propuesta:** Aumentar a 16 o 32 procesos.

---

### 36. Scheduler: round-robin sin quantum configurable

**Archivo:** `scheduler.rs:209-210`

```rust
// Every 100 ticks (10ms), switch process
if self.timer_ticks % 100 == 0 {
```

El quantum de 100 ticks (10ms) está hardcoded.

**Propuesta:** Hacer configurable vía environment variable.

---

### 37. No hay verificación de integridad del FS

**Archivo:** `fs/neodos_fs.rs`

No hay `fsck`-like functionality. Un FS corrupto pasa desapercibido.

**Propuesta:** Implementar verificación de:
- Magic number del superblock
- Link counts de inodos
- Referencia circular en directorios

---

### 38. Sin soporte para atributos extendidos

**Archivo:** `fs/neodos_fs.rs:24-37`

El `Inode` tiene campos de owner_uid/gid pero no se usan.

**Propuesta:** Implementar permisos básicos o atributos como hidden/system.

---

## 🆕 Funcionalidades adicionales

### 39. DATE y TIME commands

No existen comandos para mostrar/ajustar fecha y hora del sistema.

**Propuesta:** Implementar con RTC (Real Time Clock) del hardware.

---

### 40. PROMPT command

No hay forma de cambiar el prompt dinámicamente (más allá de SET PROMPT).

**Propuesta:** Soporte completo de variáveis en prompt ($P, $G, $T, etc).

---

### 41. Environment: sin PATH resolution completa

**Archivo:** `shell/shell.rs`

Cuando se ejecuta un comando, no se busca en PATH si no existe en el directorio actual.

**Propuesta:** Implementar búsqueda en PATH para comandos no built-in.

---

### 42. Sin redirección de output

```bash
DIR > FILE.TXT
TYPE FILE.TXT | MORE
```

No soportado.

**Propuesta:** Implementar parser de pipes y redirección.

---

### 43. Sin soporte de argumentos con comillas

```bash
echo "Hello World"
```

El parser actual no maneja comillas correctamente.

**Propuesta:** Mejorar el parser de argumentos.

---

## 🏗️ Arquitectura adicional

### 44. drivers/ata.rs: Sin soporte para múltiples discos

**Archivo:** `ata.rs:21`

```rust
const ATA_DRIVE_SELECT_LBA_BASE: u8 = 0xF0; // Slave only
```

Solo funciona con el disco slave (index=1 en QEMU).

**Propuesta:** Auto-detección o configuración en tiempo de build.

---

### 45.graphics: Sin double buffering

**Archivo:** `graphics.rs`

Cada `put_pixel` escribe directamente al framebuffer, causando flicker en animaciones.

**Propuesta:** Implementar double buffer y swap al final del frame.

---

### 46.vga.rs: Nombre confundidor

El archivo `vga.rs` en realidad usa framebuffer, no VGA real.

**Propuesta:** Renombrar a `console.rs` o `fbcon.rs`.

---

## 📦 Build system

### 47. No hay version checking

No hay verificación de que las versiones del bootloader y kernel sean compatibles.

**Propuesta:** Agregar magic number o versión en ambos extremos.

---

### 48. Dependencias no versionadas

Los crates en `Cargo.toml` no tienen versiones fijas, puede haber breakages.

**Propuesta:** Usar `cargo lock` y revisar cambios.

---

## Resumen por prioridad

| Prioridad | Items |
|-----------|-------|
| Crítica | #25 (input buffer), #27 (TSR vector conflicts), #34 (no graceful shutdown) |
| Alta | #28 (cache size), #31 (hardcoded C:), #35 (max processes), #41 (PATH resolution) |
| Media | #32 (COPY buffer), #37 (FS integrity), #39 (DATE/TIME), #42 (redirection) |
| Baja | #45 (double buffering), #46 (naming), #47 (version check) |

---

## Comparación con IMPROVEMENTS.md original

Este documento agrega **24 nuevas mejoras** a las 24 originales (ahora 48 total), mientras marca 3 como ya implementadas.

El documento `IMPROVEMENTS.md` sigue siendo la referencia principal con las mejoras más críticas documentadas.