# Power Manager — Design Document

> **Versión:** v0.2  
> **Estado:** Implementado (Fase 1 — migración a Object Manager)  
> **Versión de NeoDOS:** v0.49.1+  

---

## 1. Research

### 1.1 Auditoría de código existente

#### Kernel (`neodos-kernel/src/`)

| Archivo | Relevancia |
|---------|------------|
| `object/power.rs` | `PowerManager` Ob object at `\System\PowerManager`, `ObType::PowerManager(21)`. Shutdown/reboot via `ob_set_info(PowerShutdown=37/PowerReboot=38)`. |
| `hal/x64/cpu.rs` | `poweroff()` — QEMU debug ports (0x404, 0x604, 0xB004, 0x4004) + PS/2 reset. `reboot()` — new: 0xCF9 reset + PS/2 reset. |
| `syscall/handlers.rs` | `handler_poweroff` **removed** (was at lines 229-241). Power management via Ob API. |
| `syscall/mod.rs` | `t[42]` **removed**. Power management via `handler_ob_set_info` (RAX=63) with `PowerShutdown`/`PowerReboot`. |
| `timers/hpet.rs` | ACPI table scanner: RSDP → RSDT/XSDT. Finds HPET, MCFG, MADT tables. **No FADT, no DSDT, no S5.** |
| `watchdog/mod.rs:238` | `watchdog_reset_system()` — now calls `crate::object::power::power_reboot()` (correctly uses reboot path). |
| `arch/x64/idt.rs` | Ctrl+Alt+Del now calls `crate::object::power::power_shutdown()` (flush + event dispatch + hal). |
| `services/mod.rs` | Service Manager: 5-state machine, Registry-backed, ObType::Service (20). `SERVICE_MANAGER` global. |
| `eventbus/mod.rs:33` | `EVENT_SHUTDOWN = 12` (frozen). No `EVENT_SUSPEND`, `EVENT_HIBERNATE`, `EVENT_POWER_BUTTON`, `EVENT_LID_CLOSE`. |
| `cm/mod.rs` | Registry subsystem: cell-based hive, persistent to `C:\System\Registry\SYSTEM.hiv`. Used by Services, Networking. |
| `main.rs` | PHASE 2.765: `object::power::init_power_manager()` — registers `\System\PowerManager`. `\System` added to namespace dirs. | 

#### HAL

| Primitivo | Estado |
|-----------|--------|
| `poweroff()` ✅ | QEMU debug ports + PS/2 reset. Also available via PowerManager Ob object. |
| `reboot()` ✅ | `outb(0xCF9, 0x06)` + PS/2 reset. Available via PowerManager Ob object. |
| `acpi_fadt()` ❌ | No se parsea FADT. |
| `acpi_s5_write()` ❌ | No existe. |

#### Object Manager

- 18 ObTypes (0–20), Service(20) es el último.
- `ob_query_info`: classes 0–31 (27 query classes).
- `ob_set_info`: classes 0–36 (33 set classes — 33=ServiceStart, 34=ServiceStop, 35=ServiceRestart, 36=ServiceSetConfig).
- `sys_ob_service` (RAX=77): control de servicios via handle.

#### libneodos

- `sys_poweroff()` (RAX=42): wrapper no-return.
- No hay `sys_reboot()`, `sys_suspend()`, wrappers de plan de energía.

#### Shell

- `POWEROFF`: built-in, llama `sys_poweroff`.
- No `REBOOT`, `SHUTDOWN`, `SUSPEND` como comandos separados.

---

### 1.2 Diagnóstico: qué existe vs. qué falta

| Funcionalidad | Existe | Dónde |
|--------------|--------|-------|
| Apagado básico | ✅ | `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerShutdown)` — via Object Manager |
| Reboot | ✅ | `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerReboot)` — via Object Manager |
| Reinicio básico | ❌ | No hay syscall dedicada |
| ACPI FADT parse | ❌ | No existe |
| ACPI S5 (sleep type) | ❌ | No existe |
| Planes de energía | ❌ | No existe |
| Configuración persistente | ✅ | Registry (Cm) — usado por Services, Network |
| Coordinación con procesos | ❌ | Apagado actual es inmediato |
| Suspensión/Hibernación | ❌ | No existe |
| Eventos de power (botón, tapa) | ❌ | No existen |
| API de power en libneodos | ❌ | Solo `sys_poweroff()` |
| HAL primitives para power | ❌ | Solo `poweroff()` bruto |

---

## 2. Problem Analysis

### Current limitations

1. ~~**`sys_poweroff` es una syscall directa (RAX=42)**, no integrada en el Object Manager.~~ ✅ **CORREGIDO**: Power management via `ob_open(\\System\\PowerManager)` + `ob_set_info(PowerShutdown/Reboot)` (RAX=63). Syscall 42 eliminada.

2. **API incompleta.** Shutdown y reboot implementados. Falta `suspend()`, `hibernate()`, consulta de estado.

3. ~~**Sin reinicio.** `sys_poweroff` solo apaga.~~ ✅ **CORREGIDO**: `reboot()` en HAL + `power_reboot()` en PowerManager. Watchdog usa correctamente reboot.

4. **Sin ACPI real.** La `poweroff()` actual usa puertos QEMU específicos. En hardware real sin QEMU, solo funciona el PS/2 reset (`outb(0x64, 0xFE)`) que no apaga limpiamente. El ACPI PM1a register para S5 no se usa.

5. **Sin planes de energía.** No hay perfiles (Balanced, Performance, Power Saver). El sistema siempre opera al mismo nivel.

6. **Sin eventos de power.** No se detecta botón de encendido, cierre de tapa, batería baja. `EVENT_SHUTDOWN` existe pero solo se dispara desde `handler_poweroff`.

7. **Sin API para aplicaciones.** Neoshell, NeoCfg, y futura GUI no tienen forma de consultar estado de energía ni cambiarlo más allá del apagado total.

### Why existing abstractions can't solve it

- **Service Manager** maneja procesos de usuario pero no tiene acceso al hardware de power (ACPI, QEMU ports).
- **Event Bus** tiene `EVENT_SHUTDOWN` pero no es un gestor: solo notifica.
- **HAL `poweroff()`** es una terminal function (`-> !`). No puede coordinarse con otros subsistemas.
- **Registry** almacena configuración pero no ejecuta políticas.

Se necesita un **nuevo subsistema** que orqueste la coordinación multiplano (kernel → servicios → drivers → HAL) y exponga una API unificada.

---

## 3. Solution Design

### 3.1 Architecture Decision: Kernel component, not a service

El Power Manager debe ser un **subsistema del kernel**, no un servicio Ring 3, por:

1. **Acceso a hardware privilegiado**: ACPI PM1a, reset register, QEMU debug ports.
2. **Timing crítico**: en shutdown, el kernel debe detener otros CPUs (IPI), esperar IRP completions, y asegurar que ningún driver acceda a hardware después del poweroff.
3. **Disponibilidad**: debe funcionar incluso cuando el Service Manager no responda.
4. **Atomicidad**: shutdown/reboot son irreversibles y no deben depender de procesos de usuario.

### 3.2 Architectural overview

```
                     User applications (Ring 3)
                     ┌──────────────────────────┐
                     │  libneodos API            │
                     │  power_get_active_plan()  │
                     │  power_set_active_plan()  │
                     │  power_shutdown()          │
                     │  power_reboot()            │
                     └──────────┬───────────────┘
                                │ ob_open / ob_set_info / ob_query_info
                     ┌──────────▼───────────────┐
                     │  Object Manager           │
                     │  \Device\PowerManager     │
                     │  ObType::PowerManager(21) │
                     └──────────┬───────────────┘
                                │ internal calls
                     ┌──────────▼───────────────┐
                     │  Power Manager (kernel)   │
                     │  src/power/               │
                     │  ─────────────────────    │
                     │  PowerManager struct      │
                     │  ActivePlan               │
                     │  PolicyEngine             │
                     │  PowerCoordinator         │
                     │  ACPIHelper               │
                     └──┬───────────┬───────────┘
                        │           │
               ┌────────▼──┐  ┌────▼────────┐
               │  HAL ABI  │  │  Registry    │
               │ poweroff  │  │  (Cm)        │
               │ reboot    │  │  \Registry\  │
               │ acpi_fadt │  │  Machine\   │
               │ s5_write  │  │  System\    │
               └───────────┘  │  Power\*    │
                              └─────────────┘
```

### 3.3 New types/structs/enums

#### `ObType::PowerManager = 21`

New variant in `src/object/types.rs`.

```rust
PowerManager = 21,  // Power Manager singleton object
```

The singleton lives at `\Device\PowerManager` in the Ob namespace, created at boot (Phase 3.883, after Service Manager).

#### `src/power/mod.rs` — new module

```rust
pub struct PowerManager {
    state: PowerSystemState,
    active_plan: PowerPlan,
    plans: [PowerPlan; 3],  // Balanced, Performance, PowerSaver
    capabilities: PowerCapabilities,
    acpi: AcpiPowerState,
}

pub enum PowerSystemState {
    Active,
    ShuttingDown,
    Rebooting,
    Suspending,
    Hibernating,
    Off,
}

pub struct PowerPlan {
    name: PowerPlanName,
    policies: PowerPolicies,
    flags: u32,
}

pub enum PowerPlanName {
    Balanced = 0,
    Performance = 1,
    PowerSaver = 2,
}

pub struct PowerPolicies {
    pub display_timeout_sec: u32,
    pub sleep_timeout_sec: u32,
    pub hibernate_enabled: bool,
    pub cpu_policy: CpuPolicy,
    pub lid_action: PowerAction,
    pub power_button_action: PowerAction,
}

pub enum CpuPolicy {
    Balanced,
    Performance,
    PowerSave,
}

pub enum PowerAction {
    None,
    Sleep,
    Hibernate,
    Shutdown,
    Reboot,
}

pub struct PowerCapabilities {
    pub supports_s3: bool,
    pub supports_s4: bool,
    pub supports_s5: bool,
    pub has_battery: bool,
    pub has_lid: bool,
    pub has_power_button: bool,
}

pub struct AcpiPowerState {
    pub fadt_valid: bool,
    pub pm1a_evt_blk: u64,
    pub pm1a_ctrl_blk: u64,
    pub pm1b_ctrl_blk: u64,
    pub s5_slp_typa: u8,
    pub s5_slp_typb: u8,
    pub reset_reg: GenericAddr,
    pub reset_value: u8,
}
```

#### Event Bus new types

```rust
// New frozen-compatible event types (extending past 17)
pub const EVENT_SHUTDOWN_PHASE2: EventType = 19;   // All services stopped, drivers notified
pub const EVENT_SUSPEND: EventType = 20;            // System entering suspend
pub const EVENT_RESUME: EventType = 21;              // System resumed from suspend
pub const EVENT_POWER_BUTTON: EventType = 22;        // Power button pressed
pub const EVENT_LID_CLOSE: EventType = 23;           // Lid closed
pub const EVENT_LID_OPEN: EventType = 24;            // Lid opened
pub const EVENT_BATTERY_LOW: EventType = 25;         // Battery low (future)
pub const EVENT_POWER_SOURCE_CHANGE: EventType = 26; // AC ↔ battery switch (future)
```

### 3.4 New info classes for `ob_query_info` / `ob_set_info`

#### New ObQueryInfoClass variants (power):

| Class | Name | Description |
|-------|------|-------------|
| 32 | PowerPlanInfo | Get active plan info: name, policies |
| 33 | PowerStatus | Get system power state: state, capabilities |
| 34 | PowerSystemState | Get overall system power state enum |

#### New ObSetInfoClass variants (power):

| Class | Name | Description |
|-------|------|-------------|
| 37 | PowerShutdown | Initiate coordinated shutdown |
| 38 | PowerReboot | Initiate coordinated reboot |
| 39 | PowerSuspend | Initiate S3 suspend (future) |
| 40 | PowerHibernate | Initiate S4 hibernate (future) |
| 41 | PowerSetPlan | Switch active power plan |
| 42 | PowerSetPolicy | Modify a policy value in the active plan |

These extend the existing class tables in `src/syscall/ob.rs`.

### 3.5 New files/modules

| Path | Responsibility |
|------|----------------|
| `src/power/mod.rs` | `PowerManager` struct, `POWER_MANAGER` global, initialization |
| `src/power/plan.rs` | `PowerPlan`, `PowerPolicies`, serialization/deserialization to/from Registry |
| `src/power/coordinator.rs` | Shutdown/reboot coordination: notify services, drivers, flush, halt |
| `src/power/acpi.rs` | ACPI FADT parsing, PM1a S5 write, reset register support |
| `src/power/event.rs` | Event handlers for `EVENT_POWER_BUTTON`, `EVENT_LID_CLOSE`, etc. |

### 3.6 Changes to existing files

| File | Change |
|------|--------|
| `src/object/types.rs` | Add `PowerManager = 21` to `ObType` enum |
| `src/hal/x64/cpu.rs` | Add `reboot()` → `!` and `acpi_s5_write()` extern "C" functions |
| `src/hal/x64/mod.rs` | Export new HAL primitives |
| `src/hal/mod.rs` | Re-export `reboot`, `acpi_s5_write` |
| `src/syscall/mod.rs` | Update `MAX_VALID` and `ASSIGNED` arrays |
| `src/syscall/ob.rs` | Add dispatch for new info classes 32–34 and 37–42 in Power Manager handle |
| `src/eventbus/mod.rs` | Add new event types 19–26 |
| `src/abi_freeze.rs` | Update frozen checks for new event types |
| `main.rs` | Add PHASE 3.883 for Power Manager init (after Service Manager) |
| `libneodos/src/syscall.rs` | Add `sys_reboot()`, `power_get_active_plan()`, `power_set_active_plan()`, `power_shutdown()` wrappers |
| `userbin/neoshell/` | Add built-in `REBOOT`, update `POWEROFF` to use Power Manager |
| `src/cm/mod.rs` | Ensure `cm_ensure_default_values()` creates `\Registry\Machine\System\Power\*` keys |
| `docs/syscalls.md` | Document new info classes |
| `docs/objects.md` | Document new ObType and namespace entry |
| `docs/power-manager.md` | This document |

### 3.7 Power coordination flow (shutdown example)

```
1. User app calls power_shutdown()
2. libneodos: ob_open("\Device\PowerManager") → fd
3. libneodos: ob_set_info(fd, PowerShutdown, NULL, 0) → ! (doesn't return)
4. Kernel handler:
   a. Transition state: Active → ShuttingDown
   b. Notify Event Bus: EVENT_SHUTDOWN (type 12) → dispatch_pending()
   c. Stop all services via ServiceManager::stop_all()
   d. Notify Event Bus: EVENT_SHUTDOWN_PHASE2 (type 19)
   e. Flush registry hives (cm_flush_all_hives)
   f. Flush block/page cache
   g. Stop secondary CPUs (IPI_HALT)
   h. Disable interrupts
   i. HAL::acpi_s5_write()  // Write SLP_TYP to PM1a
   j. Halt (fallback: QEMU debug ports, then PS/2)
```

### 3.8 Registry structure

Persistent configuration at `\Registry\Machine\System\Power\`:

```
\Registry\Machine\System\Power
├── ActivePlan (REG_DWORD) = 0  // 0=Balanced, 1=Performance, 2=PowerSaver
├── LidAction (REG_DWORD) = 1   // PowerAction enum
├── PowerButtonAction (REG_DWORD) = 3  // Shutdown
├── HibernateEnabled (REG_DWORD) = 0
├── Plans\
│   ├── Balanced\
│   │   ├── DisplayTimeout (REG_DWORD) = 300  // seconds
│   │   ├── SleepTimeout (REG_DWORD) = 1800
│   │   ├── CpuPolicy (REG_DWORD) = 0
│   │   └── Description (REG_SZ) = "Balanced power plan"
│   ├── Performance\
│   │   ├── DisplayTimeout = 600
│   │   ├── SleepTimeout = 0  // never
│   │   ├── CpuPolicy = 1
│   │   └── Description = "High performance"
│   └── PowerSaver\
│       ├── DisplayTimeout = 60
│       ├── SleepTimeout = 300
│       ├── CpuPolicy = 2
│       └── Description = "Power saver"
```

All values created by `cm_ensure_default_values()` at boot if `ActivePlan` key doesn't exist.

### 3.9 libneodos API

```rust
// Power Manager wrapper
pub fn power_shutdown() -> !;
pub fn power_reboot() -> !;
pub fn power_suspend() -> Result<(), i64>;
pub fn power_hibernate() -> Result<(), i64>;
pub fn power_get_active_plan() -> Result<PowerPlanInfo, i64>;
pub fn power_set_active_plan(plan: PowerPlanName) -> Result<(), i64>;
pub fn power_set_policy(policy: PowerPolicy, value: u32) -> Result<(), i64>;

// Data types
#[repr(C)]
pub struct PowerPlanInfo {
    pub active_plan: u32,
    pub display_timeout: u32,
    pub sleep_timeout: u32,
    pub hibernate_enabled: u8,
    pub lid_action: u32,
    pub power_button_action: u32,
}

#[repr(C)]
pub struct PowerSystemStatus {
    pub state: u32,          // PowerSystemState enum
    pub capabilities: u32,   // PowerCapabilities bitmask
}
```

---

## 4. Alternatives

### Alternative A: Ring 3 Power Manager service

**Descripción:** El Power Manager se ejecuta como un servicio Ring 3 (ObType::Service), con acceso al hardware via syscalls existentes (`outb`, MMIO mapping). Se comunica con el kernel solo para operaciones que requieren Ring 0 (registries flush, CPU halt).

**Rechazada porque:**
1. **Latencia:** shutdown/reboot requieren ejecución atómica al final. Un servicio Ring 3 puede ser killado o no responder.
2. **Complejidad de seguridad:** requeriría escalar capacidades (CAP_PORTIO, CAP_MMIO) a un proceso de usuario, abriendo superficie de ataque.
3. **Sincronización:** apagar otros CPUs (IPI) y detener el scheduler solo puede hacerse desde Ring 0.
4. **Persistencia:** el servicio debe poder invocarse incluso si el Service Manager está en estado Failed.
5. **Precedente:** en NT, el Power Manager es parte del kernel (ntoskrnl.exe), no un servicio.

### Alternative B: Extender `sys_poweroff` con flags

**Descripción:** Modificar `sys_poweroff (RAX=42)` para aceptar flags: `0=shutdown`, `1=reboot`, `2=suspend`. Añadir configuración via Registry pero sin objeto en el namespace.

**Rechazada porque:**
1. Viola la regla arquitectónica: toda nueva funcionalidad debe pasar por Ob (`RAX ≥ 77 → sys_ob_*`).
2. No escala a consultas de planes, políticas, ni eventos de power.
3. No permite que aplicaciones consulten el estado sin abrir un objeto.
4. Una syscall con múltiples comportamientos es menos extensible que objetos + info classes.

### Alternative C: Fold everything into Service Manager

**Descripción:** El Service Manager existente absorbe las responsabilidades de power: `SERVICE_CONTROL_SHUTDOWN`, `SERVICE_CONTROL_REBOOT`.

**Rechazada porque:**
1. Mezcla dominios: servicios son procesos de usuario, power es infraestructura de kernel.
2. Service Manager podría estar siendo shutdown él mismo.
3. No tiene acceso HAL natural.
4. No resuelve la necesidad de planes de energía ni eventos ACPI.

---

## 5. Affected Components

| Subsystem | Impact | Details |
|-----------|--------|---------|
| **Object Manager** | Medium | Add `ObType::PowerManager(21)`, register singleton at boot |
| **HAL** | High | Add `reboot()`, `acpi_fadt_parse()`, `acpi_s5_write()` ABI functions |
| **Syscall dispatch** | Medium | New info classes 32–34, 37–42 in existing `ob_query_info`/`ob_set_info` |
| **Registry (Cm)** | Low | Add default `\Registry\Machine\System\Power\*` keys |
| **Service Manager** | Medium | Add `stop_all()` method for coordinated shutdown |
| **Scheduler** | Low | IPI to stop secondary CPUs during shutdown sequence |
| **Event Bus** | Low | Add event types 19–26 |
| **ABI freeze** | Low | Validate new event types in `abi_freeze.rs` |
| **Boot sequence** | Low | Add PHASE 3.883 in `main.rs` |
| **libneodos** | Medium | Add power wrappers and data types |
| **Neoshell** | Low | Add `REBOOT` built-in, update `POWEROFF` path |
| **NeoCfg** | Low | Future: power plan management UI |
| **Watchdog** | Low | Update `watchdog_reset_system()` to use `PowerManager::reboot()` |
| **Crash dump** | Low | Ensure crash dump path still works before `poweroff()` |
| **Documentation** | Medium | Update `docs/syscalls.md`, `docs/objects.md`, create `docs/power-manager.md` |

---

## 6. API Contract

### 6.1 `ob_set_info(PowerShutdown = 37)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle to PowerManager object, `class` = 37, `buf`/`size` = unused.
- **Returns:** Does not return (system halts).
- **Error codes:** Does not return on success. On pre-check failure: `-Perm` (no admin), `-Busy` (shutdown already in progress).
- **Preconditions:** Caller token must be admin. System state must be `Active`. All cached writes are flushed before poweroff.
- **Sequence:** See 3.7.

### 6.2 `ob_set_info(PowerReboot = 38)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle to PowerManager object, `class` = 38, `buf`/`size` = unused.
- **Returns:** Does not return.
- **Error codes:** Same as PowerShutdown.
- **Preconditions:** Same as PowerShutdown.
- **Sequence:** Same as shutdown but ends with `HAL::reboot()` instead of `poweroff()`.

### 6.3 `ob_set_info(PowerSuspend = 39)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 39.
- **Returns:** 0 on success (resumed), `-NotSupported` if S3 not available, `-Perm` (no admin).
- **Preconditions:** System must support S3 (`capabilities.supports_s3`). Must be running on ACPI-capable hardware.

### 6.4 `ob_set_info(PowerHibernate = 40)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 40.
- **Returns:** 0 on success, `-NotSupported` if S4 not available, `-Perm` (no admin).
- **Preconditions:** System must support S4. Hibernate file must exist (future).

### 6.5 `ob_set_info(PowerSetPlan = 41)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 41, `buf` = pointer to u32 (0=Balanced, 1=Performance, 2=PowerSaver), `size` = 4.
- **Returns:** 0 on success, `-Inval` for invalid plan index, `-Perm` (no admin).
- **Preconditions:** None.
- **Side effects:** Writes `ActivePlan` to Registry. Applies plan policies immediately.

### 6.6 `ob_set_info(PowerSetPolicy = 42)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 42, `buf` = `PowerPolicyUpdate` struct, `size` = 12.
  ```rust
  #[repr(C)]
  pub struct PowerPolicyUpdate {
      pub policy_id: u32,  // 0=DisplayTimeout, 1=SleepTimeout, 2=LidAction, etc.
      pub value: u64,
  }
  ```
- **Returns:** 0 on success, `-Inval` for unknown policy_id, `-Perm` (no admin).
- **Preconditions:** None.

### 6.7 `ob_query_info(PowerPlanInfo = 32)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 32, `buf` = `&mut PowerPlanInfo` (output), `size` = sizeof(PowerPlanInfo).
- **Returns:** Bytes written on success, `-Fault` if buffer too small.
- **No admin required:** Any process can query.

### 6.8 `ob_query_info(PowerStatus = 33)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 33, `buf` = `&mut PowerSystemStatus` (output), `size` = 8.
- **Returns:** Bytes written on success, `-Fault` if buffer too small.

### 6.9 `ob_query_info(PowerSystemState = 34)` on `\Device\PowerManager` handle

- **Args:** `fd` = handle, `class` = 34, `buf` = `&mut u32` (output), `size` = 4.
- **Returns:** 4 on success.

### 6.10 New HAL primitives

```rust
// ACPI: Parse FADT and extract S5 sleep type and reset register
pub extern "C" fn acpi_parse_fadt(rsdp_addr: u64) -> AcpiPowerInfo;

// ACPI: Write SLP_TYP to PM1a/b control registers for S5 shutdown
pub extern "C" fn acpi_s5_write(pm1a_ctrl: u64, pm1b_ctrl: u64,
                                 slp_typa: u8, slp_typb: u8);

// Reboot: try ACPI reset register, fallback to QEMU debug port, then PS/2
pub extern "C" fn reboot() -> !;

// Poweroff: full ACPI S5 + QEMU ports + PS/2 + halt (replaces existing)
pub extern "C" fn poweroff() -> !;  // Updated to try ACPI S5 first
```

---

## 7. Test Plan

### 7.1 Power Manager initialization (4 tests)

| # | Test | Expected |
|---|------|----------|
| 1 | `POWER_MANAGER.lock()` after Phase 3.883 returns valid state `Active` | State == Active |
| 2 | `\Device\PowerManager` exists in Ob namespace after init | ob_lookup_path succeeds |
| 3 | `ob_query_info(PowerPlanInfo)` returns Balanced plan with default policies | Balanced plan, DisplayTimeout=300 |
| 4 | `ob_query_info(PowerStatus)` returns capabilities matching hardware | capabilities.supports_s5 == true if FADT found |

### 7.2 Power plan switching (4 tests)

| # | Test | Expected |
|---|------|----------|
| 5 | `ob_set_info(PowerSetPlan)` with plan=0, then query returns Balanced | plan.name == Balanced |
| 6 | `ob_set_info(PowerSetPlan)` with plan=1, then query returns Performance | plan.name == Performance |
| 7 | `ob_set_info(PowerSetPlan)` with plan=99 returns `-Inval` | Error code -1 |
| 8 | After plan switch, Registry `ActivePlan` is updated | cm_query_value == new plan index |

### 7.3 Policy modification (4 tests)

| # | Test | Expected |
|---|------|----------|
| 9 | `ob_set_info(PowerSetPolicy)` changes DisplayTimeout to 600 | query returns 600 |
| 10 | `ob_set_info(PowerSetPolicy)` with invalid policy_id returns `-Inval` | Error code -1 |
| 11 | After policy change, Registry value is updated | cm_query_value matches |
| 12 | Policy change persists across plan switch-then-switch-back | Original policy restored |

### 7.4 Shutdown coordination (4 tests)

| # | Test | Expected |
|---|------|----------|
| 13 | Shutdown transitions state from Active → ShuttingDown | state == ShuttingDown |
| 14 | Shutdown dispatches `EVENT_SHUTDOWN` and `EVENT_SHUTDOWN_PHASE2` | Event bus observers notified |
| 15 | Shutdown calls `cm_flush_all_hives()` before `HAL::poweroff()` | (verified via mock) |
| 16 | Second shutdown call during ShuttingDown returns `-Busy` | Error code -15 |

### 7.5 ACPI detection (3 tests)

| # | Test | Expected |
|---|------|----------|
| 17 | FADT present → `acpi_parse_fadt()` returns valid `pm1a_ctrl_blk`, `s5_slp_typa` | Fields non-zero |
| 18 | FADT absent → capabilities.supports_s5 == false | Falls back to QEMU ports |
| 19 | Reset register present in FADT → `acpi_parse_fadt()` returns valid reset_reg | reset_reg.address != 0 |

### 7.6 HAL primitives (3 tests)

| # | Test | Expected |
|---|------|----------|
| 20 | `reboot()` called → does not return (test in QEMU) | Process exits, QEMU resets |
| 21 | `poweroff()` updated → tries ACPI S5 first, then QEMU ports, then PS/2 | Chain verified |
| 22 | `acpi_s5_write()` with valid PM1a writes correct SLP_TYP | PM1a register readback matches |

### 7.7 libneodos wrappers (3 tests)

| # | Test | Expected |
|---|------|----------|
| 23 | `power_get_active_plan()` returns valid `PowerPlanInfo` | plan_info fields match Registry |
| 24 | `power_set_active_plan(Performance)` succeeds and updates Registry | Registry ActivePlan == 1 |
| 25 | `power_reboot()` called from user binary → system resets | QEMU exits with reset code |

---

## 8. Implementation Plan

### Step 1: HAL primitives (2 days)
**Files:** `src/hal/x64/cpu.rs`, `src/hal/x64/mod.rs`, `src/hal/mod.rs`

1. Add `reboot()`: ACPI reset register → QEMU debug port 0xCF9 → PS/2 → `halt()`
2. Add `acpi_parse_fadt()`: parse FADT from ACPI tables (RSDP → XSDT/RSDT → FADT)
3. Add `acpi_s5_write()`: compute SLP_TYPa/b from FADT's S5 package and write to PM1a/b
4. Update existing `poweroff()`: try ACPI S5 first, fallback to current QEMU ports
5. Add `#[used]` ABI retention statics for new functions

### Step 2: Power Manager core (3 days)
**Files:** `src/power/mod.rs`, `src/power/plan.rs`, `src/power/coordinator.rs`, `src/power/acpi.rs`

1. `src/power/acpi.rs`: wrap `acpi_parse_fadt()`, store `AcpiPowerState`
2. `src/power/plan.rs`: `PowerPlan`, `PowerPolicies`, `PowerPlanName` enums
   - `load_from_registry(index)`: read `\Registry\Machine\System\Power\Plans\<Name>\*`
   - `save_to_registry(index)`: write plan policies back to Registry
3. `src/power/mod.rs`: `PowerManager` struct with `POWER_MANAGER` global
   - `init()`: parse ACPI, load active plan from Registry, create Ob object
   - `get_active_plan()`, `set_active_plan()`, `set_policy()`
4. `src/power/coordinator.rs`: `shutdown()`, `reboot()` coordination logic

### Step 3: ObType and namespace (0.5 day)
**Files:** `src/object/types.rs`, `main.rs`

1. Add `PowerManager = 21` to `ObType` enum
2. Add PHASE 3.883 in `main.rs`: `power::power_manager_init()`
   - Register `\Device\PowerManager` in Ob namespace
   - Initialize `PowerManager` struct
   - Store `ObId` in PowerManager for fast handle resolution

### Step 4: Syscall dispatch (1 day)
**Files:** `src/syscall/ob.rs`, `src/syscall/mod.rs`

1. In `handler_ob_set_info`: add match arms for classes 37–42
   - Resolve handle → verify ObType::PowerManager → delegate to `PowerManager`
   - Admin check for Shutdown/Reboot/Suspend/Hibernate/SetPlan/SetPolicy
2. In `handler_ob_query_info`: add match arms for classes 32–34
   - No admin check for query operations
3. Update `ASSIGNED` array in `mod.rs` (no new RAX needed — uses existing Ob syscalls)

### Step 5: Event Bus + ABI freeze (0.5 day)
**Files:** `src/eventbus/mod.rs`, `src/abi_freeze.rs`

1. Add event types 19–26 to `eventbus/mod.rs`
2. Update frozen type checks in `abi_freeze.rs`: verify new types not in 0–15 range
3. Add new types to the frozen validation table

### Step 6: Service Manager integration (0.5 day)
**Files:** `src/services/mod.rs`

1. Add `ServiceManager::stop_all()`: iterate services in reverse dependency order, stop each with timeout
2. `stop_all()` called by `PowerCoordinator::shutdown()` before `EVENT_SHUTDOWN_PHASE2`

### Step 7: Registry defaults (0.5 day)
**Files:** `src/cm/mod.rs`

1. Add `Power\ActivePlan = 0`, `Power\LidAction = 1`, `Power\PowerButtonAction = 3`
2. Add `Power\Plans\Balanced\*`, `Power\Plans\Performance\*`, `Power\Plans\PowerSaver\*` defaults
3. All in `cm_ensure_default_values()`

### Step 8: libneodos wrappers (1 day)
**Files:** `libneodos/src/syscall.rs`, `libneodos/src/power.rs` (new)

1. `libneodos/src/power.rs`: `power_shutdown()`, `power_reboot()`, `power_get_active_plan()`, `power_set_active_plan()`, `power_set_policy()`
2. Data types: `PowerPlanInfo`, `PowerSystemStatus`, `PowerPolicyUpdate`
3. Internal: `ob_open("\Device\PowerManager")` → cache fd → `ob_set_info`/`ob_query_info`
4. Update `libneodos/src/lib.rs` to export power module

### Step 9: Shell commands (0.5 day)
**Files:** `userbin/neoshell/`

1. Add `REBOOT` built-in: calls `libneodos::power_reboot()`
2. Update `POWEROFF` built-in: call `libneodos::power_shutdown()` via Power Manager
3. Both require admin check (currently all processes are admin)

### Step 10: Tests (2 days)
**Files:** `src/power/mod.rs` (add `#[test_case]` blocks)

1. Implement tests from Section 7
2. Add QEMU-based integration test for actual reboot/shutdown (manual test)

### Step 11: Documentation (0.5 day)
**Files:** `docs/syscalls.md`, `docs/objects.md`, `docs/eventbus.md`, `docs/power-manager.md`

1. Document new ObQueryInfoClass/ObSetInfoClass variants in `docs/syscalls.md`
2. Document ObType::PowerManager in `docs/objects.md`
3. Document new event types in `docs/eventbus.md`
4. Document Power Manager architecture in this file

### Total estimated effort: ~12 days

---

## Integration with existing invariants

1. **No automatic builds.** All changes compilable with `cargo build` in `neodos-kernel/`.
2. **Tests before commit.** All 656 existing tests must pass + new power tests.
3. **No new Ring 0 shell commands.** Power commands (POWEROFF, REBOOT) remain built-in in neoshell, using public API.
4. **RAX ≥ 77 → sys_ob_*.** Power operations use existing `ob_set_info`/`ob_query_info` with new info classes. No new RAX needed beyond existing 60–66.
5. **Code is truth.** Update docs when architecture changes.
6. **NT-like design.** Power Manager is a kernel object accessible via Ob namespace, following the pattern of `\Device\Tcp`, `\Device\Udp`.

---

## Future compatibility

The design accommodates without redesign:

| Future feature | How it fits |
|----------------|-------------|
| Battery monitoring | New event types `EVENT_BATTERY_LOW`, `EVENT_POWER_SOURCE_CHANGE`. New query class `PowerBatteryInfo`. |
| CPU frequency scaling | New `CpuPolicy` variant + integration with `CPUID`/MSR throttling via HAL. |
| Multiple user profiles | Per-user power plans stored under `HKEY_CURRENT_USER\Control Panel\Power`. |
| Automatic plan switching | Policy engine watches `EVENT_POWER_SOURCE_CHANGE` and auto-switches plan. |
| Wake timers | Integration with `Timer` ObType to schedule wake-from-S3. |
| GUI integration | Reuses libneodos `power_*()` API. No kernel changes needed. |
| Driver notification | Drivers subscribe to `EVENT_SUSPEND`/`EVENT_RESUME` via Event Bus. |
