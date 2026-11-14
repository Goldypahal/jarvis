"""Minimal MCP server over stdio (standard library only), for tests.
Tools: echo(text) -> text, add(a, b) -> sum, fail() -> tool error."""
import json
import sys

TOOLS = [
    {"name": "echo", "description": "Echo text back", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}},
    {"name": "add", "description": "Add two numbers", "inputSchema": {"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}, "required": ["a", "b"]}},
    {"name": "fail", "description": "Always fails", "inputSchema": {"type": "object", "properties": {}}},
]


def reply(msg_id, result):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "result": result}) + "\n")
    sys.stdout.flush()


for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    method, msg_id, params = msg.get("method"), msg.get("id"), msg.get("params") or {}
    if msg_id is None:
        continue  # notification
    if method == "initialize":
        reply(msg_id, {"protocolVersion": params.get("protocolVersion", "2025-06-18"),
                       "capabilities": {"tools": {}}, "serverInfo": {"name": "echo", "version": "1.0"}})
    elif method == "tools/list":
        reply(msg_id, {"tools": TOOLS})
    elif method == "tools/call":
        name, args = params.get("name"), params.get("arguments") or {}
        if name == "echo":
            reply(msg_id, {"content": [{"type": "text", "text": "echo: " + str(args.get("text"))}], "isError": False})
        elif name == "add":
            reply(msg_id, {"content": [{"type": "text", "text": str(args["a"] + args["b"])}], "isError": False})
        else:
            reply(msg_id, {"content": [{"type": "text", "text": "this tool always fails"}], "isError": True})
    elif method == "ping":
        reply(msg_id, {})
    else:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "error": {"code": -32601, "message": "unknown method"}}) + "\n")
        sys.stdout.flush()
