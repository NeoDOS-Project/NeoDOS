# Revisión issues `type/bug` — 2026-10-01 — `investigate/376-phase2` @ `e57bd18`

Base de integración: `origin/develop` @ `717a81f`.
Método: `gh issue view --comments` + verificación contra código (`rg`/`git show`/`git merge-base`).
Alcance: solo lectura. No se modificó código, no se ejecutaron builds ni comandos `gh` de escritura.

## Resumen

- Total revisadas: **14**
- FIXED: **2** · PARTIAL: **5** · OPEN: **7** · DUPLICATE: **0** · STALE: **0** · NEEDS-INFO: **0** · WRONG: **0**

| # | Título | Prioridad | Categoría | Acción propuesta |
|---|---|---|---|---|
| 384 | NeoInit user `#PF` at entry | high | OPEN | Mantener; depende de #383 |
| 383 | NoMem pese a heap libre (corrupción free-list) | high | PARTIAL | Aplicar fix residual y mergear |
| 376 | Boot deadlock en auto-start (VFS lock) | high | PARTIAL | Retitular + mergear y reconducir |
| 374 | SM nunca observa la salida del servicio | high | OPEN | Implementar hook/watcher |
| 345 | SMP1 panic de heap en spawn de NeoInit | medium | OPEN | Mergear #383 + rollback/backoff |
| 340 | netd no se planifica / `sys_yield` | medium | PARTIAL | Reencauzar al timeout de `dhcpd` |
| 331 | SMP>1 hang / OB_WAIT | high | **FIXED** | Cerrada ✅ (2026-10-01) |
| 317 | IPv4 next-hop usa solo el NIC por defecto | medium | OPEN | Usar `socket.nic_id` |
| 316 | DHCP renewal no implementado | medium | OPEN | Implementar RENEW/REBIND o corregir docs |
| 315 | TCP bypassa next-hop/gateway | medium | OPEN | Usar `nic_next_hop` en TCP |
| 313 | POWEROFF poco fiable / sin flush | high | PARTIAL | Drain completo + diagnóstico de halt |
| 311 | No perder el primer datagrama UDP (ARP) | medium | OPEN | Revivir rama `fix/311-...` |
| 302 | Child exit no despierta al padre | high | **FIXED** | Cerrada ✅ (2026-10-01) |
| 22 | SeAccessCheck: DACL vacío + group SIDs | high | PARTIAL | Arreglar group SIDs; corregir ruta/scope |

## Hallazgos globales

1. **Brecha de integración (crítica).** Los cinco commits que resuelven #382/#383/#376
   están **solo** en `origin/investigate/376-phase2`, **no en `develop`**:
   `6f0ec21` (#382 prioridad), `d1aa449` (preempt FS), `db3aa2d` (preempt NIC RX),
   `2170577` (#383 routing por rango), `e57bd18` (locks IRQ-safe + serial).
   Consecuencia: **#382 figura CLOSED pero su fix no está en la rama de integración**,
   y #376/#383 siguen abiertas de facto en `develop`.
2. **Instrumentación TEMP sin commitear** en `neodos-kernel/src/slab.rs`
   (`FB_ACTIVE`/`FB_FREE_ERR`/`FB_OVERLAP`) para cazar el doble/foreign free de #383.
3. **Residual de #383 aún presente en el HEAD de la rama.** Pese a `2170577`,
   el fall-through de `dealloc_inner` sigue enviando al fallback heap un puntero
   slab no liberado (`slab.rs:458-459` en `e57bd18`). El working tree lo neutraliza
   (sin commit).
4. **Sin PRs abiertos** referenciando ninguna de las 14 issues.

## Detalle por issue

### #331 — SMP>1: hang intermitente — shell nunca vuelve / OB_WAIT no despierta
- **Categoría:** FIXED.
- **Firma 2** (parent en `OB_WAIT` tras exit del hijo): corregida por `15f9c11` (PR #344, en develop):
  `build_tlb_target_mask()` puro/sin SCHEDULER en `arch/x64/paging.rs:39`, y wake inline de
  `ChildExit` en `scheduler/lifecycle.rs:743-750`.
- **Firma 1** (gap SMP>1 kernel→user): reatribuida a #355 y corregida por `4b8a6fa` (PR #380, en develop).
- #343 y #355 CLOSED en develop. Evidencia: `docs/investigation/smp331-exit-tlb-shootdown-self-deadlock.md`;
  16 boots SMP2 con 0 stalls.
- **Resto:** un flake solo-DHCP (DISCOVER sin ACK) que pertenece a #340, no a #331.
- **Acción:** **cerrada como completada el 2026-10-01.**
  `gh issue close 331 --reason completed --comment "Firmas 1 y 2 resueltas en develop (#344 15f9c11, #355/#380 4b8a6fa). Flake DHCP residual en #340."`

### #302 — Child exit no despierta al padre (shell cuelga en OB_WAIT tras `tree`)
- **Categoría:** FIXED (es la firma 2 de #331).
- `15f9c11` elimina la re-adquisición recursiva de `SCHEDULER` en el TLB shootdown de salida;
  `tree C:` vuelve 7/7 en SMP2 (pre-fix 3/3 colgaba). `paging.rs`/`lifecycle.rs`/`handlers.rs`/`wait.rs`
  idénticos en develop.
- **Acción:** **cerrada como completada el 2026-10-01.**
  `gh issue close 302 --reason completed --comment "Fixed by #344 / 15f9c11."`

### #383 — SMP2: spawn de servicio falla con NoMem pese a ~15.8 MB libres
- **Categoría:** PARTIAL.
- **develop:** sin corregir; `dealloc` aún enruta por `SLAB_MAGIC` del payload (`slab.rs:408-437`).
- **rama:** `2170577` corrige el misroute por magic (routing por `HEAP_START..+HEAP_SIZE`), pero
  **subsiste** el fall-through final: un puntero slab que falla su free se entrega al fallback heap
  (`slab.rs:458-459`), corrompiendo la free-list.
- Sospechosos de rollback (kernel-stack ownership) verificados sanos.
- **Acción:** commitear el arreglo del fall-through del working tree, validar `[FB_FREE_ERR]` por serial,
  y mergear `2170577` + fix residual a develop. No cerrar aún.

### #376 — Boot deadlock intermitente en auto-start (VFS lock)
- **Categoría:** PARTIAL (título/framing STALE: no es deadlock de VFS sino inanición + locks no-IRQ-safe).
- **develop:** el síntoma original persiste; lo único integrado es #355 (`4b8a6fa`) y #381.
  Ningún commit etiquetado `(#376)` está en develop.
- **rama:** `6f0ec21` (prioridad en fast-path `scheduler/schedule.rs`), `d1aa449` (preempt FS en `globals.rs`),
  `db3aa2d` (preempt RX en `net/mod.rs`), `e57bd18` (locks IRQ-safe) — todos solo en la rama.
- **Acción:** abrir PR de `investigate/376-phase2` → develop; retitular a "inanición/locks no-IRQ-safe";
  tras merge, reconducir a residuales #383/#384.

### #384 — SMP2: user `#PF` de NeoInit en entry
- **Categoría:** OPEN.
- La ruta de mapeo `elf::load_elf → spawn_usermode → wait_for_process` es coherente:
  ventana user mapeada con huge pages 2 MB `USER_ACCESSIBLE` (`paging.rs:296-324`), entry dentro de
  página ya mapeada. Ningún commit de la rama toca esta ruta.
- El fault no adjunta `virt=` ni error-code; coocurre con #383.
- **Acción:** mantener abierta como dependiente de #383; capturar `virt=`/error-code para desambiguar.

### #345 — SMP1: panic de allocación de kernel-heap en el loop de spawn de NeoInit
- **Categoría:** OPEN.
- `handler_ob_create(Process)` retorna error **después** de crear el hijo sin terminarlo
  (`syscall/ob/create.rs:266-269, 275-277, 294-296`); `add_ring3_process_with_stack` ignora el fallo de
  `ob_create_object` y devuelve `Ok` con `ob_id=None` (`lifecycle.rs:340-359`); el loop de
  `userbin/neoinit` reintenta sin backoff (`main.rs:195-221`). Fuga por intento.
- #351 ni #375 tocan el path de asignación. Causa probable: corrupción de heap de #383 (fix no en develop).
- **Acción:** mergear #383, añadir rollback post-spawn y backoff; test de fallo post-spawn.

### #340 — netd no se planifica; `sys_yield` de dhcpd colapsa la ventana DHCP
- **Categoría:** PARTIAL (premisa del título STALE).
- `handler_yield` **sigue** drenando RX sync (`syscall/handlers.rs:112`) → la premisa "RX depende de netd" es falsa.
- Síntoma DHCP inicial corregido en #341 (`8ee470a`); inanición Ring-0 corregida en #355 (`4b8a6fa`, en develop).
- Residual real: timeout de `dhcpd` contado por iteraciones, no wall-clock
  (`userbin/dhcpd/src/main.rs:54,56,162-168,428-471`).
- Nota: la inanición Ring-3-vs-Ring-3 es #382 (fix `6f0ec21` no está en develop).
- **Acción:** reencauzar el título al timeout de `dhcpd` (o cerrar como superseded por #341+#355).

### #374 — El Service Manager nunca observa la salida del servicio
- **Categoría:** OPEN (confirmado).
- `on_process_exit()` solo lo llaman tests (`services/mod.rs:110,127,303,319`); `start_service`
  (`manager.rs:362-377`) no registra watcher `ChildExit` sobre el pid. El primitivo existe pero el SM no
  es un thread que bloquee; `EVENT_PROCESS_EXIT` nunca se publica. `exit_count`/`last_exit_code` siempre 0.
- **Acción:** hook de exit en `scheduler/lifecycle.rs` (junto a waiters `ChildExit`) o thread del SM que
  bloquee en `kwait_block(ChildExit)`; respetar orden de locks de #343; test de integración.

### #317 — IPv4 next-hop usa solo el NIC por defecto
- **Categoría:** OPEN.
- `socket_send_udp_raw` usa `nic_default_id()` (`socket.rs:318-323`); el next-hop `nic_next_hop` usa
  `default_nic_id()` (`nic.rs:197-198`). `socket.nic_id` se escribe pero nunca se lee para enrutar.
- **Acción:** seleccionar NIC/source/next-hop desde `socket.nic_id`; test multi-NIC.

### #316 — DHCP lease renewal no implementado
- **Categoría:** OPEN.
- `DhcpState::Renewing` nunca se asigna; `renew()` es `dead_code` sin llamadores (`dhcpd/src/main.rs:543-565`);
  tras DORA termina en `loop { sys_yield() }` (`:733-734`). Docs afirman renovación al 50% (`stack.md:197-204`).
- **Acción:** implementar RENEW/REBIND (unicast, half-lease) o corregir docs.

### #315 — TCP bypassa next-hop/gateway
- **Categoría:** OPEN.
- `tcp_send_syn_ack` resuelve MAC con `arp_resolve(dst_ip)` directo (`tcp.rs:467`) en vez de
  `nic_next_hop`; `nic_default_id()` en `tcp.rs:428`.
- **Acción:** usar `nic_next_hop(dst_ip)` en el path TCP.

### #311 — No perder el primer datagrama UDP durante ARP
- **Categoría:** OPEN (existe fix local **no integrado**).
- develop: `arp_resolve` fire-and-forget (`arp.rs:207-234`), `socket_send_udp_raw` descarta (`socket.rs:347`);
  workaround en `libnet/src/dns.rs:99-118`.
- Fix `038b41b` (`arp_resolve_blocking` + test `net_udp_first_datagram_survives_arp_miss`) está **solo** en
  la rama local `fix/311-udp-arp-first-datagram`, sin remoto ni PR; anterior a #306, requiere rebase.
- **Acción:** rebasar/aplicar sobre #306, abrir PR y validar el test.

### #313 — POWEROFF poco fiable y puede no volcar hives
- **Categoría:** PARTIAL.
- El flush de hives **sí** está: `object/power.rs:34-45` llama `cm_flush_all_hives()` y
  `flush_cache_if_needed()` antes de `hal::poweroff()` (desde `415773e`, antes del issue).
- Pendiente: el page cache se drena de a **8 páginas** por llamada (`globals.rs:133-152`), sin bucle en
  shutdown → resto de páginas sucias sin escribir. `hal::poweroff()` (`cpu.rs:50-68`) deshabilita IRQs y
  si S5/puertos/PS2 fallan termina en `halt()` = cuelgue sin timeout ni feedback. `power/coordinator.rs`
  está duplicado y muerto.
- **Acción:** drenar todo el page cache (`while dirty>0`), feedback de progreso, unificar en coordinator o
  borrar el duplicado; diagnóstico del hang S5.

### #22 — USR-P1d: SeAccessCheck DACL vacío + group SIDs
- **Categoría:** PARTIAL (ruta del issue incorrecta).
- El archivo citado `security/access_check.rs` **no existe**; el real es `security/access.rs`.
- Empty DACL → allow (`access.rs:44-46`) y NULL DACL → full access (`:40-43`) ya implementados (desde
  `69396c4`, antes del issue). Deny-first NT-correcto en `:10-26`.
- **Pendiente real:** group SIDs — `check_dacl` solo compara `token.sid`, nunca `token.groups`
  (`token.rs:28,72-80`). SACL/audit no existen (`acl.rs:64-70`). Sin tests de estos casos.
- **Acción:** corregir/reescribir el cuerpo (ruta y scope), implementar evaluación de groups y decidir
  SACL/audit; añadir tests.

## Duplicados / familias

- **Familia exit/wakeup:** #302 ⊂ #331 (firma 2, FIXED).
- **Familia corrupción de heap / SMP:** #383 ↔ #384 ↔ #345 (misma ventana; #383 es la raíz probable).
- **Familia scheduling/starvation:** #340 (Ring-0, #355 FIXED) y #382 (Ring-3, fix no en develop).
- **Familia red/per-NIC:** #317 (UDP/ICMP) y #315 (TCP) comparten la causa "no usar el NIC del socket";
  #311 es adyacente (ARP en send).
- No se detectaron duplicados exactos entre las 14.

## Riesgos y lagunas

- El estado "arreglado" de #382/#376/#383 descansa en commits **no integrados** en develop.
- #384 no es diagnosticable sin el `virt=`/error-code del `#PF`.
- Sin builds/tests (AGENTS.md #1), la clasificación FIXED se apoya en código + evidencia de commits/docs,
  no en una ejecución nueva.
- No se detectaron defectos sin issue fuera del alcance de esta revisión.
