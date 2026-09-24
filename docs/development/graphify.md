# Graphify — Análisis estructural para agentes IA

Graphify es una herramienta auxiliar de análisis que construye un grafo de conocimiento **persistente y navegable** del workspace NeoDOS. Convierte código, relaciones entre crates, dependencias, módulos, syscalls, drivers y separación Ring 0 / Ring 3 en tres artefactos: `graph.json` (GraphRAG), `graph.html` (visualización) y `GRAPH_REPORT.md` (reporte).

> **Rol en NeoDOS:** solo análisis. **Nunca** modifica código Rust funcional, APIs, syscalls, ABI, linker scripts ni toolchain. Si Graphify no analiza un crate, se ajusta Graphify vía configuración, no NeoDOS.

## Qué proporciona

* **God nodes**: hubs arquitectónicos más conectados (ej. `Scheduler`, `libneodos`, `HandleEntry`).
* **Comunidades**: clusters por subsistema (VFS, HAL, ipc, scheduler, memory, drivers, net, security, usermode).
* **Consultas** acotadas (`query`, `path`, `explain`) que devuelven subgrafos mucho más pequeños que `grep` o `GRAPH_REPORT.md`.
* Detección automática de corpus Rust: NeoDOS no es un workspace Cargo único sino **~70 crates independientes** (kernel, bootloader, drivers, libneodos, ~40 binarios `userbin/`, 4 libs NXL, tools).

## Instalación

### Requisitos verificados en esta integración

* Python `3.14.6`, `uv 0.12.15`, Rust `1.95.0` (stable + nightly `nightly-x86_64-unknown-linux-gnu`)
* `uv` en `~/.local/bin`

```bash
# 1. Instalar uv (si falta)
curl -LsSf https://astral.sh/uv/install.sh | sh
export PATH="$HOME/.local/bin:$PATH"

# 2. Instalar Graphify (paquete graphifyy, binario graphify) — SIN sudo
uv tool install graphifyy

# 3. Verificar
graphify --version   # 0.9.63
uv tool list

# Si graphify no está en PATH
uv tool update-shell
```

> El paquete se llama `graphifyy`, el ejecutable es `graphify`. No usar `pip`, `apt` ni `sudo` si `uv tool` funciona.

## Integración con OpenCode

OpenCode en NeoDOS usa `opencode.json` en la raíz + skill `skills/` + carpeta `.opencode/` para plugins.

Graphify ofrece `graphify install --platform opencode` que:

* instala skill en `~/.config/opencode/skills/graphify/` (global, reutilizable por el usuario) y/o `.opencode/skills/graphify/` (project-scoped, reproducible)
* crea plugin ` .opencode/plugins/graphify.js` (hook `tool.execute.before`)
* registra el plugin en `.opencode/opencode.json`
* (con `--project`) añade sección `## graphify` a `AGENTS.md`

Comprobar antes de instalar:

```bash
git status
ls -la .opencode/ AGENTS.md opencode.json
```

Instalación **project-scoped** (preferida para NeoDOS — queda claro qué es del repo y qué es global del usuario):

```bash
# Instalación estándar (plugin project + skill global)
graphify install --platform opencode

# Project-scoped completo (skill también dentro del repo, commitable)
graphify install --platform opencode --project
# Salida esperada:
#   skill installed -> .opencode/skills/graphify/SKILL.md
#   .opencode/plugins/graphify.js -> tool.execute.before hook written
#   .opencode/opencode.json -> plugin registered
#   graphify section written to .../AGENTS.md
#   Project-scoped install. Add to version control:
#     git add .opencode/ AGENTS.md
```

**NO sobrescribe** `opencode.json` raíz, `AGENTS.md` existente (solo añade sección), ni `CLAUDE.md`.

Para desinstalar:

```bash
graphify uninstall --platform opencode          # o: graphify opencode uninstall
# --purge también borra graphify-out/
```

## Generar el grafo

Desde la raíz de NeoDOS (`/home/amartinper/rust-os/neodos` o `neodos/` si es clon directo):

```bash
cd /home/amartinper/rust-os/neodos

# Extracción headless solo código (sin LLM, sin API key)
graphify extract . --code-only
# o atajo:
graphify . --code-only     # equivale a extract

# Generar reporte y HTML (requiere grafo previo)
graphify cluster-only .

# Incremental tras cambios (solo código, sin API key, rápido)
graphify update .

# Opciones útiles
graphify extract . --code-only --no-cluster   # solo graph.json, sin cluster
graphify extract . --code-only --force        # re-scan completo, ignora cache
```

> NeoDOS tiene **~70 crates sin Cargo.toml raíz**, por eso `graphify extract --cargo` falla con `No such file: Cargo.toml`. El AST Rust (`tree-sitter-rust`) sí detecta los crates individuales y sus dependencias internas.

### Resultado verificado

* `graphify extract . --code-only` → `375 code files` detectados, `116 docs` saltados, `267` no clasificados (`.toml`, `.ld`, `.klc`, etc.)
* `graph.json: 7260 nodes, 13038 edges, 379 communities` (tras `cluster-only`)
* Token cost: `0` (AST-only)

Si hay `GEMINI_API_KEY`/`GOOGLE_API_KEY` configurado, `graphify extract .` sin `--code-only` intentará extracción semántica de docs con Gemini (`gemini-3-flash-preview`). Sin clave, usar `--code-only` es la vía headless soportada.

## Dónde se generan los resultados

Todos los artefactos van a `graphify-out/` (respecta `GRAPHIFY_OUT` env var):

| Archivo | Tamaño verificado | Descripción |
|---------|-------------------|-------------|
| `graph.json` | ~7.0 MiB | Grafo principal. Nodos=symbols/files, edges=calls/references/imports. Fuente para `query`/`path`/`explain`. **No versionar** (grande, regenerable). |
| `GRAPH_REPORT.md` | ~64 KiB | Reporte legible: corpus, god nodes, comunidades, freshness. Útil para revisión amplia. **No versionar**. |
| `graph.html` | ~302 KiB | Visualización agregada (379 comunidades, 701 cross-edges, límite 5000 nodos). **No versionar**. |
| `.graphify_analysis.json` | ~387 KiB | Comunidades, cohesión, god nodes, surprises. Sidecar interno. Cache. |
| `.graphify_labels.json` + `.sig` | ~8–10 KiB | Labels de comunidades (placeholders `Community N` si falla LLM). Cache. |
| `manifest.json` | ~80 KiB | Hashes por archivo para incremental (`detect_incremental`). Cache. |
| `cache/ast/` + `stat-index.json` | ~7.3 MiB | Cache AST+stat para `update`/`extract` incremental. Cache pura. |
| `.graphify_root` | 31 B | Root del scan. Auxiliar. |

> **No hacer `git add graphify-out/`**. Añadido a `.gitignore`.

## Qué archivos se versionan / ignoran

**Versionar (reproducible, Commit):**

* `.opencode/plugins/graphify.js` — plugin hook OpenCode
* `.opencode/opencode.json` — registro del plugin (`{"plugin":[".opencode/plugins/graphify.js"]}`)
* `.opencode/skills/graphify/SKILL.md` + `references/` — skill project-scoped (solo si se usó `--project`)
* `AGENTS.md` — sección `## graphify` añadida (reglas para agentes)
* `.gitignore` — línea `graphify-out/` + `.graphify_cache/`

**No versionar / ignorado:**

* `graphify-out/` completo (ver tabla arriba) — declarado en `.gitignore` raíz como `graphify-out/` y `.graphify_cache/`
* `~/.config/opencode/skills/graphify/` — instalación global del usuario (fuera del repo)
* `~/.graphify/global-graph.json` — si se usa `graphify global add` (no usado en NeoDOS)

## Uso desde OpenCode / agente

El plugin `tool.execute.before` inyecta un recordatorio en cada `bash` cuando `graphify-out/graph.json` existe:

> `[graphify] knowledge graph at graphify-out/. For focused questions, run graphify query ...`

Comandos clave (todos verificados con `graphify 0.9.63`):

```bash
# BFS — contexto amplio
graphify query "How does the scheduler work? Priorities and aging" --budget 2000

# DFS — trazar path específico
graphify query "syscall dispatch SSDT" --dfs --budget 1500

# Camino más corto entre conceptos
graphify path "Scheduler" "Memory"
graphify path "Scheduler" "Memory" --undirected   # ignora dirección

# Explicación de un nodo
graphify explain "HandleEntry"
graphify explain "neodos-kernel/src/scheduler/mod.rs::Scheduler"  # desambiguado

# Hubs arquitectónicos
graphify god-nodes --top 10
graphify god-nodes --top 10 --json

# Impacto inverso (qué depende de X)
graphify affected "Scheduler" --depth 2

# Actualizar tras editar código
graphify update .
```

**Workflow recomendado para Astra/OpenCode:**

1. Pregunta de arquitectura → `graphify query "<pregunta>"` primero (subgrafo < 2000 tokens).
2. Si falta contexto → `graphify path` / `explain` / `god-nodes`.
3. Solo si no basta → leer `graphify-out/GRAPH_REPORT.md` o `graphify-out/wiki/index.md` (si existe) o `grep`/`Read` directo.
4. Tras modificar código → `graphify update .` (AST-only, gratis, sin API).

El skill `/graphify` (instalado en `.opencode/skills/graphify/SKILL.md`) automatiza el pipeline completo (detect → AST → semantic → build → cluster) cuando el usuario escribe `/graphify` o `/graphify <path>`.

## Actualizar Graphify

```bash
uv tool install --upgrade graphifyy
graphify --version
# Si hay skill stale, reinstalar:
graphify install --platform opencode --project
```

El CLI avisa si el skill está desactualizado comparando `.graphify_version` junto al `SKILL.md`.

## Limitaciones conocidas en NeoDOS

* **Sin Cargo workspace raíz**: `--cargo` no extrae deps crate→crate; el grafo de dependencias Rust se infiere vía `tree-sitter` + imports, no vía `Cargo.toml` agregado.
* **Sin API key**: docs (`*.md`, 116 files) se saltan con `--code-only`. Para incluir docs semánticamente se necesita `GEMINI_API_KEY` (Gemini) o backend local (`--backend openai` con `OPENAI_BASE_URL=http://localhost:1234/v1` si hay LM Studio). Sin clave, el grafo cubre solo código (`no_std`, Ring 0/3, UEFI bootloader, drivers NEM, userbin).
* **Tamaño**: `graph.json` > 5 MiB activa vista agregada en `graph.html` (comunidades). Para detalle nodo-a-nodo usar `--obsidian` o `graphify query`.
* **Labeling**: `cluster-only` intenta etiquetar comunidades con `claude -p`; sin login cae a placeholders `Community N` (funcional pero menos descriptivo). No bloquea generación del grafo.

## Validación

```bash
graphify extract . --code-only && graphify cluster-only .
graphify god-nodes --top 10
graphify query "scheduler priorities" --budget 1000

# Rust — debe seguir en 0 (warnings preexistentes permitidos)
cargo check          # en neodos-kernel/ con nightly
# o
RUSTUP_TOOLCHAIN=nightly cargo check -p neodos_kernel
```

Antes vs después de Graphify: `cargo check` mismo resultado (414 warnings preexistentes, `Finished` OK). Graphify no toca toolchain ni dependencias.
