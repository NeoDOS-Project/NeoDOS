# xkbcompile

Host tool: convert a **Linux XKB** layout (`/usr/share/X11/xkb/symbols/*`) into
the NeoDOS `.kbd` keyboard format. Complements `kbdcompile` (which reads the
Microsoft `.klc` format); XKB is offline and openly licensed, so it is the
preferred source for shipping many languages.

```bash
cargo build
./target/debug/xkbcompile <layout> [variant] \
    [--name TAG] [--lang TAG] [--out FILE] \
    [--symbols-dir DIR] [--keycodes FILE] [--compose FILE]

# Spanish (es) base variant -> data/keyboard/es-ES.kbd
./target/debug/xkbcompile es basic --name es-ES --lang es-ES \
    --out ../../data/keyboard/es-ES.kbd
```

Defaults: `--symbols-dir /usr/share/X11/xkb/symbols`,
`--keycodes /usr/share/X11/xkb/keycodes/evdev`,
`--compose /usr/share/X11/locale/en_US.UTF-8/Compose`, `--out
data/keyboard/<name>.kbd`.

## What it does

- Resolves `include "file(section)"` chains (e.g. `es(basic)` →
  `latin(type4)` → `latin(basic)`), later `key` definitions overriding earlier.
- Maps EVDEV keycodes to PC set-1 scancodes (`set1 = evdev - 8`).
- Maps levels 1-3 to the kernel's `normal`/`shift`/`altgr` columns.
- Translates keysyms: Latin-1 names, `U<hex>` Unicode keysyms and the common
  punctuation/accent names.
- Marks `dead_*` keysyms (per level) and builds the compose table from the
  X11 `Compose` file for the dead keys used.

## Limits

- Only the first keyboard group; levels 4+ (Shift+AltGr) are dropped (the
  kernel format has no such column).
- Exotic keysyms (Greek, Cyrillic, ligatures, `Greek_*`, `XF86*`) are not
  translated and leave the key unmapped.
- The `Ctrl` column is left unset (XKB does not define ctrl levels); the kernel
  falls back to the normal/shift value.

Adding a layout to the OS also needs wiring it into `neodev/src/image.rs`
(`kbd_layouts`) and the kernel load list (`load_layouts()`), until dynamic
`.kbd` discovery lands (#390).
