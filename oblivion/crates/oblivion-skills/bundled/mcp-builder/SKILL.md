---
name: mcp-builder
description: Build a new MCP (Model Context Protocol) server so Oblivion, Claude or other agents can use an API, database or app as tools - in Python (FastMCP) or TypeScript - and connect it to Oblivion. Use when the user wants to connect an app or API that has no MCP server yet, or asks to build/make an MCP server or tool integration.
license: MIT
---
# Build an MCP server

1. **Design the tools first.** List the 3-10 actions an agent needs (e.g. `search_issues`, `get_issue`,
   `create_issue`), not one raw tool per API endpoint. Each tool:
   - A verb-noun name (`get_weather`, `list_tasks`).
   - A concise docstring explaining what it does and when to call it.
   - Fully type-annotated arguments with descriptive names.
   - Return concise text or compact JSON containing only relevant fields; support `limit`/pagination parameters for list tools.
   - Clearly delineate read-only tools from tools that modify external state.
2. **Check the target API**: base URL, authentication scheme (API keys, Bearer tokens), rate limits, and endpoint schemas.
   Inspect official documentation via `web_fetch`. Credentials must always be loaded from environment variables, never hardcoded.
3. **Implementation**:
   - **Python (FastMCP)** - `pip install "mcp[cli]"`:
     ```python
     import os
     import sys
     import httpx
     from mcp.server.fastmcp import FastMCP

     mcp = FastMCP("weather")
     KEY = os.environ.get("WEATHER_API_KEY", "")

     @mcp.tool()
     def current_weather(city: str) -> str:
         """Retrieve current temperature and weather conditions for a specified city."""
         if not KEY:
             return "Error: WEATHER_API_KEY environment variable is not configured."
         r = httpx.get("https://api.example.com/weather", params={"q": city, "key": KEY}, timeout=20)
         r.raise_for_status()
         d = r.json()
         return f"{city}: {d['temp_c']} °C, {d['condition']}"

     if __name__ == "__main__":
         mcp.run(transport="stdio")
     ```
   - **TypeScript**: `npm i @modelcontextprotocol/sdk zod`, create an `McpServer`, register tools using
     `server.registerTool(name, {description, inputSchema}, handler)`, and connect via `StdioServerTransport`.
4. **Stdout hygiene & error handling:**
   - **CRITICAL**: The MCP stdio transport uses `stdout` strictly for JSON-RPC framing. Never write print statements or raw logs to `stdout` (`print()`, `sys.stdout.write()`). Any non-JSON text on stdout corrupts the protocol stream and crashes the client.
   - Always route diagnostics and logs to `sys.stderr` (`print(..., file=sys.stderr)` or standard `logging`).
   - Catch upstream exceptions and return human-readable error descriptions rather than dumping tracebacks into tool output.
5. **Connect it to Oblivion** in config (`~/.oblivion/oblivion.toml`):
   ```toml
   [[mcp_servers]]
   name = "weather"
   description = "Current weather for any city"
   command = "python"
   args = ["C:\\path\\to\\server.py"]
   env = { WEATHER_API_KEY = "..." }   # or inherit from system environment
   ```
6. **Test**:
   - Run `oblivion --mcp` to verify the client connects, completes handshake, and lists all tool schemas without errors.
   - Test execution of at least one read tool and verify failure handling (e.g. invalid arguments or missing API key).
7. Write a concise README: summary of capabilities, required environment variables, setup instructions, and the Oblivion configuration snippet.
