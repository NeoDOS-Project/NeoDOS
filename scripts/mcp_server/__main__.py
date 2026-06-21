#!/usr/bin/env python3
"""
MCP Server for NeoDOS — Kernel Introspection, VFS, Runtime Module Analysis.

Usage:
  python -m mcp_server                                    # stdio MCP server
  python -m mcp_server --tool vfs_list path=\\\\            # one-shot tool call
  python -m mcp_server --help                              # this message

Environment:
  NEODOS_ROOT: path to neodos/ directory (auto-detected from CWD or parent)
"""

import os
import sys
import json
from pathlib import Path


# ── Auto-detect NEODOS_ROOT ──

def _find_neodos_root() -> str:
    """Walk up from CWD looking for AGENTS.md with 'NeoDOS' in it."""
    cwd = Path.cwd().resolve()
    for parent in [cwd] + list(cwd.parents):
        agents = parent / "AGENTS.md"
        if agents.exists():
            for line in open(agents):
                if "NeoDOS" in line:
                    return str(parent)
        # Alternative: check for neodos-kernel directory
        if (parent / "neodos-kernel").is_dir():
            return str(parent)
    return str(cwd)


# ── Server Builder ──

def build_server(root_dir: str):
    """Build and configure the MCP server with all tools, resources, and prompts."""
    from .server import McpServer, ToolSpec, ResourceSpec, PromptSpec

    # Configure tool modules
    from .tools import kernel_tools, vfs_tools, module_tools, libneodos_tools, system_tools, lsp_tools

    kernel_tools.configure(root_dir)
    vfs_tools.configure(root_dir)
    module_tools.configure(root_dir)
    libneodos_tools.configure(root_dir)
    system_tools.configure(root_dir)
    lsp_tools.configure(root_dir)

    server = McpServer(name="neodos-mcp", version="0.28.0")

    # ── Tools ──

    # Kernel introspection
    server.register_tool(ToolSpec(
        name="kernel_index",
        description="List all kernel source files with line counts, grouped by subsystem",
        input_schema={
            "type": "object",
            "properties": {
                "format": {
                    "type": "string",
                    "enum": ["tree", "summary"],
                    "description": "Output format: 'tree' (full hierarchy) or 'summary' (counts only)",
                    "default": "tree",
                }
            },
        },
        handler=kernel_tools.kernel_index,
    ))

    server.register_tool(ToolSpec(
        name="search_symbol",
        description="Search for function, struct, trait, or const definitions in kernel source",
        input_schema={
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Symbol name to search for (case-insensitive, partial match)",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results to return (default: 30)",
                    "default": 30,
                },
            },
            "required": ["query"],
        },
        handler=kernel_tools.search_symbol,
    ))

    server.register_tool(ToolSpec(
        name="get_kernel_architecture",
        description="Return kernel memory layout, boot phase sequence, subsystem boundaries, and forbidden dependencies",
        input_schema={
            "type": "object",
            "properties": {},
        },
        handler=kernel_tools.get_kernel_architecture,
    ))

    server.register_tool(ToolSpec(
        name="get_build_errors",
        description="Check for build issues: duplicate code, missing artifacts, NEM/ABI mismatches",
        input_schema={
            "type": "object",
            "properties": {},
        },
        handler=kernel_tools.get_build_errors,
    ))

    # VFS tools
    server.register_tool(ToolSpec(
        name="vfs_list",
        description="List directory contents from NeoDOS filesystem via VFS",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path (e.g. \\ or \\System\\Drivers)",
                    "default": "\\",
                },
                "drive": {
                    "type": "string",
                    "description": "Drive letter (C, D, A)",
                    "default": "C",
                },
            },
        },
        handler=vfs_tools.vfs_list,
    ))

    server.register_tool(ToolSpec(
        name="vfs_read",
        description="Read a file from NeoDOS filesystem via VFS. Shows text or hex dump",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path (e.g. \\README.TXT)",
                },
                "drive": {
                    "type": "string",
                    "description": "Drive letter (C, D, A)",
                    "default": "C",
                },
            },
            "required": ["path"],
        },
        handler=vfs_tools.vfs_read,
    ))

    server.register_tool(ToolSpec(
        name="vfs_stat",
        description="Get file/directory metadata (inode, size, permissions) via VFS",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File or directory path",
                },
                "drive": {
                    "type": "string",
                    "description": "Drive letter",
                    "default": "C",
                },
            },
            "required": ["path"],
        },
        handler=vfs_tools.vfs_stat,
    ))

    server.register_tool(ToolSpec(
        name="vfs_resolve",
        description="Resolve a path through VFS with fallback search across all drives",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to resolve (with or without drive letter)",
                },
                "drive": {
                    "type": "string",
                    "description": "Primary drive letter",
                    "default": "C",
                },
            },
            "required": ["path"],
        },
        handler=vfs_tools.vfs_resolve,
    ))

    server.register_tool(ToolSpec(
        name="vfs_tree",
        description="Recursive directory tree listing of NeoDOS filesystem",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Root path for tree",
                    "default": "\\",
                },
                "drive": {
                    "type": "string",
                    "description": "Drive letter",
                    "default": "C",
                },
            },
        },
        handler=vfs_tools.vfs_tree,
    ))

    server.register_tool(ToolSpec(
        name="vfs_dump_superblock",
        description="Dump NeoDOS filesystem superblock details",
        input_schema={
            "type": "object",
            "properties": {
                "drive": {
                    "type": "string",
                    "description": "Drive letter",
                    "default": "C",
                },
            },
        },
        handler=vfs_tools.vfs_dump_superblock,
    ))

    server.register_tool(ToolSpec(
        name="vfs_dump_inodes",
        description="Dump NeoDOS inode table",
        input_schema={
            "type": "object",
            "properties": {
                "drive": {
                    "type": "string",
                    "description": "Drive letter",
                    "default": "C",
                },
            },
        },
        handler=vfs_tools.vfs_dump_inodes,
    ))

    # Runtime module tools
    server.register_tool(ToolSpec(
        name="list_loaded_modules",
        description="List NEM drivers and NXLs found in build artifacts",
        input_schema={
            "type": "object",
            "properties": {
                "category": {
                    "type": "string",
                    "enum": ["all", "nem", "dll"],
                    "description": "Module category to list",
                    "default": "all",
                },
            },
        },
        handler=module_tools.list_loaded_modules,
    ))

    server.register_tool(ToolSpec(
        name="get_module_symbols",
        description="Get symbols, exports, and relocations for a NEM driver or DLL",
        input_schema={
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Module name (partial match)",
                },
                "format": {
                    "type": "string",
                    "enum": ["detailed", "compact"],
                    "description": "Output detail level",
                    "default": "detailed",
                },
            },
            "required": ["name"],
        },
        handler=module_tools.get_module_symbols,
    ))

    server.register_tool(ToolSpec(
        name="sys_loadlib_analyze",
        description="Analyze what sys_loadlib would do with a module — format, ABI, dependencies (read-only)",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to module file (searches build output)",
                },
            },
            "required": ["path"],
        },
        handler=module_tools.sys_loadlib_analyze,
    ))

    # libneodos API tools
    server.register_tool(ToolSpec(
        name="analyze_libneodos_api",
        description="Analyze the libneodos userspace library: ABI table, syscall wrappers, error codes",
        input_schema={
            "type": "object",
            "properties": {
                "detail": {
                    "type": "string",
                    "enum": ["summary", "full"],
                    "description": "Detail level",
                    "default": "summary",
                },
            },
        },
        handler=libneodos_tools.analyze_libneodos_api,
    ))

    server.register_tool(ToolSpec(
        name="check_abi_compatibility",
        description="Check ABI compatibility between a NEM driver and the kernel",
        input_schema={
            "type": "object",
            "properties": {
                "module_type": {
                    "type": "string",
                    "enum": ["nem", "elf"],
                    "description": "Module type",
                    "default": "nem",
                },
                "abi_min": {
                    "type": "integer",
                    "description": "Driver's minimum ABI version (required for NEM)",
                },
                "abi_target": {
                    "type": "integer",
                    "description": "Driver's target ABI version (required for NEM)",
                },
                "abi_max": {
                    "type": "integer",
                    "description": "Driver's maximum ABI version (required for NEM)",
                },
            },
            "required": ["module_type"],
        },
        handler=libneodos_tools.check_abi_compatibility,
    ))

    server.register_tool(ToolSpec(
        name="analyze_libneodos_coverage",
        description="Show which kernel syscalls have libneodos wrappers and which are missing",
        input_schema={
            "type": "object",
            "properties": {},
        },
        handler=libneodos_tools.analyze_libneodos_coverage,
    ))

    # System consistency tools
    server.register_tool(ToolSpec(
        name="check_consistency",
        description="Validate architectural consistency: code, docs, artifacts, invariants",
        input_schema={
            "type": "object",
            "properties": {
                "targets": {
                    "type": "string",
                    "enum": ["all", "code", "docs", "artifacts", "invariants"],
                    "description": "What to check",
                    "default": "all",
                },
            },
        },
        handler=system_tools.check_consistency,
    ))

    # ── LSP Analysis Tools ──

    server.register_tool(ToolSpec(
        name="lsp_list_symbols",
        description="List all Rust symbols (functions, structs, enums, traits, consts, modules) in a file or directory",
        input_schema={
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File or directory path relative to neodos/ (default: neodos-kernel/src)",
                    "default": "",
                },
                "recursive": {
                    "type": "boolean",
                    "description": "If True and path is a directory, recurse into subdirectories",
                    "default": False,
                },
            },
        },
        handler=lsp_tools.lsp_list_symbols,
    ))

    server.register_tool(ToolSpec(
        name="lsp_search_symbol",
        description="Search for symbols (functions, structs, enums, traits) across the codebase by name pattern",
        input_schema={
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Symbol name to search for (case-insensitive, partial match)",
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum results (default: 30)",
                    "default": 30,
                },
            },
            "required": ["query"],
        },
        handler=lsp_tools.lsp_search_symbol,
    ))

    server.register_tool(ToolSpec(
        name="lsp_get_syscalls",
        description="List all NeoDOS syscalls with numbers, handler names, and source locations",
        input_schema={"type": "object", "properties": {}},
        handler=lsp_tools.lsp_get_syscalls,
    ))

    server.register_tool(ToolSpec(
        name="lsp_get_shell_commands",
        description="List all NeoDOS shell commands with categories and descriptions",
        input_schema={"type": "object", "properties": {}},
        handler=lsp_tools.lsp_get_shell_commands,
    ))

    server.register_tool(ToolSpec(
        name="lsp_get_capabilities",
        description="List all NeoDOS capability flags with bit values and descriptions",
        input_schema={"type": "object", "properties": {}},
        handler=lsp_tools.lsp_get_capabilities,
    ))

    server.register_tool(ToolSpec(
        name="lsp_get_diagnostics",
        description="Run basic source code diagnostics on a Rust file: unbalanced delimiters, line length, style issues",
        input_schema={
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to .rs file, relative to neodos/",
                },
            },
            "required": ["file_path"],
        },
        handler=lsp_tools.lsp_get_diagnostics,
    ))

    server.register_tool(ToolSpec(
        name="lsp_get_driver_states",
        description="List all NEM driver lifecycle states and valid transitions",
        input_schema={"type": "object", "properties": {}},
        handler=lsp_tools.lsp_get_driver_states,
    ))

    server.register_tool(ToolSpec(
        name="lsp_get_kernel_modules",
        description="List all kernel modules/subsystems with responsibilities and file counts",
        input_schema={"type": "object", "properties": {}},
        handler=lsp_tools.lsp_get_kernel_modules,
    ))

    # ── Resources ──

    server.register_resource(ResourceSpec(
        uri="neodos://system/info",
        name="System Information",
        description="Overview of NeoDOS project structure, version, and build artifacts",
        mime_type="text/plain",
        handler=system_tools.get_system_resource,
    ))

    server.register_resource(ResourceSpec(
        uri="neodos://kernel/architecture",
        name="Kernel Architecture",
        description="Kernel memory layout, boot phases, subsystem boundaries",
        mime_type="text/plain",
        handler=kernel_tools.get_kernel_architecture,
    ))

    server.register_resource(ResourceSpec(
        uri="neodos://libneodos/api",
        name="libneodos API Reference",
        description="Complete libneodos ABI table, error constants, syscall mapping",
        mime_type="text/plain",
        handler=lambda: libneodos_tools.analyze_libneodos_api(detail="full"),
    ))

    # ── Prompts ──

    server.register_prompt(PromptSpec(
        name="analyze_system_state",
        description="Analyze current NeoDOS system state: version, modules, ABI, consistency",
        arguments=[
            {
                "name": "focus",
                "description": "Analysis focus area",
                "required": False,
                "type": "string",
                "enum": ["full", "kernel", "vfs", "modules", "libneodos"],
            }
        ],
        handler=lambda focus="full": {
            "description": "Comprehensive NeoDOS system state analysis",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": (
                            f"Analyze NeoDOS system state with focus: {focus}. "
                            "Check kernel version, loaded modules, VFS filesystem, "
                            "libneodos API coverage, and architectural consistency."
                        ),
                    },
                }
            ],
        },
    ))

    server.register_prompt(PromptSpec(
        name="debug_module_loading",
        description="Debug why a NEM driver or DLL fails to load",
        arguments=[
            {
                "name": "module_path",
                "description": "Path to the module file",
                "required": True,
                "type": "string",
            }
        ],
        handler=lambda module_path: {
            "description": f"Debug loading of {module_path}",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": (
                            f"Debug why '{module_path}' fails to load in NeoDOS. "
                            "Check: file format, ABI compatibility, unresolved symbols, "
                            "capability requirements, isolation region allocation."
                        ),
                    },
                }
            ],
        },
    ))

    server.register_prompt(PromptSpec(
        name="analyze_vfs_path",
        description="Trace a VFS path resolution through NeoDOS filesystem",
        arguments=[
            {
                "name": "path",
                "description": "Path to trace (e.g. \\System\\Drivers\\keyboard.nem)",
                "required": True,
                "type": "string",
            }
        ],
        handler=lambda path: {
            "description": f"Trace VFS path: {path}",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": (
                            f"Trace the VFS path resolution for '{path}' through NeoDOS filesystem. "
                            "Show directory walk, inode lookup, file metadata, and permissions."
                        ),
                    },
                }
            ],
        },
    ))

    return server


# ── CLI Entry Points ──

def main():
    root_dir = os.environ.get("NEODOS_ROOT", _find_neodos_root())
    os.environ["NEODOS_ROOT"] = root_dir

    # Support --tool for one-shot invocation
    if len(sys.argv) > 2 and sys.argv[1] == "--tool":
        tool_name = sys.argv[2]
        args = {}
        for arg in sys.argv[3:]:
            if "=" in arg:
                k, v = arg.split("=", 1)
                args[k] = v

        server = build_server(root_dir)
        tool = server.tools.get(tool_name)
        if tool is None:
            print(json.dumps({"error": f"Unknown tool: {tool_name}"}))
            sys.exit(1)

        # Coerce types based on tool schema
        schema = tool.input_schema
        props = schema.get("properties", {})
        coerced = {}
        for k, v in args.items():
            prop = props.get(k, {})
            ptype = prop.get("type", "string")
            if ptype == "integer":
                try:
                    coerced[k] = int(v)
                except (ValueError, TypeError):
                    coerced[k] = v
            elif ptype == "number":
                try:
                    coerced[k] = float(v)
                except (ValueError, TypeError):
                    coerced[k] = v
            elif ptype == "boolean":
                coerced[k] = v.lower() in ("true", "1", "yes")
            else:
                coerced[k] = v

        try:
            result = tool.handler(**coerced)
            print(result)
        except Exception as e:
            import traceback
            print(json.dumps({"error": str(e), "traceback": traceback.format_exc()}))
            sys.exit(1)
        return

    if "--help" in sys.argv or "-h" in sys.argv:
        print(__doc__)
        print(f"\nNEODOS_ROOT = {root_dir}")
        return

    # Normal MCP server mode (stdio)
    server = build_server(root_dir)
    try:
        server.run_stdio()
    except KeyboardInterrupt:
        sys.exit(0)


if __name__ == "__main__":
    main()
