# NeoDev — NeoDOS Development Tool

Una única herramienta Rust que centraliza compilación, generación de imágenes,
ejecución de máquinas virtuales y pruebas automatizadas para NeoDOS.

Soporta múltiples hipervisores mediante una arquitectura de backends.

## Uso

```text
neodev build                      Compilar todos los componentes
neodev build --quick              Solo kernel + bootloader
neodev build --image              Compilar y generar imagen de disco
neodev image                      Generar imagen de disco (NE2 + ESP + GPT)
neodev run                        Ejecutar NeoDOS en QEMU
neodev run --backend virtualbox   Ejecutar en VirtualBox
neodev run --kvm --gdb            Con KVM y GDB (QEMU)
neodev run --storage virtio       Almacenamiento VirtIO
neodev test                       Ejecutar tests en QEMU
neodev test --backend virtualbox  Ejecutar tests en VirtualBox
neodev test --iterations 5        Ejecutar 5 iteraciones
neodev vm start                   Iniciar VM (backend por defecto)
neodev vm stop                    Detener VM
neodev vm reset                   Reiniciar VM
neodev vm status                  Estado de la VM
neodev vm create                  Crear VM
neodev vm delete                  Eliminar VM
neodev clean                      Limpiar artefactos
neodev list                       Listar proyectos descubiertos
neodev config                     Mostrar configuración
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
│   ├── run.rs        # Punto de entrada para ejecución de VM
│   ├── test_.rs      # Integración con NeoTest (backend-agnostic)
│   ├── clean.rs      # Limpieza de artefactos
│   ├── report.rs     # Informes de compilación y prueba
│   └── vmm/          # Virtual Machine Manager
│       ├── mod.rs    # HypervisorBackend trait, VmConfig, factory
│       ├── qemu.rs   # QEMU backend
│       └── vbox.rs   # VirtualBox backend
```

## Backends de Hipervisor

NeoDev abstrae el hipervisor mediante el trait `HypervisorBackend`:

| Backend     | Comando                     | Requisitos                  |
|-------------|-----------------------------|-----------------------------|
| QEMU        | `neodev run` (por defecto)  | `qemu-system-x86_64`, OVMF  |
| VirtualBox  | `neodev run --backend virtualbox` | `VBoxManage`         |

Selección del backend:
- Por defecto: configurado en `neodev.toml` (`[vm] backend = "qemu"`)
- Por línea de comandos: `--backend qemu|virtualbox`

## Módulos

| Módulo  | Responsabilidad |
|---------|----------------|
| vmm     | Gestión de máquinas virtuales: trait `HypervisorBackend`, backends QEMU y VirtualBox |
| build   | Compilar workspace, detectar proyectos, validar errores, generar informe |
| image   | Crear sistema de archivos NE2, partición ESP FAT32, imagen GPT unificada |
| run     | Ejecutar VM con el backend seleccionado |
| test    | Lanzar VM headless, monitorizar serial, analizar resultados de NeoTest |
| clean   | Eliminar artefactos de compilación (target/, *.nxe,*.nxl, *.nem, imágenes, VDI) |
| config  | Configuración centralizada en `neodev.toml` |
| report  | Generar informes de compilación y pruebas |

## Configuración

Toda la configuración reside en `neodev.toml` en la raíz del proyecto:

```toml
[project]
kernel_target = "x86_64-unknown-none"
bootloader_target = "x86_64-unknown-uefi"
esp_size_mb = 100
neodos_size_mb = 10

[vm]
backend = "qemu"          # o "virtualbox"
memory = 512              # MB
cpus = 2

[vm.qemu]
kvm = false
bdm = false
ovmf_code = "/usr/share/OVMF/OVMF_CODE.fd"
ovmf_vars_template = "/usr/share/OVMF/OVMF_VARS.fd"
```

## Comandos VM

```text
neodev vm start [--backend qemu|virtualbox] [--headless]
neodev vm stop [--backend qemu|virtualbox]
neodev vm reset [--backend qemu|virtualbox]
neodev vm status [--backend qemu|virtualbox]
neodev vm create [--backend qemu|virtualbox]
neodev vm delete [--backend qemu|virtualbox]
```

## Cómo añadir un nuevo backend

1. Crear `tools/neodev/src/vmm/mibackend.rs`
2. Implementar `HypervisorBackend` trait
3. Añadir `mod mibackend;` a `tools/neodev/src/vmm/mod.rs`
4. Añadir `"mibackend" => Ok(Box::new(mibackend::MiBackend))` en `create_backend()`

No es necesario modificar el CLI, run.rs, test_.rs ni la configuración.

## Scripts sustituidos

| Script original | Sustituido por |
| ---------------- | ---------------- |
| `scripts/build.sh` | `neodev build` |
| `scripts/qemu-debug.sh` | `neodev run` |
| `scripts/qemu-net.sh` | `neodev run --net tap` |
| `scripts/auto_test.py` | `neodev test` |
| `scripts/create_ne2_image.py` | `neodev image` |
| `scripts/create_gpt_image.py` | `neodev image` |
| `scripts/vbox-setup.sh` | `neodev run --backend virtualbox` / `neodev vm create --backend virtualbox` |

## Integración con NeoTest

NeoDev lanza la VM headless con el backend configurado, monitoriza el log
serial, y analiza los resultados cuando detecta `ALL_TESTS_COMPLETE` en la
salida. Funciona con QEMO y VirtualBox.
