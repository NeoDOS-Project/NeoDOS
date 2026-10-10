# Auditoría integral del sistema NeoDOS

> **Versión:** v1.0  
> **Fecha:** 2026-10-07  
> **Alcance:** Workspace `/home/amartinper/rust-os` (repositorio principal `neodos/`, repos hermanos `neodev/`, `neotools/`, `neodos-dev-server/`)  
> **Kernel:** `neodos-kernel` v0.51.4 · **ABI:** v8 · **SSDT:** 37 syscalls asignados (RAX 0–99, máximo asignado 99)  
> **Método:** Exploración del árbol de código, cruce con `docs/`, `AGENTS.md`, `roadmap/improvements.md`, informes previos (`investigation/audit-code-health-2026-09.md`, `scheduler_audit.md`, informes SMP/netd). **Sin modificaciones de código ni ejecución de `neodev test` en esta pasada.**

---

## 1. Resumen ejecutivo

NeoDOS es un SO x86_64 en Rust con arranque UEFI, kernel monolítico, drivers NEM aislados, Object Manager estilo NT, planificador preemptivo con SMP (work stealing, TLB shootdown), pila de red userland (`netd`, DHCP), NeoFS v2, registro Cm, i18n NLT y ~49 binarios Ring 3. La **calidad de ingeniería del kernel es alta**: invariantes documentados en `docs/architecture/source-of-truth.md`, cientos de tests en kernel (`test_case!` / aserciones distribuidas en subsistemas), trazabilidad forense extensa en `docs/investigation/`, y toolchain externa madura (NeoDev, NeoTools, neodos-dev-server).

Los **riesgos principales** no son “falta de SO”, sino **deuda de escala y coherencia**: tablas de tamaño fijo, colas que pueden perder trabajo en saturación, validación de punteros userland incompleta (`copy_from_user`/`copy_to_user` ausentes), ~230 funciones muertas y duplicación estructural (FSCK, IPI, MMIO, montajes VFS), **SMP>1** con historial de bugs de `iretq`/KPRCB aún activos en memoria del proyecto, y **documentación desincronizada** respecto al renumbering de syscalls (Ob 40–48, Cm 50–59) en varios diseños históricos.

El informe [`reference/audit-report.md`](audit-report.md) sigue siendo la auditoría **de separación de repositorios** (2026-07); la migración ya ocurrió (NeoDev, NeoTools, neodos-dev-server). **Este documento es la auditoría canónica del producto SO** a partir de octubre 2026.

**Evidencia detallada previa:** [`investigation/audit-code-health-2026-09.md`](../investigation/audit-code-health-2026-09.md) (ítems CH-01..CH-17 en `roadmap/improvements.md`).

---

## 2. Fortalezas (con evidencia en código/docs)

| Área | Evidencia |
|------|-----------|
| **Gobernanza arquitectónica** | `docs/architecture/source-of-truth.md` define invariantes MUST/MUST NOT; `neodos-kernel/src/invariants.rs` comprueba anidamiento IRQ y contexto timer; `lock_order.rs` detecta inversiones VFS→PAGE_CACHE→BLOCK_DEVICES (#343). |
| **Object Manager y syscalls** | SSDT validada en boot (`syscall/mod.rs::validate_abi`, 37 entradas); syscalls Ob modularizados en `syscall/ob/`; tipos en `object/types.rs` (22 `ObType`, 39+ clases info). |
| **Tests en kernel** | Registro central `testing.rs::register_tests()`; cobertura por subsistema (scheduler ~90 `test_case!`, Ob, red, CM, drivers, IRP, page cache, etc.). `AGENTS.md` cita 890 tests (valor canónico); el conteo automático de `test_case!` en `.rs` del kernel no coincide exactamente (≈833; diferencia por otros macros de test). |
| **Drivers NEM** | 10 crates en `drivers/` (acpi, ahci, ata, e1000, pci, ps2kbd, ps2mouse, rtc, serial, virtio-blk); runtime en `drivers/driver_runtime/`; especificación `docs/drivers/nem-spec.md`. |
| **Async I/O NT-like** | IRP (`irp/mod.rs`), DPC, work queue, APC; documentado en `docs/kernel/ipc.md`. |
| **Red funcional (con reservas)** | Stack en `neodos-kernel/src/net/`; servicios `userbin/netd`, utilidades `ipconfig`, `ping`; informes de recuperación en `docs/development/net-recovery-2026-09-26.md`. |
| **Userland rico** | 49 proyectos `userbin/*/Cargo.toml` incl. `neoinit`, `neoshell`, `neotop`, `ntpd`, herramientas core `core*`. |
| **libneodos** | Wrappers syscall en `libneodos/` alineados con SSDT; NXL en `libneodos-nxl`, `libconsole-nxl`, `libmath-nxl`, `libnet-nxl`. |
| **Toolchain y repos externos** | NeoDev (build/run/test/imagen), NeoTools (nxeinfo/nxpkg/nxdump — ya no en monorepo), neodos-dev-server (MCP); reflejado en `AGENTS.md` y `docs/README.md`. |
| **Investigación operativa** | >30 informes en `docs/investigation/` (SMP, netd, teclado, scheduler); reduce repetición de debugging. |
| **Seguridad base** | `security/access.rs::se_access_check` usado desde `object/security.rs` en `ob_open_path`; tests en `security/mod.rs`. |
| **Documentación de syscalls actualizada** | `docs/kernel/syscalls.md` refleja reorg v0.50 (37 syscalls, Ob 40–48). |
| **Documentación Ob parcialmente alineada** | `docs/kernel/objects.md` usa `ob_open (RAX=40)` y documenta `PowerState` como pendiente de handler. |

---

## 3. Debilidades y riesgos

### 3.1 Código vs contrato público

| Riesgo | Evidencia |
|--------|-----------|
| **`ObInfoClass::PowerState` sin implementar** | Declarado en `object/types.rs:152`; **no** aparece en `syscall/ob/query/`; documentado como pendiente en `docs/kernel/objects.md:220` y `docs/kernel/syscalls.md:403`. |
| **Huecos ABI en enums Ob** | Gaps numéricos en `ObInfoClass`/`ObSetInfoClass`; sin test de completitud (CH-06). |
| **Placeholder Ob create process** | `syscall/ob/create_process.rs` — stub `NoSys` (CH-02). |
| **~230 funciones sin referencias** | Inventario en `audit-code-health-2026-09.md` §2; incluye paths de seguridad/token no usados, wake socket, URN, hotreload. |
| **Duplicación estructural** | FSCK dual, IPI×3, MMIO/port I/O, ACPI RSDP, dos tablas de montaje (CH-03..CH-05, CH-16). |
| **Pérdida silenciosa de trabajo** | Colas acotadas IRP/DPC/work queue/hotreload (CH-12). |
| **Copia userland frágil** | No hay `copy_from_user`/`copy_to_user` en el kernel; validación ad hoc en handlers (CH-13). |
| **Límites fijos** | MAX_DRIVERS, MAX_SOCKETS, MAX_TCP, MAX_PIPES, etc. (CH-14) — riesgo de agotamiento en cargas reales. |
| **Contención global** | Mutex scheduler, Ob table O(n), locks VFS/page cache (CH-15). |

### 3.2 Scheduler y SMP

| Riesgo | Evidencia |
|--------|-----------|
| **Runqueue: duplicados y entradas obsoletas** | `docs/scheduler_audit.md` P0-3 — `enqueue_to_cpu_run_queue` sin dedup; fallback O(n) global en `schedule()`. |
| **Historial SMP crítico** | Informes #331, #346, #348, #474, #476, #488 en `docs/investigation/`; rama local `fix/scheduler-critical-invariants` indica trabajo activo. |
| **SMP feature flag** | `Cargo.toml` feature `smp-ap-sched` (Phase 13 AP scheduling) — capacidad avanzada pero aumenta superficie de bugs. |

### 3.3 Seguridad y confianza

| Riesgo | Evidencia |
|--------|-----------|
| **Modelo NT incompleto en runtime** | `design/users-security-design.md` planifica sesiones; SAM limitado (`security/sam.rs`, max 64 entradas); privilegios en token poco usados fuera de tests. |
| **Sin ASLR / espacio user pequeño** | Aún citado en `docs/architecture/vision.md` §1 debilidades. |
| **Admin bypass** | Tokens admin en paths de Ob — correcto para bootstrap, riesgo si se expande userland sin DACL consistente. |

### 3.4 Tooling (neodos-dev-server)

| Riesgo | Evidencia |
|--------|-----------|
| **MCP `check_consistency` no valida** | `audit-code-health-2026-09.md` §1 — `targets` ignorado. |
| **Rutas de artefactos incorrectas** | `get_build_errors` / `list_loaded_modules` buscan paths distintos a `neodos/kernel.elf`. |
| **Metadatos Ob obsoletos en MCP** | “16 types / 7 syscalls” vs código real (CH-10). |

### 3.5 Documentación desfasada (no exhaustivo)

| Documento | Problema |
|-----------|----------|
| `docs/kernel/obj-arch.md` | Syscalls Ob **RAX 60–66** (histórico); migración completada a 40–48. |
| `docs/design/neocfg-design.md`, `docs/design/users-security-design.md` | Referencias RAX 60–66. |
| `docs/architecture/vision.md` | Afirma “Sin gestión de energía” mientras existen `power/`, `docs/services/power-manager.md`, `userbin/poweroff`, `reboot`. |
| `docs/architecture/repository.md` | “28 user binaries”, `tools/neodev` dentro de monorepo — **obsoleto** post-migración. |
| `reference/audit-report.md` | Acción “migrar NeoTools/LSP/MCP inmediatamente” — **hecho** para NeoTools y MCP; LSP no presente en tree `neodos/`. |
| `docs/architecture/vision.md` | Fecha 2026-06; varios diagnósticos no actualizados (tests 825 sí en header de `docs/README.md`). |
| `docs/filesystem/vfs-patterns.md` | Enlaces rotos (CH-09). |
| Docs secundarios | Algunos paths de fuente renombrados (`syscall/ob.rs` → directorio, `cm/hive.rs` → directorio) aún citados en diseños viejos. |

**Parcialmente corregido desde sept-2026:** renumbering syscalls en `objects.md`, `syscalls.md`, `registry/registry.md` (según grep 2026-10-07); persisten diseños históricos listados arriba.

---

## 4. Mapa de madurez por subsistema

Escala **0–5** (0=inexistente, 5=producción estable): juicio cualitativo basado en código, tests e informes.

| Subsistema | Nivel | Notas |
|------------|-------|-------|
| Boot / UEFI | 4 | `neodos-bootloader/`, `docs/boot/boot-flow.md`, handoff BootInfo |
| HAL / interrupciones | 4 | IOAPIC, MSI, IRQL; deuda HAL→timers (#436–438) documentada en SoT |
| Scheduler (UP) | 4 | Tests extensos; invariantes `make_thread_ready` en progreso |
| Scheduler (SMP) | 3 | Funcional con flags; historial GPF/deadlock; Phase 13 activa |
| Memoria / paging | 3 | Buddy/slab; límites 4 GB bitmap; sin ASLR |
| Object Manager | 4 | Centro del diseño; gap PowerState; ObOperations fino |
| VFS / NeoFS v2 | 4 | B-tree, page cache, FSCK dual (deuda) |
| Drivers NEM | 4 | 10 drivers; certificación KCR; ABI v8 no congelada |
| Red (kernel + netd) | 3 | TCP/IP, DHCP; informes de carreras init RX |
| IPC / IRP / servicios | 3–4 | Service manager; lock-order bugs corregidos con guardias |
| Seguridad (SID/ACL) | 3 | Implementado; sesiones/login no completos |
| Registry (Cm) | 4 | Syscalls 50–59, hive, tests |
| Power / ACPI | 3 | Código en `power/`; Ob PowerState query pendiente |
| Consola / VT / input | 4 | 4 VTs; pipeline kbd investigado |
| i18n (NLT) | 3–4 | `data/locale/`, `tools/nltc`, runtime libneodos |
| Userland / shell | 4 | neoshell, neoinit, utilidades core |
| Paquetes (.NXP) | 2–3 | Diseño en docs; madurez menor que NXE |
| Tooling dev | 4 | NeoDev + NeoTools + MCP (con bugs de paths) |
| Documentación técnica | 3 | Mucho volumen; drift en diseños y visión |
| CI / calidad repo | 3–4 | `neodev check-deps`, markdownlint en AGENTS; benchmarks planeados (PERF-BENCH-Suite) |

---

## 5. Prioridades recomendadas

| Prioridad | Tema | Referencia |
|-----------|------|------------|
| **P0** | Estabilidad SMP + invariantes scheduler (runqueue, frames KPRCB) | `scheduler_audit.md`, rama `fix/scheduler-critical-invariants`, issues #346–#488 |
| **P1** | Seguridad userland (`copy_from_user`/`copy_to_user`) | CH-13 |
| **P1** | Completar API Ob documentada (`PowerState` query + test completitud enums) | CH-06 |
| **P1** | Eliminar pérdida silenciosa en colas IRP/DPC/work | CH-12 |
| **P2** | Barrido documentación: syscalls, paths, visión energía, repo arch | CH-07, CH-08; este informe §3.5 |
| **P2** | Reparar neodos-dev-server (paths, consistency, conteos Ob) | CH-10 |
| **P2** | Consolidación deuda duplicada (FSCK, IPI, mount tables) | CH-03..CH-05, CH-16 |
| **P3** | Dead code sweep | CH-01 |
| **P3** | Escalabilidad tablas fijas / contención locks | CH-14, CH-15 |
| **Post-v1.0** | Congelar ABI, NeoDrivers repo, NeoSDK | `vision.md` §8, `audit-report.md` |

---

## 6. Relación con otros documentos

| Documento | Rol |
|-----------|-----|
| **Este informe** | Auditoría SO integral (estado oct-2026) |
| [`audit-report.md`](audit-report.md) | Auditoría separación repos (histórica, migración ejecutada) |
| [`../investigation/audit-code-health-2026-09.md`](../investigation/audit-code-health-2026-09.md) | Evidencia código muerto, docs, MCP |
| [`../scheduler_audit.md`](../scheduler_audit.md) | Invariantes runqueue P0 |
| [`../architecture/source-of-truth.md`](../architecture/source-of-truth.md) | Invariantes normativas |
| [`../../roadmap/improvements.md`](../../roadmap/improvements.md) | Backlog CH-* y milestones GitHub |

---

## 7. Canvas interactivo

Visualización resumida (madurez, riesgos, docs): abrir el canvas en el IDE:

`/home/amartinper/.cursor/projects/home-amartinper-rust-os/canvases/neodos-system-audit-2026-10-07.canvas.tsx`

---

*Auditoría de lectura; no sustituye `neodev test` ni validación en QEMU/VirtualBox. Actualizar este informe tras hitos mayores (congelación ABI, cierre milestone v0.66, resolución SMP Phase 13).*
