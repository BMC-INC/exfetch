# exfetch

Browser-aware web execution layer for LLM agents. Combines direct HTTP fetching with a Chrome extension bridge to give AI assistants authenticated, JavaScript-rendered web access.

## Architecture

```
                          +------------------+
                          |   Claude / LLM   |
                          +--------+---------+
                                   |
                    JSON-RPC (stdio or SSE)
                                   |
                          +--------v---------+
                          |   exfetch CLI    |
                          |  (Rust binary)   |
                          +--+-----+------+--+
                             |     |      |
               +-------------+     |      +--------------+
               |                   |                     |
       +-------v-------+  +-------v-------+  +----------v---------+
       |  HTTP Fetcher  |  | Search Engine |  | WebSocket Bridge   |
       | (reqwest+TLS)  |  | (DDG/SearXNG) |  | (tokio-tungstenite)|
       +-------+--------+  +---------------+  +----------+---------+
               |                                         |
       +-------v--------+                      +---------v---------+
       | Content Extract |                      | Chrome Extension  |
       | readability/md  |                      | (Manifest V3)     |
       +----------------+                      +-------------------+
```

## Install

```bash
cargo install --path .
```

## CLI Usage

### Fetch a page

```bash
exfetch fetch https://example.com
exfetch fetch https://example.com --markdown
exfetch fetch https://example.com --json
exfetch fetch https://example.com --raw
exfetch fetch https://example.com --max-length 5000
```

### Search the web

```bash
exfetch search "rust programming language"
exfetch search "rust programming" --results 10
exfetch search "rust programming" --fetch          # also fetch top results
exfetch search "rust programming" --json
exfetch search "query" --engine searxng --searxng-url https://searx.be
```

### Start the server

```bash
exfetch serve                          # WebSocket server for browser extension
exfetch serve --mcp-stdio              # MCP server over stdin/stdout
exfetch serve --mcp-sse 8080           # MCP server over SSE
exfetch serve --daemon                 # Fork to background
```

### Check status

```bash
exfetch status
```

## MCP Setup for Claude Code

Add to your Claude Code MCP configuration:

```json
{
  "mcpServers": {
    "exfetch": {
      "command": "exfetch",
      "args": ["serve", "--mcp-stdio"]
    }
  }
}
```

## MCP Setup for claude.ai

For claude.ai, you need a tunnel (cloudflared or ngrok) to expose the SSE endpoint:

```bash
# Terminal 1: start exfetch with SSE
exfetch serve --mcp-sse 8080

# Terminal 2: expose via tunnel
cloudflared tunnel --url http://localhost:8080
# or
ngrok http 8080
```

Then configure the tunnel URL as a remote MCP server in claude.ai settings.

## Chrome Extension

1. Open `chrome://extensions/` in Chrome
2. Enable "Developer mode"
3. Click "Load unpacked"
4. Select the `extension/chrome/` directory
5. Start the server with `exfetch serve`
6. The extension will auto-connect via WebSocket

## Config File

exfetch reads configuration from `~/.config/exfetch/config.toml`:

```toml
[defaults]
timeout = 15
user_agent = "exfetch/0.1"
search_engine = "duckduckgo"   # or "searxng"
# searxng_url = "https://searx.be"

[policy]
blocked_domains = []
allow_cookie_read = false
max_requests_per_minute = 60

[tabs]
reuse_strategy = "exact"
open_in_background = true
```

## CLAUDE.md Integration

Add to your project's `CLAUDE.md`:

```
Use `exfetch <url>` to fetch web content. Use `exfetch search <query>` to search.
```

## License

MIT
