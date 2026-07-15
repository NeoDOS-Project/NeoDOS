"""
Basic tests for the NeoDOS MCP server: engine dispatch, tool listing, syscall parsing.
"""

import json
import sys
import os
from pathlib import Path

# Ensure the parent is on the path
sys.path.insert(0, str(Path(__file__).parent.parent))

from server import McpServer, ToolSpec, make_request, make_response, make_error
from tools.lsp_tools import _parse_syscall_nums


def make_jsonrpc(method: str, params: dict = None, id_: int = 1) -> str:
    return json.dumps({
        "jsonrpc": "2.0",
        "method": method,
        "params": params or {},
        "id": id_,
    })


def test_ping():
    server = McpServer()
    resp = server.handle_line(make_jsonrpc("ping"))
    parsed = json.loads(resp)
    assert parsed["jsonrpc"] == "2.0"
    assert parsed["result"] == {}
    print("  ✓ ping works")


def test_initialize():
    server = McpServer()
    resp = server.handle_line(make_jsonrpc("initialize", {
        "protocolVersion": "1.0",
        "capabilities": {},
    }))
    parsed = json.loads(resp)
    assert parsed["result"]["protocolVersion"] == "1.0"
    assert "tools" in parsed["result"]["capabilities"]
    assert "resources" in parsed["result"]["capabilities"]
    print("  ✓ initialize works")


def test_tools_list():
    server = McpServer()
    server.register_tool(ToolSpec("test_tool", "A test", {"type": "object", "properties": {}}, lambda: "ok"))
    resp = server.handle_line(make_jsonrpc("tools/list"))
    parsed = json.loads(resp)
    tools = parsed["result"]["tools"]
    assert len(tools) == 1
    assert tools[0]["name"] == "test_tool"
    print("  ✓ tools/list works")


def test_tools_call():
    server = McpServer()
    server.register_tool(ToolSpec("hello", "Say hello", {"type": "object", "properties": {}}, lambda: "Hello!"))
    resp = server.handle_line(make_jsonrpc("tools/call", {"name": "hello", "arguments": {}}))
    parsed = json.loads(resp)
    assert parsed["result"]["content"][0]["text"] == "Hello!"
    print("  ✓ tools/call works")


def test_tools_call_unknown():
    server = McpServer()
    resp = server.handle_line(make_jsonrpc("tools/call", {"name": "nonexistent", "arguments": {}}))
    parsed = json.loads(resp)
    assert "error" in parsed
    assert parsed["error"]["code"] == -32601
    print("  ✓ tools/call unknown tool returns error")


def test_invalid_json():
    server = McpServer()
    resp = server.handle_line("not json")
    parsed = json.loads(resp)
    assert "error" in parsed
    assert parsed["error"]["code"] == -32700
    print("  ✓ invalid JSON returns parse error")


def test_notification_no_response():
    server = McpServer()
    resp = server.handle_line(make_jsonrpc("initialized", {}, id_=None))
    assert resp is None
    print("  ✓ notification produces no response")


def test_syscall_parsing():
    root = os.environ.get("NEODOS_ROOT", str(Path(__file__).parent.parent.parent.parent))
    syscalls = _parse_syscall_nums(root)
    assert len(syscalls) > 0
    # Check known syscalls exist
    assert "exit" in syscalls
    assert syscalls["exit"] == 0
    assert "write" in syscalls
    assert "read" in syscalls
    print(f"  ✓ syscall parsing: {len(syscalls)} entries, includes exit(0), write({syscalls.get('write')})")


def test_resource_templates():
    server = McpServer()
    from server import ResourceSpec
    server.register_resource_template(
        "neodos://vfs/{drive}/{path}",
        ResourceSpec("neodos://vfs/{drive}/{path}", "VFS", "desc", "text/plain", lambda: "ok"),
    )
    resp = server.handle_line(make_jsonrpc("resources/templates/list"))
    parsed = json.loads(resp)
    assert len(parsed["result"]["resourceTemplates"]) == 1
    assert parsed["result"]["resourceTemplates"][0]["uri"] == "neodos://vfs/{drive}/{path}"
    print("  ✓ resource templates work")


def test_run_test():
    server = McpServer()
    server.register_tool(ToolSpec("echo", "Echo", {"type": "object", "properties": {}}, lambda: "echo!"))
    inputs = [
        make_jsonrpc("initialize", {"protocolVersion": "1.0", "capabilities": {}}),
        make_jsonrpc("tools/list"),
    ]
    outputs = server.run_test(inputs)
    assert len(outputs) == 2
    parsed = json.loads(outputs[1])
    assert "tools" in parsed["result"]
    print("  ✓ run_test mode works")


if __name__ == "__main__":
    print("MCP Server Tests:")
    test_ping()
    test_initialize()
    test_tools_list()
    test_tools_call()
    test_tools_call_unknown()
    test_invalid_json()
    test_notification_no_response()
    test_syscall_parsing()
    test_resource_templates()
    test_run_test()
    print("\nAll tests passed!")
