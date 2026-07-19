# NeoDOS — Roadmap / GitHub Sync

Este directorio contiene la configuración para sincronizar el roadmap de NeoDOS
con GitHub Issues.

## Estructura

| Archivo | Propósito |
|---------|-----------|
| `improvements.md` | Ideas locales. Cada ítem se convierte en GitHub Issue. |
| `labels.yaml` | Definición completa de labels de GitHub. |
| `milestones.yaml` | Definición de milestones (versiones). |
| `issue_templates/` | Templates para crear Issues. |

## Sincronización

```bash
scripts/sync-roadmap.sh sync    # Sincroniza todo
scripts/sync-roadmap.sh check   # Verifica conexión
```

## Formato de improvements.md

Cada ítem sigue esta estructura:

```
- **ID**: Título `prioridad` `etiqueta1` `etiqueta2` `hito`
  Descripción del ítem.

  state: open|closed        (opcional, defecto open)
  Dependencies: lista       (opcional)
```

### Campos

- **ID**: Identificador único del ítem (ej: NFSv2-BTREE, USR-P1a)
- **Título**: Descripción corta
- **Backtick tokens** (entre acentos graves):
  - `priority/*` — prioridad (de labels.yaml)
  - `area/*` — área (de labels.yaml)
  - `type/*` — tipo (de labels.yaml)
  - `vX.Y — Name` — milestone (debe coincidir con milestones.yaml)
- **Descripción**: Texto libre indentado tras la línea del título
- **state**: `open` (pendiente) o `closed` (implementada)
- **Dependencies**: IDs de los que depende

### Items implementados

Las funcionalidades ya implementadas se añaden con `state: closed`.
Durante la sincronización se crea la issue con toda la metadata y se cierra
automáticamente, manteniendo el histórico completo.
