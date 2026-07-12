# CONFIG.SYS — System Configuration

NeoDOS lee `CONFIG.SYS` de la raíz del disco al arrancar (antes de `AUTOEXEC.BAT`).

## Formato

```text
CLAVE=VALOR
; comentarios con punto y coma
# comentarios con numeral
```

Líneas vacías, las que empiezan con `;` o `#` se ignoran. Cada `CLAVE=VALOR` se asigna como variable de entorno.

## Variables

| Clave     | Descripción                     | Valor por defecto |
|-----------|---------------------------------|-------------------|
| `CURSOR`  | Intervalo de parpadeo (ticks PIT ~18.2 Hz) | `18` (~1 seg) |

### CURSOR

Controla la velocidad del parpadeo del cursor. El número son ticks del timer PIT (~18.2 Hz).

| Valor | Parpadeo           |
|-------|--------------------|
| `18`  | ~1 vez/segundo     |
| `9`   | ~2 veces/segundo   |
| `5`   | ~3-4 veces/segundo |
| `36`  | ~1 vez cada 2 seg  |

Se puede cambiar en caliente con `SET CURSOR=N` desde el shell.

## Ejemplo

```ini
CURSOR=9
; linea comentada
```

## Crear CONFIG.SYS en el disco

El disco del sistema es `scripts/neodos_image.img`. Hay dos formas de añadir `CONFIG.SYS`:

### 1. Modificar `create_ne2_image.py`

Editar `scripts/create_ne2_image.py` para personalizar `CONFIG.SYS`.

Luego regenerar la imagen:

```bash
cd scripts && python3 create_ne2_image.py
```

### 2. Añadirlo al disco manualmente (solo FAT32 data disk)

El disco `disk_image.img` (FAT32) se puede modificar con mtools:

```bash
echo "CURSOR=9" > /tmp/CONFIG.SYS
mcopy -i neodos/disk_image.img /tmp/CONFIG.SYS ::
```
