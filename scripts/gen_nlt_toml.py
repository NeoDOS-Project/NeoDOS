#!/usr/bin/env python3
"""
gen_nlt_toml.py — Generate NLT TOML source files and compile them to NLTv2.

Usage:
  python3 scripts/gen_nlt_toml.py               # generate all .toml + compile to .nlt
  python3 scripts/gen_nlt_toml.py --toml-only   # only generate .toml, no compilation
"""

import os, sys, subprocess
from pathlib import Path

LOCALE_DIR = Path(__file__).resolve().parent.parent / "data" / "locale"
NLTC = str(Path(__file__).resolve().parent.parent / "tools" / "nltc" / "target" / "debug" / "nltc")

# ── Translation data ────────────────────────────────────────────────────
# Format: (locale, app, [ (IDS_NAME, id, string), ... ])

TRANSLATIONS = {}

def t(locale, app, entries):
    key = (locale, app)
    if key not in TRANSLATIONS:
        TRANSLATIONS[key] = []
    TRANSLATIONS[key].extend(entries)

# ── en-US ───────────────────────────────────────────────────────────────

t("en-US", "corehelp", [
    ("IDS_HEADER", 1001, "NeoDOS Core Tools"),
    ("IDS_COMMANDS_SUFFIX", 1002, " command(s) available"),
    ("IDS_TYPE_FOR_DETAILS", 1003, "Type HELP <command> for details on a specific command."),
    ("IDS_EXAMPLE", 1004, "  Example: HELP CLS"),
    ("IDS_NO_PROGRAMS_DIR", 1005, "No Programs directory found."),
    ("IDS_CREATE_PROGRAMS_DIR", 1006, "Create C:\\Programs with .NXE tools."),
    ("IDS_ERROR_READING_DIR", 1007, "(error reading directory)"),
    ("IDS_NO_DESCRIPTION", 1008, "(no description)"),
    ("IDS_NO_HELP", 1009, "No help available for this command."),
    ("IDS_CMD_NOT_FOUND", 1010, "HELP: command not found"),
    ("IDS_HELP_USAGE", 1011, "HELP [command]"),
    ("IDS_USAGE_DESC1", 1012, "  Lists available commands with descriptions."),
    ("IDS_USAGE_DESC2", 1013, "  HELP <command>     Shows detailed help for a specific command."),
    ("IDS_USAGE_DESC3", 1014, "  HELP              Lists all commands."),
])

t("en-US", "neoshell", [
    ("IDS_STARTUP_HINT", 1001, "Type HELP for a list of commands."),
    ("IDS_INVALID_DRIVE", 1002, "Invalid drive"),
    ("IDS_OB_WAIT_ERROR", 1003, "ob_wait error"),
    ("IDS_CD_NOT_FOUND", 1004, "cd: directory not found"),
    ("IDS_BAD_COMMAND", 1005, "Bad command or file name"),
    ("IDS_PIPE_ERROR", 1006, "Pipe error"),
    ("IDS_PIPE_SYNTAX", 1007, "Invalid pipe syntax"),
    ("IDS_PIPE_BUILTIN", 1008, "Cannot pipe built-in"),
    ("IDS_CALL_USAGE", 1009, "Usage: CALL batchfile"),
    ("IDS_CALL_NOT_FOUND", 1010, "Batch file not found"),
    ("IDS_CALL_READ_ERROR", 1011, "Error reading batch"),
    ("IDS_STATUS_POWEROFF", 1012, "powering off..."),
    ("IDS_PROMPT_PAUSE", 1013, "Press any key to continue . . ."),
])

t("en-US", "neoinit", [
    ("IDS_STATUS_BOOT", 1001, "NeoDOS Kernel v{ver} - The Rusty DOS Revival"),
    ("IDS_INIT_COMPLETE", 1002, "Initialization complete."),
    ("IDS_SHELL_SPAWN", 1003, "Spawning shell..."),
    ("IDS_REG_READ_ERROR", 1004, "Registry read error"),
    ("IDS_SHELL_NOT_FOUND", 1005, "Shell not found."),
    ("IDS_REGISTRATION_OK", 1006, "Registration successful."),
    ("IDS_REGISTRATION_FAIL", 1007, "Registration failed."),
])

t("en-US", "coredir", [
    ("IDS_DIR_OF", 1001, "Directory of "),
    ("IDS_VOLUME", 1002, " Volume in drive "),
    ("IDS_PROMPT_PAUSE", 1003, "Press any key to continue . . ."),
    ("IDS_FILE_COUNT", 1004, "File(s)"),
    ("IDS_BYTES", 1005, "bytes"),
    ("IDS_DIR_LABEL", 1006, "<DIR>"),
    ("IDS_NO_LABEL", 1007, "has no label"),
    ("IDS_PATH_NOT_FOUND", 1008, "Path not found"),
])

t("en-US", "corecopy", [
    ("IDS_MISSING_SRC", 1001, "COPY: missing source"),
    ("IDS_MISSING_SRC_DST", 1002, "COPY: missing source or destination"),
    ("IDS_SAME_FILE", 1003, "COPY: source and destination are the same"),
    ("IDS_READ_FAILED", 1004, "COPY: cannot read source"),
    ("IDS_WRITE_FAILED", 1005, "COPY: cannot write destination"),
    ("IDS_OPEN_SRC_FAILED", 1006, "COPY: cannot open source"),
    ("IDS_COPIED", 1007, " file(s) copied"),
])

t("en-US", "coretype", [
    ("IDS_FILE_NOT_FOUND", 1001, "File not found"),
    ("IDS_READ_ERROR", 1002, "Error reading file"),
    ("IDS_USAGE", 1003, "Usage: TYPE <filename>"),
])

t("en-US", "neolocale", [
    ("IDS_TOOL_USAGE", 1001, "NeoLocale v0.2 — NLT translation file tool"),
    ("IDS_TOOL_VALIDATE", 1002, "  neolocale validate <file.nlt>     Validate format and structure"),
    ("IDS_TOOL_STATS", 1003, "  neolocale stats    <file.nlt>     Show entry statistics"),
    ("IDS_TOOL_DIFF", 1004, "  neolocale diff     <f1> <f2>      Key-by-key differences"),
    ("IDS_TOOL_CHECK", 1005, "  neolocale check    [dir]          Cross-locale missing check"),
    ("IDS_TOOL_CREATE", 1006, "  neolocale create   <app> [locale] Empty NLT scaffold (stdout)"),
    ("IDS_STATUS_VALID", 1007, "VALID"),
    ("IDS_STATUS_INVALID", 1008, "INVALID"),
    ("IDS_ERROR_CANNOT_OPEN", 1009, "ERROR: cannot open file"),
    ("IDS_ERROR_UNKNOWN_CMD", 1010, "Unknown command"),
])

# ── es-ES ───────────────────────────────────────────────────────────────

t("es-ES", "corehelp", [
    ("IDS_HEADER", 1001, "Herramientas Principal de NeoDOS"),
    ("IDS_COMMANDS_SUFFIX", 1002, " comando(s) disponibles"),
    ("IDS_TYPE_FOR_DETAILS", 1003, "Escriba AYUDA <comando> para detalles de un comando espec\u00edfico."),
    ("IDS_EXAMPLE", 1004, "  Ejemplo: AYUDA CLS"),
    ("IDS_NO_PROGRAMS_DIR", 1005, "No se encontr\u00f3 el directorio de programas."),
    ("IDS_CREATE_PROGRAMS_DIR", 1006, "Cree C:\\Programs con herramientas .NXE."),
    ("IDS_ERROR_READING_DIR", 1007, "(error al leer el directorio)"),
    ("IDS_NO_DESCRIPTION", 1008, "(sin descripci\u00f3n)"),
    ("IDS_NO_HELP", 1009, "No hay ayuda disponible para este comando."),
    ("IDS_CMD_NOT_FOUND", 1010, "AYUDA: comando no encontrado"),
    ("IDS_HELP_USAGE", 1011, "AYUDA [comando]"),
    ("IDS_USAGE_DESC1", 1012, "  Lista los comandos disponibles con descripciones."),
    ("IDS_USAGE_DESC2", 1013, "  AYUDA <comando>    Muestra ayuda detallada de un comando."),
    ("IDS_USAGE_DESC3", 1014, "  AYUDA              Lista todos los comandos."),
])

t("es-ES", "neoshell", [
    ("IDS_STARTUP_HINT", 1001, "Escriba AYUDA para una lista de comandos."),
    ("IDS_INVALID_DRIVE", 1002, "Unidad no v\u00e1lida"),
    ("IDS_OB_WAIT_ERROR", 1003, "error de espera"),
    ("IDS_CD_NOT_FOUND", 1004, "cd: directorio no encontrado"),
    ("IDS_BAD_COMMAND", 1005, "Comando o nombre de archivo incorrecto"),
    ("IDS_PIPE_ERROR", 1006, "Error de tuber\u00eda"),
    ("IDS_PIPE_SYNTAX", 1007, "Sintaxis de tuber\u00eda no v\u00e1lida"),
    ("IDS_PIPE_BUILTIN", 1008, "No se puede usar tuber\u00eda con comandos internos"),
    ("IDS_CALL_USAGE", 1009, "Uso: CALL archivo.bat"),
    ("IDS_CALL_NOT_FOUND", 1010, "Archivo batch no encontrado"),
    ("IDS_CALL_READ_ERROR", 1011, "Error al leer el archivo batch"),
    ("IDS_STATUS_POWEROFF", 1012, "apagando..."),
    ("IDS_PROMPT_PAUSE", 1013, "Presione una tecla para continuar . . ."),
])

t("es-ES", "neoinit", [
    ("IDS_STATUS_BOOT", 1001, "NeoDOS Kernel v{ver} - El Renacimiento de DOS en Rust"),
    ("IDS_INIT_COMPLETE", 1002, "Inicializaci\u00f3n completa."),
    ("IDS_SHELL_SPAWN", 1003, "Iniciando el int\u00e9rprete de comandos..."),
    ("IDS_REG_READ_ERROR", 1004, "Error al leer el registro"),
    ("IDS_SHELL_NOT_FOUND", 1005, "Int\u00e9rprete de comandos no encontrado."),
    ("IDS_REGISTRATION_OK", 1006, "Registro exitoso."),
    ("IDS_REGISTRATION_FAIL", 1007, "Error de registro."),
])

t("es-ES", "coredir", [
    ("IDS_DIR_OF", 1001, "Directorio de "),
    ("IDS_VOLUME", 1002, " Volumen en unidad "),
    ("IDS_PROMPT_PAUSE", 1003, "Presione una tecla para continuar . . ."),
    ("IDS_FILE_COUNT", 1004, "Archivo(s)"),
    ("IDS_BYTES", 1005, "bytes"),
    ("IDS_DIR_LABEL", 1006, "<DIR>"),
    ("IDS_NO_LABEL", 1007, "sin etiqueta"),
    ("IDS_PATH_NOT_FOUND", 1008, "Ruta no encontrada"),
])

t("es-ES", "corecopy", [
    ("IDS_MISSING_SRC", 1001, "COPY: falta el origen"),
    ("IDS_MISSING_SRC_DST", 1002, "COPY: falta el origen o el destino"),
    ("IDS_SAME_FILE", 1003, "COPY: el origen y el destino son el mismo"),
    ("IDS_READ_FAILED", 1004, "COPY: no se puede leer el origen"),
    ("IDS_WRITE_FAILED", 1005, "COPY: no se puede escribir el destino"),
    ("IDS_OPEN_SRC_FAILED", 1006, "COPY: no se puede abrir el origen"),
    ("IDS_COPIED", 1007, " archivo(s) copiado(s)"),
])

t("es-ES", "coretype", [
    ("IDS_FILE_NOT_FOUND", 1001, "Archivo no encontrado"),
    ("IDS_READ_ERROR", 1002, "Error al leer el archivo"),
    ("IDS_USAGE", 1003, "Uso: TYPE <archivo>"),
])

t("es-ES", "neolocale", [
    ("IDS_TOOL_USAGE", 1001, "NeoLocale v0.2 — Herramienta de archivos NLT"),
    ("IDS_TOOL_VALIDATE", 1002, "  neolocale validar <archivo.nlt>     Validar formato y estructura"),
    ("IDS_TOOL_STATS", 1003, "  neolocale stats  <archivo.nlt>     Mostrar estad\u00edsticas"),
    ("IDS_TOOL_DIFF", 1004, "  neolocale diff   <a> <b>           Diferencias clave a clave"),
    ("IDS_TOOL_CHECK", 1005, "  neolocale check  [dir]             Verificar traducciones faltantes"),
    ("IDS_TOOL_CREATE", 1006, "  neolocale crear  <app> [locale]    Crear andamio NLT vac\u00edo"),
    ("IDS_STATUS_VALID", 1007, "V\u00c1LIDO"),
    ("IDS_STATUS_INVALID", 1008, "NO V\u00c1LIDO"),
    ("IDS_ERROR_CANNOT_OPEN", 1009, "ERROR: no se puede abrir el archivo"),
    ("IDS_ERROR_UNKNOWN_CMD", 1010, "Comando desconocido"),
])

# ── TOML generation ─────────────────────────────────────────────────────

def generate_toml(locale, app, entries):
    """Generate TOML source content from entries."""
    lines = []
    lines.append("[meta]")
    lines.append(f'app = "{app}"')
    lines.append(f'language = "{locale}"')
    lines.append("")
    lines.append("[ids]")
    for name, sid, _string in entries:
        lines.append(f"{name} = {sid}")
    lines.append("")
    lines.append("[strings]")
    for name, _sid, string in entries:
        escaped = string.replace("\\", "\\\\").replace('"', '\\"')
        lines.append(f'{name} = "{escaped}"')
    lines.append("")
    return "\n".join(lines)


def main():
    only_toml = "--toml-only" in sys.argv

    for (locale, app), entries in sorted(TRANSLATIONS.items()):
        app_dir = LOCALE_DIR / locale
        app_dir.mkdir(parents=True, exist_ok=True)
        toml_path = app_dir / f"{app}.toml"

        content = generate_toml(locale, app, entries)
        toml_path.write_text(content, encoding="utf-8")
        print(f"  [TOML] {locale}/{app}.toml  ({len(entries)} entries)")

        if not only_toml and os.path.exists(NLTC):
            nlt_path = app_dir / f"{app}.nlt"
            result = subprocess.run(
                [NLTC, str(toml_path), str(nlt_path)],
                capture_output=True, text=True
            )
            if result.returncode == 0:
                out = result.stderr.strip()
                if out:
                    print(f"    {out}")
            else:
                print(f"    ERROR: {result.stderr}")

    if only_toml:
        print("\nTOML files generated. Compile with: nltc --generate-all <locale-dir>")

    print("\nDone.")


if __name__ == "__main__":
    main()
