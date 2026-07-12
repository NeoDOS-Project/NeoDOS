# NeoDev — NeoDOS Development Tool

Una única herramienta Rust que centraliza compilación, generación de imágenes,
ejecución QEMU y pruebas automatizadas para NeoDOS.

## Uso

```text
neodev build        Compilar todos los componentes
neodev build --quick        Solo kernel + bootloader
neodev build --image        Compilar y generar imagen de disco
neodev image                Generar imagen de disco (NE2 + ESP + GPT)
neodev run                  Ejecutar NeoDOS en QEMU
neodev run --kvm --gdb      Con KVM y GDB
neodev run --storage virtio   Almacenamiento VirtIO
neodev test                 Ejecutar tests automatizados
neodev test --iterations 5  Ejecutar 5 iteraciones
neodev clean                Limpiar artefactos de compilación
neodev list                 Listar proyectos descubiertos
neodev config               Mostrar configuración
```

## Arquitectura

```text
tools/neodev/
├── Cargo.toml
├── src/
│   ├── main.rs       # CLI (clap) y punto de entrada
│   ├── config.rs     # Configuración centralizada
│   ├── discovery.rs  # Descubrimiento automático de proyectos
│   ├── build.rs      # Compilación (kernel, bootloader, NXE, NXL, NEM)
│   ├── image.rs      # Generación de imágenes (NE2, ESP, GPT)
│   ├── run.rs        # Ejecución QEMU (multi-modo)
│   ├── test_.rs      # Integración con NeoTest
│   ├── clean.rs      # Limpieza de artefactos
│   └── report.rs     # Informes de compilación y prueba
```

## Descubrimiento automático

No existen listas manuales de programas. NeoDev analiza:

- `userbin/*/Cargo.toml` → NXE binaries
- `drivers/*/build_nem.py` → NEM drivers
- `lib*-nxl/Cargo.toml` → NXL shared libraries
- `neodos-kernel/Cargo.toml` → Kernel
- `neodos-bootloader/Cargo.toml` → Bootloader

## Módulos

| Módulo  | Responsabilidad |
|---------|----------------|
| build   | Compilar workspace, detectar proyectos, validar errores, generar informe |
| image   | Crear sistema de archivos NE2, partición ESP FAT32, imagen GPT unificada |
| run     | Ejecutar QEMU con soporte AHCI/ATA/NVMe/VirtIO, SLiRP/TAP/Bridge, KVM/TCG |
| test    | Lanzar QEMU headless, monitorizar serial, analizar resultados de NeoTest |
| clean   | Eliminar artefactos de compilación (target/, *.nxe,*.nxl, *.nem, imágenes) |
| config  | Configuración centralizada en `neodev.toml` |
| report  | Generar informes de compilación y pruebas |

## Configuración

Toda la configuración reside en `config.rs` o en `neodev.toml` en la raíz del proyecto:

- Tamaño de partición ESP (por defecto 100 MB)
- Tamaño de partición NeoDOS (por defecto 10 MB)
- Rutas OVMF (code + vars)
- Memoria QEMU (por defecto 512M)
- Targets Rust (x86_64-unknown-none, x86_64-unknown-uefi)

## Cómo añadir un nuevo módulo

1. Crear `tools/neodev/src/mimodulo.rs`
2. Añadir `pub mod mimodulo;` a `main.rs`
3. Añadir el subcomando a `enum Commands` en `main.rs`
4. Implementar la lógica en `mimodulo.rs`

No es necesario modificar el descubrimiento automático ni las listas de programas.

## Integración con NeoTest

NeoDev lanza QEMU en modo headless, se conecta al monitor vía telnet,
monitoriza el log serial, y analiza los resultados cuando detecta
`ALL_TESTS_COMPLETE` en la salida.

## Scripts sustituidos

| Script original | Sustituido por |
| ---------------- | ---------------- |
| `scripts/build.sh` | `neodev build` |
| `scripts/qemu-debug.sh` | `neodev run` |
| `scripts/qemu-net.sh` | `neodev run --net tap` |
| `scripts/auto_test.py` | `neodev test` |
| `scripts/create_ne2_image.py` | `neodev image` (NE2 portado a Rust) |
| `scripts/create_gpt_image.py` | `neodev image` (GPT portado a Rust) |
| `scripts/clean` (no existía) | `neodev clean` |
