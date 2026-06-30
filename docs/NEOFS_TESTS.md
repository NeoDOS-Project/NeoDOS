# NeoFS — Propuesta de Nuevos Tests (Fase 8)

> Tests propuestos para cubrir los vacíos identificados en la auditoría
> [NEOFS_AUDIT.md](NEOFS_AUDIT.md).

---

## T1: Inode Stress Tests

| Test | Descripción | Tiempo estimado |
|------|-------------|-----------------|
| `inode_create_300` | Crear 300 archivos secuencialmente, verificar que todos existen | 50 líneas |
| `inode_reuse_after_delete` | Crear 256 archivos, borrar 100, crear 100 nuevos — verificar IDs reciclados | 60 líneas |
| `inode_max_limit` | Crear hasta que `find_free_inode()` falle, verificar error `NoInodeAvailable` | 30 líneas |
| `inode_collision_check` | Crear archivo A, verificar que el inode del archivo A coincide al releer | 40 líneas |
| `inode_corruption_detect` | Corromper byte en sector de inode table, verificar que `load_inode()` detecta | 40 líneas |

## T2: Directory / Namespace Tests

| Test | Descripción | Tiempo estimado |
|------|-------------|-----------------|
| `ns_path_long_255` | Path de 255 caracteres (anidamiento profundo), verificar create + lookup | 40 líneas |
| `ns_path_too_long` | Path > 255 caracteres → error `OB_PATH_TOO_LONG` | 20 líneas |
| `ns_deeply_nested_32` | 32 niveles de directorios anidados + archivo hoja, verificar lookup | 50 líneas |
| `ns_entry_corrupted_0xE5` | Simular directory entry con primer byte 0xE5 (deleted), verificar que se salta | 30 líneas |
| `ns_entry_corrupted_bad_len` | Entry con `name_len = 250` (excede límite), verificar que no causa panic | 30 líneas |
| `ns_reserved_name_con` | Intentar crear archivo "CON" o "PRN" → rechazado | 20 líneas |
| `ns_case_insensitive_unicode` | Nombres con mayúsculas/minúsculas mezcladas y acentos | 30 líneas |

## T3: Driver / Namespace Conflict Tests

| Test | Descripción | Tiempo estimado |
|------|-------------|-----------------|
| `driver_ns_register_device` | Driver registra `\Device\TestDev`, verificar entry en namespace | 40 líneas |
| `driver_ns_name_collision` | Dos drivers intentan registrar `\Device\Mismo` — segundo debe fallar | 40 líneas |
| `driver_ns_protected_root` | Intentar `ob_create(Directory, "\Driver")` debe fallar o requerir admin | 30 líneas |
| `driver_ns_protected_global_info` | Intentar crear entry bajo `\Global\Info\` sin admin debe fallar | 30 líneas |
| `driver_ns_hot_unload_cleanup` | Driver registra `\Device\Foo`, se descarga vía hot reload → entry eliminada | 60 líneas |
| `driver_ns_hot_unload_blocks_removed` | Hot unload driver con device → drivers y Ob objects limpiados | 50 líneas |
| `driver_ns_duplicate_name` | Cargar dos drivers con el mismo nombre → error | 30 líneas |
| `driver_ns_cap_required` | Intentar operación de namespace sin cap requerida → denegado | 30 líneas |

## T4: Stress Tests

| Test | Descripción | Tiempo estimado |
|------|-------------|-----------------|
| `fs_stress_create_open_close_delete_10k` | create → open → close → delete × 10000, sin leaks | 40 líneas |
| `fs_stress_concurrent_files` | 10 archivos concurrentes, cada uno write/read 1000 veces | 60 líneas |
| `ns_stress_1000_entries_namespace` | 1000 objetos en namespace, verificar lookup + enum | 50 líneas |
| `fs_stress_long_path_walk` | Path de 10 directorios de profundidad, walk de un archivo en el fondo | 40 líneas |
| `driver_stress_load_unload_cycle` | Cargar driver NEM, descargar, cargar de nuevo × 50 ciclos | 50 líneas |
| `driver_stress_concurrent_load` | 4 CPUs intentan cargar drivers simultáneamente | 40 líneas |

---

## Resumen

| Categoría | Tests | Líneas estimadas |
|-----------|-------|------------------|
| Inode stress | 5 | ~220 |
| Namespace | 7 | ~220 |
| Driver/NS conflict | 8 | ~310 |
| Stress | 6 | ~280 |
| **Total** | **26** | **~1030** |

**Requisito:** Todos los tests deben ejecutarse como parte de
`auto_test.py` (kernel tests registrados en `testing.rs`).

---
