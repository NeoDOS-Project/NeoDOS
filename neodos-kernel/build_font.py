#!/usr/bin/env python3
"""Generate font.rs from DOS System Font OTF - raw extraction, no vertical shifting."""
from PIL import Image, ImageDraw, ImageFont

FONT_PATH = "/home/amartinper/Descargas/dos-system-font.otf/dos-system-font.otf"
OUTPUT_PATH = "/home/amartinper/rust-os/neodos/neodos-kernel/src/font.rs"

font = ImageFont.truetype(FONT_PATH, 16)


def render_glyph(codepoint):
    try:
        c = chr(codepoint)
        img = Image.new("1", (8, 16), 0)
        draw = ImageDraw.Draw(img)
        draw.text((0, 0), c, font=font, fill=1)
        return [sum(((1 if img.getpixel((x, y)) else 0) << (7 - x)) for x in range(8)) for y in range(16)]
    except:
        return [0] * 16


def has_pixels(rows):
    return any(r != 0 for r in rows)


# Render all glyphs
glyphs = [render_glyph(cp) for cp in range(256)]

# Blank control chars
for cp in range(0x00, 0x20):
    glyphs[cp] = [0] * 16
glyphs[0x20] = [0] * 16   # Space
glyphs[0x7F] = [0] * 16   # DEL
glyphs[0xAD] = [0] * 16   # Soft hyphen

# Euro at slot 0x80
euro = render_glyph(0x20AC)
glyphs[0x80] = list(euro) if has_pixels(euro) else [0] * 16

# Full block at slot 0x81
glyphs[0x81] = [0xFF] * 16


def fmt_glyph(g):
    return ', '.join(f'0x{r:02X}' for r in g)


def esc_char(c):
    if c == '\\':
        return "b'\\\\'"
    if c == '\'':
        return "b'\\''"
    if c == '"':
        return "b'\"'"
    return f"b'{c}'"


lines = [
    "// src/font.rs",
    "// 8x16 bitmap font generated from DOS System Font (CC-BY-SA 3.0, krystman)",
    "// https://fontstruct.com/fontstructions/show/87372",
    "",
    "pub const FONT_HEIGHT: usize = 16;",
    "pub const FONT_WIDTH: usize = 8;",
    "",
    "pub const FONT: [[u8; 16]; 256] = {",
    "    let mut f = [[0u8; 16]; 256];",
    "",
    "    let mut i = 0;",
    "    while i < 256 {",
    "        f[i] = [0; 16];",
    "        i += 1;",
    "    }",
    "",
    "    // Numbers (0x30-0x39)",
]

for cp in range(0x30, 0x3A):
    lines.append(f"    f[{esc_char(chr(cp))} as usize] = [{fmt_glyph(glyphs[cp])}];")

lines.append("")
lines.append("    // Uppercase A-Z (0x41-0x5A)")
for cp in range(0x41, 0x5B):
    lines.append(f"    f[{esc_char(chr(cp))} as usize] = [{fmt_glyph(glyphs[cp])}];")

lines.append("")
lines.append("    // Lowercase a-z (0x61-0x7A)")
for cp in range(0x61, 0x7B):
    lines.append(f"    f[{esc_char(chr(cp))} as usize] = [{fmt_glyph(glyphs[cp])}];")

lines.append("")
lines.append("    // Symbols and punctuation")
others = [cp for cp in range(0x21, 0x7F) if cp not in range(0x30, 0x3A) and cp not in range(0x41, 0x5B) and cp not in range(0x61, 0x7B)]
for cp in others:
    lines.append(f"    f[{esc_char(chr(cp))} as usize] = [{fmt_glyph(glyphs[cp])}];")

lines.append("")
lines.append("    // ============================================================")
lines.append("    // Latin-1 Supplement - Spanish accented chars and symbols")
lines.append("    // ============================================================")

name_map = {
    0xC0: "Agrave", 0xC1: "Aacute", 0xC2: "Acirc", 0xC3: "Atilde",
    0xC4: "Adiaer", 0xC5: "Aring", 0xC7: "Ccedil",
    0xC8: "Egrave", 0xC9: "Eacute", 0xCA: "Ecirc", 0xCB: "Ediaer",
    0xCC: "Igrave", 0xCD: "Iacute", 0xCE: "Icirc", 0xCF: "Idiaer",
    0xD1: "Ntilde", 0xD2: "Ograve", 0xD3: "Oacute", 0xD4: "Ocirc",
    0xD5: "Otilde", 0xD6: "Odiaer", 0xD9: "Ugrave", 0xDA: "Uacute",
    0xDB: "Ucirc", 0xDC: "Udiaer", 0xDD: "Yacute", 0xDF: "germandbls",
    0xF1: "ntilde", 0xFF: "ydiaer", 0xBF: "questiondown", 0xA1: "exclamdown",
    0xAA: "ordfeminine", 0xBA: "ordmasculine",
}

for cp in range(0xA0, 0x100):
    g = glyphs[cp]
    if has_pixels(g):
        comment = f"  // {name_map[cp]}" if cp in name_map else ""
        lines.append(f"    f[0x{cp:02X}] = [{fmt_glyph(g)}];{comment}")

lines.append("")
lines.append("    // Extra glyphs (non-Latin-1, mapped from codepoints)")
lines.append("    // EUR SIGN (U+20AC) at slot 0x80")
lines.append(f"    f[0x80] = [{fmt_glyph(glyphs[0x80])}];")
lines.append("    // FULL BLOCK (U+2588) at slot 0x81 - fallback for unknown glyphs")
lines.append(f"    f[0x81] = [{fmt_glyph(glyphs[0x81])}];")
lines.append("")
lines.append("    f")
lines.append("};")
lines.append("")
lines.append("use crate::graphics::RENDERER;")
lines.append("")
lines.append("pub fn draw_char(c: u8, x: usize, y: usize, color: u32) {")
lines.append("    unsafe {")
lines.append("        if let Some(ref r) = RENDERER {")
lines.append("            let font_char = FONT[c as usize];")
lines.append("            for (row, byte) in font_char.iter().enumerate() {")
lines.append("                for col in 0..8 {")
lines.append("                    if (byte & (0x80 >> col)) != 0 {")
lines.append("                        r.put_pixel(x + col, y + row, color);")
lines.append("                    } else {")
lines.append("                        r.put_pixel(x + col, y + row, 0x000000);")
lines.append("                    }")
lines.append("                }")
lines.append("            }")
lines.append("        }")
lines.append("    }")
lines.append("}")

with open(OUTPUT_PATH, 'w') as f:
    f.write('\n'.join(lines) + '\n')

print(f"Generated {OUTPUT_PATH} ({256 - sum(1 for g in glyphs if not has_pixels(g))} non-empty glyphs)")
PYEOF
