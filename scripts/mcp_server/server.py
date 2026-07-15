#!/usr/bin/env python3
"""
mcp_server/server.py — Core MCP Protocol Engine for NeoDOS.

Implements the Model Context Protocol (MCP) over stdio transport
using JSON-RPC 2.0. Provides kernel introspection, VFS analysis,
runtime module inspection, and architectural validation for NeoDOS.

Protocol:
  - Transport: stdio (one JSON-RPC message per line)
  - Version: 1.0 (MCP draft)
  - All messages are JSON-RPC 2.0

Usage:
  python -m mcp_server           # stdio transport (default)
  python -m mcp_server --sse     # SSE transport (not yet)

Environment:
  NEODOS_ROOT: path to neodos/ directory (default: auto-detect)
"""

import json
import sys
import os
import re
import logging
import traceback
from typing import Any, Callable
from pathlib import Path


# ── Logging ──

logging.basicConfig(
    level=logging.INFO,
    format="[MCP] %(levelname)s %(message)s",
    stream=sys.stderr,
)
log = logging.getLogger("mcp")


# ── Version from AGENTS.md ──

def get_neodos_version(root_dir: str = None) -> str:
    """Read version from AGENTS.md to avoid hardcoding."""
    if root_dir is None:
        root_dir = os.environ.get("NEODOS_ROOT", "")
    agents = Path(root_dir) / "AGENTS.md"
    if agents.exists():
        for line in open(agents):
            m = re.match(r'\*\*Version:\*\* v([\w.-]+)', line)
            if m:
                return m.group(1)
    return "dev"


# ── MCP Protocol Constants ──

MCP_VERSION = "1.0"
JSON_RPC_VERSION = "2.0"

# ── Error Codes (JSON-RPC standard + MCP extensions) ──

class McpError(Exception):
    def __init__(self, code: int, message: str, data: Any = None):
        self.code = code
        self.message = message
        self.data = data

class ParseError(McpError):
    def __init__(self, msg="-32700: Parse error"):
        super().__init__(-32700, msg)

class InvalidRequest(McpError):
    def __init__(self, msg="-32600: Invalid Request"):
        super().__init__(-32600, msg)

class MethodNotFound(McpError):
    def __init__(self, msg="-32601: Method not found"):
        super().__init__(-32601, msg)

class InvalidParams(McpError):
    def __init__(self, msg="-32602: Invalid params"):
        super().__init__(-32602, msg)

class InternalError(McpError):
    def __init__(self, msg="-32603: Internal error"):
        super().__init__(-32603, msg)


# ── JSON-RPC Message Helpers ──

def make_request(method: str, params: dict = None, id_: Any = None) -> str:
    msg = {"jsonrpc": JSON_RPC_VERSION, "method": method}
    if params is not None:
        msg["params"] = params
    if id_ is not None:
        msg["id"] = id_
    return json.dumps(msg)

def make_response(id_: Any, result: Any = None) -> str:
    msg = {"jsonrpc": JSON_RPC_VERSION, "id": id_, "result": result}
    return json.dumps(msg)

def make_error(id_: Any, code: int, message: str, data: Any = None) -> str:
    err = {"code": code, "message": message}
    if data is not None:
        err["data"] = data
    msg = {"jsonrpc": JSON_RPC_VERSION, "id": id_, "error": err}
    return json.dumps(msg)


# ── Tool/Resource/Prompt Descriptors ──

class ToolSpec:
    def __init__(self, name: str, description: str, input_schema: dict, handler: Callable):
        self.name = name
        self.description = description
        self.input_schema = input_schema
        self.handler = handler

    def to_dict(self) -> dict:
        return {
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        }


class ResourceSpec:
    def __init__(self, uri: str, name: str, description: str, mime_type: str, handler: Callable):
        self.uri = uri
        self.name = name
        self.description = description
        self.mime_type = mime_type
        self.handler = handler

    def to_dict(self) -> dict:
        return {
            "uri": self.uri,
            "name": self.name,
            "description": self.description,
            "mimeType": self.mime_type,
        }


class PromptSpec:
    def __init__(self, name: str, description: str, arguments: list, handler: Callable):
        self.name = name
        self.description = description
        self.arguments = arguments
        self.handler = handler

    def to_dict(self) -> dict:
        d = {"name": self.name, "description": self.description}
        if self.arguments:
            d["arguments"] = self.arguments
        return d


# ── MCP Server Engine ──

class McpServer:
    def __init__(self, name: str = "neodos-mcp", version: str = None):
        self.name = name
        self.version = version or get_neodos_version()
        self.tools: dict[str, ToolSpec] = {}
        self.resources: dict[str, ResourceSpec] = {}
        self.resource_templates: dict[str, ResourceSpec] = {}
        self.prompts: dict[str, PromptSpec] = {}
        self._initialized = False
        self._client_capabilities = {}
        self._server_capabilities = {
            "tools": {},
            "resources": {},
            "prompts": {},
            "resourceTemplates": {},
        }

    def register_tool(self, spec: ToolSpec):
        self.tools[spec.name] = spec

    def register_resource(self, spec: ResourceSpec):
        self.resources[spec.uri] = spec

    def register_resource_template(self, uri_template: str, spec: ResourceSpec):
        self.resource_templates[uri_template] = spec

    def register_prompt(self, spec: PromptSpec):
        self.prompts[spec.name] = spec

    # ── Message Dispatch ──

    def handle_line(self, line: str) -> str | None:
        """Process one JSON-RPC message. Returns response line or None for notifications."""
        line = line.strip()
        if not line:
            return None

        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            return make_error(None, -32700, f"Parse error: {e}")

        if not isinstance(msg, dict) or msg.get("jsonrpc") != JSON_RPC_VERSION:
            return make_error(msg.get("id"), -32600, "Invalid Request: not JSON-RPC 2.0")

        method = msg.get("method", "")
        params = msg.get("params", {})
        id_ = msg.get("id")

        # Notifications (no id) must not produce a response
        is_notification = id_ is None

        try:
            result = self._dispatch(method, params)
            log.debug("→ %s (%s)", method, "ok" if result else "no response")
            if is_notification:
                return None
            return make_response(id_, result)
        except McpError as e:
            log.warning("→ %s error: %s", method, e.message)
            if is_notification:
                return None
            return make_error(id_, e.code, e.message, e.data)
        except Exception as e:
            log.error("→ %s exception: %s", method, e)
            if is_notification:
                return None
            tb = traceback.format_exc()
            return make_error(id_, -32603, f"Internal error: {e}", {"traceback": tb})

    def _dispatch(self, method: str, params: dict) -> Any:
        handlers = {
            # Lifecycle
            "initialize": self._handle_initialize,
            "initialized": self._handle_initialized,
            "ping": lambda p: {},
            "notifications/initialized": lambda p: self._handle_initialized(p),
            # Tools
            "tools/list": self._handle_tools_list,
            "tools/call": self._handle_tools_call,
            # Resources
            "resources/list": self._handle_resources_list,
            "resources/read": self._handle_resources_read,
            "resources/templates/list": self._handle_resource_templates_list,
            # Prompts
            "prompts/list": self._handle_prompts_list,
            "prompts/get": self._handle_prompts_get,
        }
        handler = handlers.get(method)
        if handler is None:
            raise MethodNotFound(f"Method '{method}' not found")
        return handler(params)

    # ── Lifecycle Handlers ──

    def _handle_initialize(self, params: dict) -> dict:
        self._client_capabilities = params.get("capabilities", {})
        protocol_version = params.get("protocolVersion", MCP_VERSION)
        return {
            "protocolVersion": protocol_version,
            "capabilities": self._server_capabilities,
            "serverInfo": {
                "name": self.name,
                "version": self.version,
            },
        }

    def _handle_initialized(self, params: dict) -> dict:
        self._initialized = True
        return {}

    # ── Tools Handlers ──

    def _handle_tools_list(self, params: dict) -> dict:
        return {"tools": [t.to_dict() for t in self.tools.values()]}

    def _handle_tools_call(self, params: dict) -> dict:
        name = params.get("name", "")
        arguments = params.get("arguments", {})
        tool = self.tools.get(name)
        if tool is None:
            raise MethodNotFound(f"Tool '{name}' not found")
        try:
            result = tool.handler(**arguments)
            return {"content": [{"type": "text", "text": result}]}
        except TypeError as e:
            raise InvalidParams(str(e))

    # ── Resources Handlers ──

    def _handle_resources_list(self, params: dict) -> dict:
        return {"resources": [r.to_dict() for r in self.resources.values()]}

    def _handle_resources_read(self, params: dict) -> dict:
        uri = params.get("uri", "")
        resource = self.resources.get(uri)
        if resource is None:
            raise InvalidParams(f"Resource '{uri}' not found")
        content = resource.handler()
        return {
            "contents": [{
                "uri": uri,
                "mimeType": resource.mime_type,
                "text": content,
            }]
        }

    # ── Resource Templates Handlers ──

    def _handle_resource_templates_list(self, params: dict) -> dict:
        return {"resourceTemplates": [r.to_dict() for r in self.resource_templates.values()]}

    # ── Prompts Handlers ──

    def _handle_prompts_list(self, params: dict) -> dict:
        return {"prompts": [p.to_dict() for p in self.prompts.values()]}

    def _handle_prompts_get(self, params: dict) -> dict:
        name = params.get("name", "")
        prompt = self.prompts.get(name)
        if prompt is None:
            raise InvalidParams(f"Prompt '{name}' not found")
        args = params.get("arguments", {})
        return prompt.handler(**args)

    # ── Run ──

    def run_stdio(self):
        """Run server over stdin/stdout (one JSON-RPC message per line)."""
        log.info("MCP server v%s starting (neodos %s)", MCP_VERSION, self.version)
        log.info("%d tools, %d resources, %d templates, %d prompts",
                  len(self.tools), len(self.resources),
                  len(self.resource_templates), len(self.prompts))
        for line in sys.stdin:
            response = self.handle_line(line)
            if response is not None:
                sys.stdout.write(response + "\n")
                sys.stdout.flush()

    def run_test(self, input_lines: list[str]) -> list[str]:
        """Run server with test input, return responses. For testing."""
        outputs = []
        for line in input_lines:
            resp = self.handle_line(line)
            if resp is not None:
                outputs.append(resp)
        return outputs
