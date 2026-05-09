# NeoDOS — Propuestas de mejora

> Priorizadas por impacto. Revisado: Mayo 2026

---

## 🐛 Críticos / Bugs

### 1. Sin scrolling en consola VGA

**Archivo:** `vga.rs:71`

Al llegar a la fila 50, el shell resetea el cursor a (0,0) y limpia toda la pantalla en lugar de hacer scroll. Se pierde todo el historial.

**Propuesta:** Implementar scroll real:
- Copiar las filas 1..N a 0..N-1 pixel a pixel (o con `memcpy` del framebuffer)
- Limpiar solo la última fila
- Alternativa más simple: buffer circular con offset de scroll

---

### 2. ATA drive select: master vs slave

**Archivo:** `ata.rs:14`

```rust
const ATA_DRIVE_SELECT_LBA_BASE: u16 = 0xF0; // 0xF0 = slave
```

El disco de arranque en QEMU se pasa como master (index=0), pero el driver lo trata como slave. El log muestra que lee correctamente, pero en hardware real o con distinta configuración de QEMU fallaría.

**Propuesta:** Cambiar a `0xE0` (master), o mejor, detectar el drive en inicialización.

---

### 3. `execute_batch()` no existe

**Archivos:** `shell/commands/call.rs`, `shell/shell.rs`

`call.rs` y `check_autoexec()` invocan `self.execute_batch(content)` pero el método no está definido en `DosShell`. El código no compilaría si se usara.

**Propuesta:** Implementar `execute_batch(&mut self, content: &str)` que parsea líneas y llama a `execute_line()`.

---

### 4. Frame allocator sin asignación

**Archivo:** `memory.rs`

El `FrameAllocator` tiene `mark_free_region()` y `mark_used_region()` pero **no hay `allocate_frame()`**. El allocator solo sirve para estadísticas. Cualquier intento de asignar memoria física falla.

**Propuesta:** Implementar `allocate_frame()` con búsqueda de primer bit libre en el bitmap, y `free_frame()`.

---

## ⚡ Rendimiento

### 5. Shell busy-wait 100% CPU

**Archivo:** `shell/shell.rs:177`

```rust
while self.running {
    // busy loop sin hlt
}
```

El shell consume una CPU completa al 100% en idle. En portátiles y máquinas reales esto calienta y consume batería.

**Propuesta:** Insertar `hlt` cuando no hay input:

```rust
if input::pop_byte().is_none() {
    core::arch::asm!("hlt");
}
```

O mejor: usar una waitqueue / IRQ-driven wakeup.

---

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
        color as u8,  // o manejarlo como u32
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

### 8. ATA PIO sin DMA

**Archivo:** `drivers/ata.rs`

Toda transferencia es PIO (Programmed I/O): la CPU espera activamente a que el disco esté listo y mueve cada palabra de 16 bits con `insw`. Durante una lectura de sector, la CPU no puede hacer otra cosa.

**Propuesta:** Implementar bus-master DMA usando el controlador IDE de la PCI. Las transferencias las maneja el hardware mientras la CPU sigue ejecutando código.

---

## 🧹 Calidad de código

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

### 11. `from_utf8_unchecked` en input del shell

**Archivo:** `shell/shell.rs:264`

```rust
let line = unsafe { core::str::from_utf8_unchecked(&line_buffer[..line_len]) };
```

Aunque el decodificador UTF-8 de arriba debería garantizar validez, un bug dejaría el sistema en estado indefinido.

**Propuesta:** Usar `from_utf8` y saltar el comando si falla.

---

### 12. TSR: buffer de 64 KB en el stack

**Archivo:** `tsr/mod.rs:32`

```rust
let mut buffer = [0u8; 65536];
```

El stack del kernel es limitado. Un array de 64 KB en el stack puede desbordarlo y causar corrupción de memoria.

**Propuesta:** Usar un buffer en BSS (`static mut`) o heap.

---

### 13. Cache dirty no persiste periódicamente

**Archivo:** `buffer/block_cache.rs`

Los bloques marcados como `dirty` solo se escriben al disco cuando son víctimas de evicción LRU. Si el sistema se apaga inesperadamente, se pierden.

**Propuesta:** `flush()` periódico desde el timer tick.

---

## 🆕 Funcionalidades

### 14. DEL y REN (stubs)

**Archivo:** `shell/commands/mod.rs:57-58`

```rust
"DEL" => println!("DEL not yet implemented"),
"REN" => println!("REN not yet implemented"),
```

**Propuesta:** Implementar borrado de archivos (marcar inodo como libre) y rename.

---

### 15. Heap allocator

Actualmente el kernel no tiene heap. No se puede usar `alloc` (`Box`, `Vec`, `String`). Todo es con buffers estáticos de tamaño fijo.

**Propuesta:** Integrar `linked_list_allocator` o un page allocator simple sobre el frame allocator.

---

### 16. FAT32 driver para el ESP

El kernel arranca desde una partición FAT32 (el ESP), pero no puede leerla. Solo entiende el FS propio NeoDOS.

**Propuesta:** Implementar un driver FAT32 mínimo (lectura) para poder leer archivos del ESP en runtime.

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

### 19. Test infrastructure

No hay tests. Cero cobertura.

**Propuesta:** Añadir `#[cfg(test)] mod tests` donde tenga sentido (parser de paths, environment, keyboard compose table, etc.). Para integración, test en QEMU con `expect`.

---

## 🏗️ Arquitectura

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

Se deshabilitan/habilitan las interrupciones en cada iteración del shell. El shell llama a `pop_byte()` en un loop cerrado, causando miles de cambios de estado de IF por segundo.

**Propuesta:** Usar `Mutex` sin cli/sti (el Mutex ya es seguro en single-core si las IRQs no lo toman), o usar un `AtomicU8` para head/tail y evitar el lock completamente.

---

### 21. Nombre engañoso: `vga.rs`

No usa VGA real — es una consola sobre framebuffer. El nombre confunde.

**Propuesta:** Renombrar a `console.rs` o `fbcon.rs`.

---

### 22. `print!` macro escribe a serial y VGA

**Archivo:** `vga.rs:35-39`

Útil para debug, pero el output serial incluye caracteres de control ANSI del framebuffer.

**Propuesta:** Separar `print!` en `vga_print!` y `serial_print!`, o que `_print` solo vaya a VGA y tener un `debug!` separado.

---

## 📦 Herramientas

### 23. `neodos_image.img` no se genera en `build.sh`

`build.sh` crea `disk_image.img` (FAT32 para UEFI) pero NO genera `scripts/neodos_image.img` (el FS NeoDOS). Hay que acordarse de ejecutar `python3 create_neodos_image.py` aparte.

**Propuesta:** Integrar la generación de `neodos_image.img` en `build.sh`.

---

### 24. Gap entre inode table y data blocks

**Archivo:** `scripts/create_neodos_image.py`

```
DATA_START_SECTOR = 200
```

Inodes ocupan sectores 1-63. Sectores 64-199 (68 KB) no se usan para nada.

**Propuesta:** Mover `DATA_START_SECTOR` a 64 o 128.
