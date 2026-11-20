"""Fake Playwright MCP server over stdio (standard library only), for tests.
Speaks the newer argument style (`target` for element refs) and replies in
Playwright MCP's response layout. Every call is logged to stderr-free memory and
echoed back so tests can check the arguments that arrived."""
import json
import sys

PAGE = """### Ran Playwright code
```js
await page.goto('{url}');
```

### Page state
- Page URL: {url}
- Page Title: Practice Quiz
- Page Snapshot:
```yaml
- generic [active] [ref=e1]:
  - heading "Practice Quiz" [level=1] [ref=e3]
  - generic [ref=e4]:
    - paragraph [ref=e7]: "Question 1: What is 2 + 3?"
    - radio "4" [ref=e9]
    - radio "5" [ref=e10]
    - button "Submit quiz" [ref=e20] [cursor=pointer]
```
### Last call
{call}"""

PICTURE_PAGE = """### Ran Playwright code
```js
await page.goto('{url}');
```

### Page state
- Page URL: {url}
- Page Title: Visual Geometry Quiz
- Page Snapshot:
```yaml
- generic [ref=e1]:
  - img "geometry_diagram_q2.png" [ref=e2]
  - button "Answer A" [ref=e3]
  - button "Answer B" [ref=e4]
```
### Last call
{call}"""

def tool(name, props, req=()):
    return {"name": name, "description": name, "inputSchema": {"type": "object", "properties": {p: {"type": "string"} for p in props}, "required": list(req)}}

TOOLS = [
    tool("browser_navigate", ["url"], ["url"]),
    tool("browser_snapshot", []),
    tool("browser_take_screenshot", []),
    tool("browser_click", ["element", "target"], ["target"]),
    tool("browser_type", ["element", "target", "text", "submit"], ["target", "text"]),
    tool("browser_select_option", ["element", "target", "values"], ["target", "values"]),
    tool("browser_press_key", ["key"], ["key"]),
]
url = "about:blank"

def reply(msg_id, result):
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "result": result}) + "\n")
    sys.stdout.flush()

for line in sys.stdin:
    if not line.strip():
        continue
    msg = json.loads(line)
    method, msg_id, params = msg.get("method"), msg.get("id"), msg.get("params") or {}
    if msg_id is None:
        continue
    if method == "initialize":
        reply(msg_id, {"protocolVersion": params.get("protocolVersion", "2025-06-18"),
                       "capabilities": {"tools": {}}, "serverInfo": {"name": "fake-playwright", "version": "1.0"}})
    elif method == "tools/list":
        reply(msg_id, {"tools": TOOLS})
    elif method == "tools/call":
        name, args = params.get("name"), params.get("arguments") or {}
        if name == "browser_navigate":
            url = args["url"]
        if name == "browser_take_screenshot":
            reply(msg_id, {
                "content": [
                    {
                        "type": "image",
                        "data": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
                        "mimeType": "image/png"
                    }
                ],
                "isError": False
            })
            continue
        if name == "browser_click" and args.get("target") == "e99":
            reply(msg_id, {"content": [{"type": "text", "text": "Ref e99 not found in the current page snapshot."}], "isError": True})
            continue
        call = json.dumps({"tool": name, "args": args}, sort_keys=True)
        template = PICTURE_PAGE if "picture" in url else PAGE
        reply(msg_id, {"content": [{"type": "text", "text": template.format(url=url, call=call)}], "isError": False})
    elif method == "ping":
        reply(msg_id, {})
    else:
        sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": msg_id, "error": {"code": -32601, "message": "unknown method"}}) + "\n")
        sys.stdout.flush()
