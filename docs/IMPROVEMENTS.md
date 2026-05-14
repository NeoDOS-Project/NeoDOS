# NeoDOS — Propuestas de mejora pendientes

> Versión: 0.10.0 | Actualizado: Mayo 2026
> 
> Items ya implementados han sido removidos. Ver `AGENTS.md` para funcionalidad existente.

---

## ⚡ Rendimiento

### 6. `Renderer::clear()` píxel a píxel

**Archivo:** `graphics.rs:25`

```rust
for y in 0..self.fb.height {
    for x in 0..self.fb.width {
        self.put_pixel(x, y, color);
    }
}
```

En 1920×1080 son ~2 millones de iteraciones con llamada a función volátil cada una.

**Propuesta:** Usar `memset` o `rep stosd`:
```rust
unsafe {
    core::ptr::write_bytes(
        self.fb.base_address as *mut u8,
        color as u8,
        self.fb.size,
    );
}
```

---

### 7. Asignación de bloques O(256×12)

**Archivo:** `fs/neodos_fs.rs:allocate_block()`

Para encontrar el siguiente bloque libre, recorre todos los inodos (256) y sus punteros directos (12). En el peor caso: 3072 iteraciones por cada bloque escrito.

**Propuesta:** Implementar bitmap de bloques libres en el superbloque (o en un bloque dedicado). O(1) por asignación.

---

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

## 🧹 Calidad de código

---

### 9. `static mut` global sin sincronización

**Archivos:** `main.rs:39-41`, `graphics.rs:41`

```rust
pub static mut ATA_DRIVER: Option<AtaDriver> = None;
pub static mut BLOCK_CACHE: Option<BlockCache> = None;
pub static mut NEODOS_FS: Option<NeoDosFs> = None;
pub static mut RENDERER: Option<Renderer> = None;
```

Accesibles desde cualquier contexto sin locks, compilador emite warnings de UB.

**Propuesta:** Envolver en `Mutex<Option<T>>` o `OnceLock<Mutex<T>>`.

---

### 10. ~30 `unwrap()`/`expect()` que paniquean

Repartidos por todo el kernel. Cualquier fallo (disco corrupto, DMA malformado, etc.) tira el sistema.

**Propuesta:** Reemplazar con `?` y propagar errores. Al menos en las rutas no críticas (shell commands), mostrar mensaje de error en vez de paniquear.

---

### 31. Hardcoded 'C' drive en inicialización

**Archivo:** `shell/shell.rs:76`

```rust
let _ = drive_manager.mount('C', FsInstanceId::PRIMARY);
```

Solo se monta la unidad C. No hay soporte para A:, B:, D:, etc.

**Propuesta:** Hacer configurable el drive primario o auto-detectar.

> **IMPLEMENTADO (v0.10.1):** Ahora lee variable de entorno `SYSTEMDRIVE` para configurar el drive primario. Por defecto es `C`. Se inicializa automáticamente si no existe.

---

### 32. Sin validación de tamaño de archivo en COPY

**Archivo:** `shell/commands/copy.rs:16`

```rust
let mut buf = [0u8; 16384];
```

Buffer fijo de 16 KB. Archivos más grandes no se pueden copiar completamente.

**Propuesta:** Implementar lectura/escritura en chunks o usar heap dinámico.

---

### 33. Serial output mezclado con output VGA

**Archivo:** `console.rs` y `serial.rs`

Las macros `print!` y `println!` escriben tanto a VGA como a serial, pero el código serie incluye caracteres de control del framebuffer.

**Propuesta:** Separar en macros distintas o sanitizar output serie.

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

No hay funcionalidad `fsck`. Un FS corrupto pasa desapercibido.

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

## 🆕 Funcionalidades

---

### 17. `DIR /W`, `DIR /P`

**Archivo:** `shell/commands/dir.rs`

Solo listado vertical simple.

**Propuesta:** Añadir `DIR /W` (wide, columnas) y `DIR /P` (pausa cada pantalla).

---

### 18. Historial de comandos

No hay forma de recuperar comandos anteriores. Cada línea hay que tipearla de nuevo.

**Propuesta:** Buffer circular de ~16 comandos, navegación con ↑/↓ (scan codes 0x48/0x50).

---

### 40. PROMPT command

No hay forma de cambiar el prompt dinámicamente (más allá de SET PROMPT).

**Propuesta:** Soporte completo de variables en prompt ($P, $G, $T, etc).

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

## 🏗️ Arquitectura

---

### 20. Interrupts habilitados/deshabilitados en cada `pop_byte()`

**Archivo:** `input.rs:52-61`

```rust
pub fn pop_byte() -> Option<u8> {
    crate::arch::x64::disable_interrupts();
    let result = { let mut b = INPUT_BUFFER.lock(); b.pop() };
    crate::arch::x64::enable_interrupts();
    result
}
```

Se deshabilitan/habilitan las interrupciones en cada iteración del shell.

**Propuesta:** Usar `Mutex` sin cli/sti, o usar un `AtomicU8` para head/tail y evitar el lock completamente.

---

### 22. `print!` macro escribe a serial y VGA

**Archivo:** `vga.rs:35-39`

Útil para debug, pero el output serial incluye caracteres de control ANSI del framebuffer.

**Propuesta:** Separar `print!` en `vga_print!` y `serial_print!`, o que `_print` solo vaya a VGA y tener un `debug!` separado.

---

### 45. graphics: Sin double buffering

**Archivo:** `graphics.rs`

Cada `put_pixel` escribe directamente al framebuffer, causando flicker en animaciones.

**Propuesta:** Implementar double buffer y swap al final del frame.

---

## 📦 Herramientas

---

### 24. Gap entre inode table y data blocks

**Archivo:** `scripts/create_neodos_image.py`

```
DATA_START_SECTOR = 200
```

Inodes ocupan sectores 1-63. Sectores 64-199 (68 KB) no se usan para nada.

**Propuesta:** Mover `DATA_START_SECTOR` a 64 o 128.

---

### 47. No hay version checking

No hay verificación de que las versiones del bootloader y kernel sean compatibles.

**Propuesta:** Agregar magic number o versión en ambos extremos.

> **IMPLEMENTADO (v0.10.2):** Añadido magic number `0x4E444F53` ("NDOS") y verificación de versión en BootInfo. El kernel comprueba compatibilidad al iniciar.

---

### 48. AHCI `read_sectors` returns only 1 sector

**Archivo:** `drivers/ahci.rs:194-203`

```rust
fn ata_dma(&mut self, port: u8, lba: u64, count: u8) -> Result<[u8; 512], ()> {
    // ... sends count sectors via DMA into DMA_BUF (4096 bytes) ...
    let mut buf = [0u8; 512];
    buf.copy_from_slice(&DMA_BUF[..512]);  // <-- always copies 512 bytes!
    Ok(buf)
}
```

`read_sectors()` calls `ata_dma(count)` but only gets 512 bytes back, then tries to copy `count*512` bytes into the output buffer — panics if count > 1.

**Propuesta:** Have `ata_dma` return `Result<(), ()>` and let `read_sectors` directly consume from `DMA_BUF`.

---

### 49. AHCI multi-port uses shared static buffers

**Archivo:** `drivers/ahci.rs:97-117`

`CMD_LIST`, `RECV_FIS`, `CMD_TABLE`, `DMA_BUF` are single static instances shared by all AHCI ports. Only one port can be active at a time (fine for single-core polling), but if IRQ-driven DMA is added later, buffers would collide.

**Propuesta:** Allocate per-port buffers or use a pool.

---

### 50. Q35: ATA driver ports don't exist

On Q35 (`-machine q35`), legacy PIO ports 0x1F0/0x170 are not present. The `AtaDriver` for both channels still initialises and attempts PIO. The `ahci_fallback` flag transparently delegates to AHCI, but:

- `write_sector()`/`write_sectors()`: AHCI write is stubbed to `Err(())` — writes to Q35 disks fail silently.
- DMA (BMBA) is disabled (`No IDE bus-master controller found`).
- The secondary ATA channel driver (`is_secondary=true`) routes to AHCI port 1 (second disk).

**Propuesta:** Implement AHCI write path, or detect at init time that PIO ports are missing and skip initialisation.

---

### 51. Dependencias no versionadas

Los crates en `Cargo.toml` no tienen versiones fijas, puede haber breakages.

**Propuesta:** Usar `cargo lock` y revisar cambios.

---

## Resumen por prioridad

| Prioridad | Items |
|-----------|-------|
| Alta | #6 (clear píxel a píxel), #7 (allocate block), #9 (static mut), #28 (cache size), #35 (max processes), #41 (PATH resolution), **#50 (Q35 AHCI write stub)** |
| Media | #10 (unwrap), #17 (DIR /W), #18 (history), #20 (pop_byte cli/sti), #32 (COPY buffer), #33 (serial/VGA), #36 (quantum), #37 (FS integrity), #42 (redirection), #43 (quotes), **#48 (AHCI read_sectors bug)**, **#49 (shared AHCI buffers)** |
| Baja | #22 (print macro), #24 (gap), #29 (paging), #30 (inode cache), #38 (extended attributes), #40 (PROMPT), #45 (double buffer), #47 (version check), #51 (deps) |
