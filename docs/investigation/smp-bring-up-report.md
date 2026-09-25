# SMP Bring-Up Investigation — NeoDOS

**Branch:** `investigation/kbd-write-stress` @ `e3cefc6` (+ smp trampoline fix)  
**Kernel:** v0.50.0, QEMU q35, TCG, -smp 1 vs -smp 2, 716 tests, VT 4096  
**Date:** 2026-09-25  

---

## Fase 1 — Baseline SMP=1

```
qemu -smp 1 -machine q35,accel=tcg -m 512M -serial file:serial.log
```

- `BSP APIC ID 0` (`msr::is_bsp()` true), `KPRCB 0x2403000`, `GS 0x2403000`, `CR3 0x1f401000`, `init_smp` → `Max LVT 6` but `AP_READY_COUNT 0` → `1 CPU online` → `I/O APIC` `0xfec00000` 24 pins → `STI` → `PAGING before/after` → `All 716 PASS` → `C:\> `_.
- **PASS**.

---

## Fase 2 — SMP=2

```
qemu -smp 2
```

- `SMP_DETECT apic_base 0xfee00000 version 0x50014 max_lvt 6 ICR clear true final true` → `has_aps true`.
- `MADT dump` no imprimió ( `find_madt_table` → None, `BOOT_RSDP_ADDR` 0x1fec1014 pero XSDT no mapeada).
- `BSP KPRCB 0x2403000` OK.
- `INIT IPI` + `SIPI vector 0x80` (0x80000) enviados, `AP_READY_COUNT` 0 tras 1s → `No APs found (single CPU mode)` → `1 CPU online` (segundo warning).
- `I/O APIC` OK, `STI` → `PAGE_TABLE_CORRUPTION` panic `arch/x64/idt.rs:871` `Page fault @ virt write=1 np=0 rip` `PAGE_TABLE_CORRUPTION`.

**No llega a `C:\>`**.

---

## Fase 3 — No APs found

Dos warnings `smp.rs:477` y `536` mismo texto, distinta causa:

- Primero tras `has_aps=false` → no se intenta AP. Con `-smp 2` `has_aps=true` → no dispara.
- Segundo tras `AP_READY_COUNT==0` 1s → **has_aps true pero AP nunca incrementa `AP_READY_COUNT`**.

Por tanto **Caso C**: MADT encontró 2 CPUs (implícito por `max_lvt`), AP startup ocurrió (INIT/SIPI enviados) pero handshake nunca llegó.

---

## Fase 4 — MADT

`timers/hpet.rs:330` `find_madt_table()` vía `RSDP→XSDT→APIC` no imprimió. Con `SMP=1` QEMU expone 1 `Processor Local APIC` (APIC ID 0, flags 1), con `SMP=2` debería exponer 2 (APIC 0 y 1). No se pudo registrar `logical CPU / APIC ID / enabled` por falta de dump. `max_lvt=6` sugiere 6 LVT entries, no CPU count.

---

## Fase 5 — INIT-SIPI-SIPI

`arch/x64/smp.rs:248` `send_init_ipi()` `ICR 3<<18|5<<8|1<<14` (all excl self, INIT), `send_sipi(0x80)` `3<<18|6<<8|0x80`. Destino `all excl self` `PHYSICAL` sin `apic_id<<32`, broadcast. Vector `0x80` → `0x80000` (512 KB). Correcto para Q35.

No se pudo verificar `destination mode` vs `logical` porque AP no responde.

---

## Fase 6 — AP Trampoline

`AP_TRAMPOLINE_ADDR` era `0x80_0000` (8 MB) pero `SIPI` `0x80` → `0x80000` (512 KB) **mismatch** → AP saltaba a basura → triple fault → no `AP_READY`. **Fix:** `0x8000` (32 KB) `smp.rs:25`, `vector 0x08` → `0x8000`.

Tras fix, `T` (trampoline start), `P` (PM32), `L` (LM64), `A`/`B`/`C` (ap_entry early `out 0x3F8`) aparecen en raw serial (counts 33/61/21/33/22/44), pero `AP_ENTRY` string no. AP llega a `ap_entry` `smp.rs:284` `out 'A'`/`out 'B'`/`out 'C'` pero `serial_println!` no imprime (GS/log no listo). `AP_READY_COUNT` sigue 0 → AP se queda en bucle sin incrementar.

Próximo marcador: `out 'D'`/`'E'` tras `write_gs_base` y `init_ap` no se vieron.

**Primer punto donde deja de ser correcta:** `copy_trampoline` destino vs `SIPI` vector (ya corregido), y luego `ap_entry` antes de `AP_READY_COUNT++`.

---

## Primer punto de fallo (actual)

```
BIOS/QEMU MADT(2 CPUs) → OK
  ↓
RSDP 0x1fec1014 → find_madt_table → FAIL (no dump)
  ↓
detect_aps max_lvt 6 → true
  ↓
copy_trampoline 0x8000 (fix) → OK (T,P,L,A,B,C visibles)
  ↓
INIT/SIPI 0x08 → AP salta a 0x8000 → OK (T)
  ↓
AP PM32 → P, AP LM64 → L, ap_entry A,B,C → OK
  ↓
ap_entry serial_println! → FAIL (no AP_ENTRY)
  ↓
AP_READY_COUNT 0 → timeout 1s → No APs found → 1 CPU
  ↓
STI → PAGE_TABLE_CORRUPTION (BSP) → síntoma secundario
```

`PAGE_TABLE_CORRUPTION` tras `STI` es síntoma de que la memoria en `0x8000` (trampolín) solapa con heap/stack/KPRCB o la tabla en `CR3 0x1f401000` fue corrompida por `copy_trampoline` a `0x8000` que está dentro de área EBDA/BDA (0x8000 es 32KB, usado por BIOS). Con `0x800000` (8MB) estaba en RAM libre (16 MB @0x2400000) pero fuera de <1MB.

---

## Corrección mínima

- Trampolín a `0x8000` + `0x8000` en todos los `.set` ya aplicado.
- Añadir `out` markers en cada fase de `ap_entry` tras `write_gs_base`, `init_ap`, `AP_READY_COUNT++`.
- Hacer `find_madt_table` robusto mapeando XSDT como `USER` o usando `BOOT_RSDP_ADDR` ya identidad-mapeado.
- No tocar PS/2/queue (ya READY).

---

## Validación pendiente

- `qemu -smp 2` con fix debe ver `AP_ENTRY` y `2 CPU(s) online` y `C:\>` sin panic.
- `qemu -smp 4` → 4 CPUs.
- Luego repetir `vtdiag` + `SHELL_BUSY` en SMP2/4.

**Estado actual:** `SMP1 PASS`, `SMP2 FAIL` por trampolín (parcialmente fix, AP ahora arranca pero no completa), `PAGE_TABLE_CORRUPTION` es síntoma.

