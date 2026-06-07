#!/bin/bash
# MCP Server for NeoDOS
# Launches the MCP stdio server for kernel introspection, VFS, and module analysis.
#
# Usage:
#   bash scripts/mcp-server.sh                          # stdio mode (for AI integration)
#   NEODOS_ROOT=/path/to/neodos bash scripts/mcp-server.sh
#   bash scripts/mcp-server.sh --tool kernel_index      # one-shot tool call
#   bash scripts/mcp-server.sh --help

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
NEODOS_ROOT="${NEODOS_ROOT:-"$(cd "$SCRIPT_DIR/.." && pwd)"}"

export NEODOS_ROOT="$NEODOS_ROOT"
export PYTHONPATH="$SCRIPT_DIR:$PYTHONPATH"

if [ "$1" = "--help" ] || [ "$1" = "-h" ]; then
    exec python3 -m mcp_server --help
elif [ "$1" = "--tool" ]; then
    exec python3 -m mcp_server "$@"
else
    exec python3 -m mcp_server
fi
