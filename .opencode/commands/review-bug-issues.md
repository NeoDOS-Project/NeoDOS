---
description: Revisar todas las issues abiertas con label:type/bug contra el código real
---

# Revisión de issues abiertas `type/bug`

Actúa como revisor de issues de **NeoDOS-Project/NeoDOS**. Tu tarea es auditar
**todas** las issues abiertas con la etiqueta `type/bug`, comprobar si siguen
siendo válidas contra el estado real del repositorio, y emitir un informe con
una recomendación accionable por issue.

> Regla de oro: **el código es la verdad** (AGENTS.md #7). No cierres ninguna
> issue, no modifiques código y no crees ramas/PRs. Esta tarea es **solo
> lectura y análisis**. Las recomendaciones de cierre/duplicado se proponen,
> nunca se ejecutan sin aprobación humana explícita.

## 0. Preparación

Repositorio objetivo: `NeoDOS-Project/NeoDOS`.

```bash
gh repo view NeoDOS-Project/NeoDOS
gh auth status
```

## 1. Inventario de issues a revisar

Obtén la lista completa (no te fíes de una lista cacheada; ejecuta el comando):

```bash
gh issue list \
  --repo NeoDOS-Project/NeoDOS \
  --search 'is:issue state:open label:type/bug' \
  --limit 200 \
  --json number,title,labels,milestone,createdAt,updatedAt,assignees,comments
```

Guarda la lista de números. Para cada issue, recupera el cuerpo y **todos** los
comentarios (los issues de este proyecto acumulan mucha investigación):

```bash
gh issue view <N> --repo NeoDOS-Project/NeoDOS --comments
```

## 2. Recopilar evidencia de estado

Para cada issue, extrae:

- **Síntoma** y **causa raíz** afirmados (título, cuerpo, comentarios).
- **Archivos/funciones** referenciados (`neodos-kernel/src/...`, `userbin/...`).
- **Commits/PRs/branches** mencionados (`#NNN`, `commit hash`, `fix/...`).
- **Issues cruzadas** (`#NNN`), para detectar duplicados/solapes.
- **Docs de investigación** citados (`docs/investigation/*.md`).

Comprueba si ya existe un fix aterrizado:

```bash
git log --oneline --all --grep='#<N>' -20
git log --oneline --all --grep='<término de la causa>' -20
gh pr list --repo NeoDOS-Project/NeoDOS --state all --search '#<N> in:body' --limit 20
gh issue view <N> --repo NeoDOS-Project/NeoDOS --json closedByPullRequestsReferences
```

Verifica el código referenciado en el **estado actual** de `develop`:

```bash
git fetch --all --prune
git log -1 --oneline origin/develop
rg -n '<símbolo o archivo citado>' neodos-kernel/src userbin
```

Consulta los documentos de investigación existentes antes de repetir análisis:

```bash
ls docs/investigation/
rg -n '#<N>|#<vecino>' docs/investigation/
```

## 3. Clasificar cada issue

Asigna exactamente **una** categoría:

| Categoría | Significado | Acción propuesta |
|---|---|---|
| `FIXED` | El fix está en `develop` (commit/PR) y hay tests o evidencia runtime | Proponer cierre con el hash del fix |
| `PARTIAL` | Parte del issue (una firma/causa) está resuelta; otra sigue | Reescribir alcance; no cerrar |
| `DUPLICATE` | Solapa con otra issue abierta | Enlazar y proponer dedupe |
| `STALE` | Título/descripción obsoletos, pero el defecto subyacente sigue | Reenfocar título/cuerpo |
| `NEEDS-INFO` | No reproducible / falta evidencia o entorno | Pedir reproducción |
| `OPEN` | Defecto real, sin fix, bien descrito | Mantener abierta |
| `WRONG` | El comportamiento descrito es el esperado / premisa refutada | Reenfocar o cerrar con justificación |

Criterios de evidencia (exige al menos uno para `FIXED`):

- Commit en `develop` que corrige la causa (no solo refactor adyacente).
- Test de regresión que cubre el caso.
- Evidencia runtime reproducible (boots, captura de log, `neodev test`).
- Documento `docs/investigation/*.md` que pruebe el estado final.

## 4. Detectar duplicados y relaciones

Construye un grafo de referencias cruzadas entre las issues revisadas:

- Agrupa por **subsistema** (`area/kernel`, `area/net`, `area/memory`,
  `area/fs`, `area/power`, `area/security`, `area/shell`).
- Marca familias de bugs que comparten causa raíz o síntoma.
- Señala issues que una misma investigación resolvió o reenfocó.

## 5. Entregable

Genera un informe en Markdown con esta estructura:

````markdown
# Revisión issues `type/bug` — <fecha> — <rama/commit base>

## Resumen
- Total revisadas: N
- FIXED: n · PARTIAL: n · DUPLICATE: n · STALE: n · NEEDS-INFO: n · OPEN: n · WRONG: n

## Tabla
| # | Título | Prioridad | Categoría | Evidencia | Acción propuesta |
|---|--------|-----------|-----------|-----------|------------------|
| 384 | ... | high | OPEN | ... | mantener |

## Detalle por issue

### #<N> — <título>
- **Categoría:** <cat>
- **Estado actual:** <qué dice el código hoy>
- **Evidencia:** <archivo:línea, commit hash, PR, doc>
- **Relacionadas:** #X, #Y
- **Acción propuesta:** <cerrar / reenfocar / merge / pedir info / mantener>
- **Comando de cierre sugerido (NO ejecutar):**
  `gh issue close <N> --comment "Fixed by <hash> (<PR #M>). Evidence: ..."`

## Duplicados / familias
- ...

## Riesgos y lagunas
- Issues sin evidencia suficiente para decidir.
- Defectos detectados en el código que **no** tienen issue (no crear; listar).
````

## 6. Guardrails

- No modifiques código, tests, docs ni el roadmap.
- No ejecutes `build`/`test`/`run` salvo que el usuario lo autorice
  (AGENTS.md #1: no builds automáticos).
- No cierres issues, no apliques labels, no crees issues nuevas.
- Toda afirmación debe llevar **evidencia verificable** (archivo:línea, hash,
  log o doc). Si no la tienes, clasifica como `NEEDS-INFO`, nunca como `FIXED`.
- Si encuentras una capacidad o defecto no trackeado, **repórtalo en el informe**
  pero no abras issue (flujo AGENTS.md: *Discover → Verify → Issue → Document*).
- Responde en el idioma del usuario; mantén los identificadores técnicos en su
  forma original.
