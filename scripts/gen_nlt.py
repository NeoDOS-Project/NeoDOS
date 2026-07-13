#!/usr/bin/env python3
"""
gen_nlt.py — Generate NLT translation files for all locales.

Usage:
  python3 scripts/gen_nlt.py                 # generate all
  python3 scripts/gen_nlt.py en-US           # generate one locale
  python3 scripts/gen_nlt.py en-US corehelp  # one locale, one app

Output: data/locale/{locale}/{app}.nlt
"""

import struct, sys, json, os
from pathlib import Path

MAGIC = b'NLT\0'
VERSION = 1

LOCALE_DIR = Path(__file__).resolve().parent.parent / "data" / "locale"

# ── Translation tables ─────────────────────────────────────────────────

TRANSLATIONS = {}

def t(locale, app, entries):
    """Register a translation table for locale+app."""
    key = (locale, app)
    if key not in TRANSLATIONS:
        TRANSLATIONS[key] = []
    TRANSLATIONS[key].extend(entries)

# ═══════════════════════════════════════════════════════════════════════
# en-US (English, default)
# ═══════════════════════════════════════════════════════════════════════

t("en-US", "corehelp", [
    ("help.header", "NeoDOS Core Tools"),
    ("help.commands_suffix", " command(s) available"),
    ("help.type_for_details", "Type HELP <command> for details on a specific command."),
    ("help.example", "  Example: HELP CLS"),
    ("error.no_programs_dir", "No Programs directory found."),
    ("error.create_programs_dir", "Create C:\\Programs with .NXE tools."),
    ("error.reading_dir", "(error reading directory)"),
    ("tooltip.no_description", "(no description)"),
    ("error.no_help", "No help available for this command."),
    ("error.cmd_not_found", "HELP: command not found"),
    ("help.usage", "HELP [command]"),
    ("help.usage_desc1", "  Lists available commands with descriptions."),
    ("help.usage_desc2", "  HELP <command>     Shows detailed help for a specific command."),
    ("help.usage_desc3", "  HELP              Lists all commands."),
])

t("en-US", "neoshell", [
    ("prompt.startup_hint", "Type HELP for a list of commands."),
    ("error.invalid_drive", "Invalid drive"),
    ("error.ob_wait", "ob_wait error"),
    ("error.cd_not_found", "cd: directory not found"),
    ("error.bad_command", "Bad command or file name"),
    ("error.pipe", "Pipe error"),
    ("error.pipe_syntax", "Invalid pipe syntax"),
    ("error.pipe_builtin", "Cannot pipe built-in"),
    ("error.call_usage", "Usage: CALL batchfile"),
    ("error.call_not_found", "Batch file not found"),
    ("error.call_read", "Error reading batch"),
    ("status.poweroff", "powering off..."),
    ("prompt.pause", "Press any key to continue . . ."),
])

t("en-US", "neoinit", [
    ("status.boot", "NeoDOS Kernel v{ver} - The Rusty DOS Revival"),
    ("status.init_complete", "Initialization complete."),
    ("status.shell_spawn", "Spawning shell..."),
    ("error.reg_read", "Registry read error"),
    ("error.shell_not_found", "Shell not found."),
    ("status.registration", "Registration successful."),
    ("error.registration", "Registration failed."),
])

t("en-US", "coredir", [
    ("header.dir_of", "Directory of "),
    ("header.volume", " Volume in drive "),
    ("prompt.pause", "Press any key to continue . . ."),
    ("label.file_count", "File(s)"),
    ("label.bytes", "bytes"),
    ("label.dir", "<DIR>"),
    ("label.no_label", "has no label"),
    ("error.path_not_found", "Path not found"),
])

t("en-US", "corecopy", [
    ("error.missing_src", "COPY: missing source"),
    ("error.missing_src_dst", "COPY: missing source or destination"),
    ("error.same_file", "COPY: source and destination are the same"),
    ("error.read_failed", "COPY: cannot read source"),
    ("error.write_failed", "COPY: cannot write destination"),
    ("error.open_src", "COPY: cannot open source"),
    ("status.copied", " file(s) copied"),
])

t("en-US", "coretype", [
    ("error.file_not_found", "File not found"),
    ("error.read_error", "Error reading file"),
    ("error.usage", "Usage: TYPE <filename>"),
])

t("en-US", "neolocale", [
    ("tool.usage", "NeoLocale v0.1 — NLT translation file tool"),
    ("tool.validate", "  neolocale validate <file.nlt>     Validate format and structure"),
    ("tool.stats", "  neolocale stats    <file.nlt>     Show entry statistics"),
    ("tool.diff", "  neolocale diff     <f1> <f2>      Key-by-key differences"),
    ("tool.check", "  neolocale check    [dir]          Cross-locale missing check"),
    ("tool.create", "  neolocale create   <app> [locale] Empty NLT scaffold (stdout)"),
    ("status.valid", "VALID"),
    ("status.invalid", "INVALID"),
    ("error.cannot_open", "ERROR: cannot open file"),
    ("error.unknown_cmd", "Unknown command"),
])

# ═══════════════════════════════════════════════════════════════════════
# es-ES (Spanish)
# ═══════════════════════════════════════════════════════════════════════

t("es-ES", "corehelp", [
    ("help.header", "Herramientas Principal de NeoDOS"),
    ("help.commands_suffix", " comando(s) disponibles"),
    ("help.type_for_details", "Escriba AYUDA <comando> para detalles de un comando espec\u00edfico."),
    ("help.example", "  Ejemplo: AYUDA CLS"),
    ("error.no_programs_dir", "No se encontr\u00f3 el directorio de programas."),
    ("error.create_programs_dir", "Cree C:\\Programs con herramientas .NXE."),
    ("error.reading_dir", "(error al leer el directorio)"),
    ("tooltip.no_description", "(sin descripci\u00f3n)"),
    ("error.no_help", "No hay ayuda disponible para este comando."),
    ("error.cmd_not_found", "AYUDA: comando no encontrado"),
    ("help.usage", "AYUDA [comando]"),
    ("help.usage_desc1", "  Lista los comandos disponibles con descripciones."),
    ("help.usage_desc2", "  AYUDA <comando>    Muestra ayuda detallada de un comando."),
    ("help.usage_desc3", "  AYUDA              Lista todos los comandos."),
])

t("es-ES", "neoshell", [
    ("prompt.startup_hint", "Escriba AYUDA para una lista de comandos."),
    ("error.invalid_drive", "Unidad no v\u00e1lida"),
    ("error.ob_wait", "error de espera"),
    ("error.cd_not_found", "cd: directorio no encontrado"),
    ("error.bad_command", "Comando o nombre de archivo incorrecto"),
    ("error.pipe", "Error de tuber\u00eda"),
    ("error.pipe_syntax", "Sintaxis de tuber\u00eda no v\u00e1lida"),
    ("error.pipe_builtin", "No se puede usar tuber\u00eda con comandos internos"),
    ("error.call_usage", "Uso: CALL archivo.bat"),
    ("error.call_not_found", "Archivo batch no encontrado"),
    ("error.call_read", "Error al leer el archivo batch"),
    ("status.poweroff", "apagando..."),
    ("prompt.pause", "Presione una tecla para continuar . . ."),
])

t("es-ES", "neoinit", [
    ("status.boot", "NeoDOS Kernel v{ver} - El Renacimiento de DOS en Rust"),
    ("status.init_complete", "Inicializaci\u00f3n completa."),
    ("status.shell_spawn", "Iniciando el int\u00e9rprete de comandos..."),
    ("error.reg_read", "Error al leer el registro"),
    ("error.shell_not_found", "Int\u00e9rprete de comandos no encontrado."),
    ("status.registration", "Registro exitoso."),
    ("error.registration", "Error de registro."),
])

t("es-ES", "coredir", [
    ("header.dir_of", "Directorio de "),
    ("header.volume", " Volumen en unidad "),
    ("prompt.pause", "Presione una tecla para continuar . . ."),
    ("label.file_count", "Archivo(s)"),
    ("label.bytes", "bytes"),
    ("label.dir", "<DIR>"),
    ("label.no_label", "sin etiqueta"),
    ("error.path_not_found", "Ruta no encontrada"),
])

t("es-ES", "corecopy", [
    ("error.missing_src", "COPY: falta el origen"),
    ("error.missing_src_dst", "COPY: falta el origen o el destino"),
    ("error.same_file", "COPY: el origen y el destino son el mismo"),
    ("error.read_failed", "COPY: no se puede leer el origen"),
    ("error.write_failed", "COPY: no se puede escribir el destino"),
    ("error.open_src", "COPY: no se puede abrir el origen"),
    ("status.copied", " archivo(s) copiado(s)"),
])

t("es-ES", "coretype", [
    ("error.file_not_found", "Archivo no encontrado"),
    ("error.read_error", "Error al leer el archivo"),
    ("error.usage", "Uso: TYPE <archivo>"),
])

t("es-ES", "neolocale", [
    ("tool.usage", "NeoLocale v0.1 — Herramienta de archivos NLT"),
    ("tool.validate", "  neolocale validar <archivo.nlt>     Validar formato y estructura"),
    ("tool.stats", "  neolocale stats  <archivo.nlt>     Mostrar estad\u00edsticas"),
    ("tool.diff", "  neolocale diff   <a> <b>           Diferencias clave a clave"),
    ("tool.check", "  neolocale check  [dir]             Verificar traducciones faltantes"),
    ("tool.create", "  neolocale crear  <app> [locale]    Crear andamio NLT vac\u00edo"),
    ("status.valid", "V\u00c1LIDO"),
    ("status.invalid", "NO V\u00c1LIDO"),
    ("error.cannot_open", "ERROR: no se puede abrir el archivo"),
    ("error.unknown_cmd", "Comando desconocido"),
])

# ═══════════════════════════════════════════════════════════════════════
# NLT generation
# ═══════════════════════════════════════════════════════════════════════

def build_nlt(entries):
    """Build an NLT binary from a list of (key, value) pairs."""
    count = len(entries)
    key_offsets = []
    val_offsets = []

    # Key strings
    key_data = bytearray()
    for k, _ in entries:
        key_offsets.append(12 + count * 8 + len(key_data))
        key_data.extend(k.encode("utf-8") + b"\0")

    # Value strings
    val_data = bytearray()
    val_start = 12 + count * 8 + len(key_data)
    for _, v in entries:
        val_offsets.append(val_start + len(val_data))
        val_data.extend(v.encode("utf-8") + b"\0")

    # Build binary
    buf = bytearray()
    buf.extend(MAGIC)
    buf.extend(struct.pack("<I", VERSION))
    buf.extend(struct.pack("<I", count))
    for ko in key_offsets:
        buf.extend(struct.pack("<I", ko))
    for vo in val_offsets:
        buf.extend(struct.pack("<I", vo))
    buf.extend(key_data)
    buf.extend(val_data)
    return bytes(buf)


def generate_locale(locale, target_app=None):
    """Generate NLT files for a locale. If target_app given, only that app."""
    output_dir = LOCALE_DIR / locale
    output_dir.mkdir(parents=True, exist_ok=True)

    generated = 0
    for (loc, app), entries in sorted(TRANSLATIONS.items()):
        if loc != locale:
            continue
        if target_app and app != target_app:
            continue
        data = build_nlt(entries)
        out_path = output_dir / f"{app}.nlt"
        out_path.write_bytes(data)
        generated += 1
        print(f"  {app}.nlt  ({len(data)} bytes, {len(entries)} entries)")

    return generated


def main():
    os.makedirs(LOCALE_DIR, exist_ok=True)

    # Determine which locales/apps to generate
    target_locales = []
    target_app = None

    args = [a for a in sys.argv[1:] if not a.startswith("-")]

    if len(args) >= 1:
        # Specific locale(s) - could be "all", a locale name, or "en-US,es-ES"
        locales_arg = args[0]
        if locales_arg.lower() == "all":
            target_locales = sorted(set(loc for loc, _ in TRANSLATIONS))
        else:
            target_locales = [l.strip() for l in locales_arg.split(",")]
    else:
        target_locales = sorted(set(loc for loc, _ in TRANSLATIONS))

    if len(args) >= 2:
        target_app = args[1]

    # Generate
    total_files = 0
    for locale in target_locales:
        print(f"\n[{locale}]")
        n = generate_locale(locale, target_app)
        total_files += n

    print(f"\nGenerated {total_files} NLT file(s) in {LOCALE_DIR}")
    print("Locales:", ", ".join(sorted(set(loc for loc, _ in TRANSLATIONS))))


if __name__ == "__main__":
    main()
