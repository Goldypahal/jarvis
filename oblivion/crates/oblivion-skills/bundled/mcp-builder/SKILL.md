---
name: mcp-builder
description: Build a new MCP (Model Context Protocol) server so Oblivion, Claude or other agents can use an API, database or app as tools - in Python (FastMCP) or TypeScript - and connect it to Oblivion. Use when the user wants to connect an app or API that has no MCP server yet, or asks to build/make an MCP server or tool integration.
license: MIT
---
# Build an MCP server

1. **Design the tools first.** List the 3-10 actions an agent needs (e.g. `search_issues`, `get_issue`,
   `create_issue`), not one tool per API endpoint. Each tool: a verb-noun name, a one-sentence description
   saying when to use it, and few, clearly named arguments. Prefer returning short, readable text or
   compact JSON with only useful fields; support `limit`/paging for lists. Mark which tools change data.
2. **Check the API**: base URL, authentication (API key, OAuth token), rate limits, the endpoints behind
   each tool. Read the official docs with web_fetch. Secrets come from environment variables, never code.
3. **Python (simplest)** - `pip install "mcp[cli]"`, then `server.py`:
   ```python
   import os, httpx
   from mcp.server.fastmcp import FastMCP

   mcp = FastMCP("weather")
   KEY = os.environ.get("WEATHER_API_KEY", "")

   @mcp.tool()
   def current_weather(city: str) -> str:
       """Current temperature and conditions for a city."""
       r = httpx.get("https://api.example.com/weather", params={"q": city, "key": KEY}, timeout=20)
       r.raise_for_status()
       d = r.json()
       return f"{city}: {d['temp_c']} °C, {d['condition']}"

   if __name__ == "__main__":
       mcp.run()  # stdio
   ```
   **TypeScript** - `npm i @modelcontextprotocol/sdk zod`, create an `McpServer`, register tools with
   `server.registerTool(name, {description, inputSchema}, handler)`, and connect a `StdioServerTransport`.
4. **Errors**: catch API failures and return a clear message (what failed, what to try), not a stack trace.
   Never print to stdout except through the MCP library (stdout is the protocol channel); log to stderr.
5. **Connect it to Oblivion** in `~/.oblivion/oblivion.toml`:
   ```toml
   [[mcp_servers]]
   name = "weather"
   description = "Current weather for any city"
   command = "python"
   args = ["C:\\path\\to\\server.py"]
   env = { WEATHER_API_KEY = "..." }   # or set it in the system environment instead
   ```
6. **Test**: `oblivion --mcp` lists the server's tools (fix any startup error it shows), then ask Oblivion
   to use one tool. Also test a failure (bad key, unknown city) and check the message is helpful.
7. Write a short README: what it does, required environment variables, the config snippet above.
