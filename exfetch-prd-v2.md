# PRD: exfetch
**Project:** exfetch (BMC-INC/exfetch) | **Stack:** Rust binary + Chrome/Safari Extension | **Status:** Draft

## 1. Product Objective
A browser-aware web execution layer that gives any LLM agent full authenticated web access through the user's real browser sessions. Three interfaces: CLI for direct use, MCP server for Claude Code/claude.ai native integration, and a browser extension bridge for authenticated page interaction. The Rust binary is the single brain. The extension is a thin executor. Every command flows through the binary, creating a natural enforcement checkpoint for governance, logging, and policy gating.

## 2. User Stories

**Story 1: Developer using Claude Code**
**As a** developer using Claude Code in the terminal, **I want to** give Claude full web access through my real browser sessions via MCP, **so that** Claude can fetch authenticated content, interact with web apps, and search the web without flaky third-party MCP servers.
**Acceptance criteria:**
- [ ] Claude Code connects to exfetch MCP server automatically when binary is running
- [ ] `fetch_page` MCP tool returns clean extracted content from any URL
- [ ] `search_web` MCP tool returns search results with optional content fetching
- [ ] `browser_action` MCP tool can click, type, read, screenshot in authenticated browser tabs
- [ ] All commands route through the Rust binary, never directly to the extension

**Story 2: Developer using CLI**
**As a** developer in a terminal, **I want to** fetch and extract web content with a single command, **so that** I can pipe clean text into any tool without a browser dependency.
**Acceptance criteria:**
- [ ] `exfetch https://example.com` returns clean text to stdout when browser is not connected
- [ ] `exfetch search "query"` returns search results to stdout
- [ ] `exfetch https://example.com --browser` routes through extension when connected
- [ ] CLI works independently of extension and MCP server

**Story 3: claude.ai user**
**As a** claude.ai user with MCP connectors, **I want to** connect exfetch as a remote MCP server, **so that** Claude on the web can browse and interact with my authenticated sessions.
**Acceptance criteria:**
- [ ] exfetch MCP server is connectable from claude.ai MCP settings
- [ ] Same tool interface as Claude Code (fetch_page, search_web, browser_action)
- [ ] Extension relay works identically whether invoked from Claude Code or claude.ai

## 3. System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    LLM Agents                           │
│  ┌──────────┐  ┌──────────┐  ┌────────────────────┐    │
│  │Claude Code│  │claude.ai │  │Any MCP Client      │    │
│  └─────┬────┘  └─────┬────┘  └─────────┬──────────┘    │
│        │MCP          │MCP              │MCP             │
└────────┼─────────────┼─────────────────┼────────────────┘
         │             │                 │
    ┌────▼─────────────▼─────────────────▼────┐
    │          exfetch (Rust Binary)           │
    │                                         │
    │  ┌─────────┐ ┌──────┐ ┌──────────────┐  │
    │  │MCP Srvr │ │Router│ │CLI Interface  │  │
    │  └────┬────┘ └──┬───┘ └──────┬───────┘  │
    │       │         │            │           │
    │  ┌────▼─────────▼────────────▼───────┐  │
    │  │         Command Engine            │  │
    │  │  ┌──────────────────────────────┐ │  │
    │  │  │   Enforcement Checkpoint     │ │  │
    │  │  │   (policy, logging, gating)  │ │  │
    │  │  └──────────────────────────────┘ │  │
    │  └──────┬────────────────┬───────────┘  │
    │         │                │               │
    │  ┌──────▼──────┐  ┌─────▼────────────┐  │
    │  │HTTP Fetcher │  │WebSocket Server  │  │
    │  │(reqwest)    │  │(localhost:9876)   │  │
    │  │CLI fallback │  │Extension bridge  │  │
    │  └─────────────┘  └─────┬────────────┘  │
    └─────────────────────────┼───────────────┘
                              │ WebSocket
    ┌─────────────────────────▼───────────────┐
    │       Browser Extension                  │
    │  ┌──────────┐  ┌───────────────────┐    │
    │  │Chrome MV3│  │Safari Web Ext     │    │
    │  └─────┬────┘  └────────┬──────────┘    │
    │        │                │               │
    │  ┌─────▼────────────────▼────────────┐  │
    │  │  Thin Executor                    │  │
    │  │  - DOM read/extract               │  │
    │  │  - Click/type/navigate            │  │
    │  │  - Screenshot capture             │  │
    │  │  - Cookie/session access          │  │
    │  │  - Tab management                 │  │
    │  └───────────────────────────────────┘  │
    └─────────────────────────────────────────┘
```

## 4. Crate Structure

```
exfetch/
├── Cargo.toml
├── src/
│   ├── main.rs                    # Entry point: CLI + service launcher
│   │
│   ├── cli/
│   │   ├── mod.rs
│   │   └── commands.rs            # clap derive CLI interface
│   │
│   ├── mcp/
│   │   ├── mod.rs
│   │   ├── server.rs              # MCP server (stdio + SSE transport)
│   │   ├── tools.rs               # MCP tool definitions (fetch_page, search_web, browser_action)
│   │   └── types.rs               # MCP request/response types
│   │
│   ├── engine/
│   │   ├── mod.rs
│   │   ├── router.rs              # Routes commands to HTTP fetcher or extension bridge
│   │   ├── command.rs             # Canonical command types (fetch, search, interact, screenshot)
│   │   └── policy.rs              # Enforcement checkpoint: domain allowlist, action gating, logging
│   │
│   ├── fetch/
│   │   ├── mod.rs
│   │   ├── http.rs                # reqwest HTTP fetcher (CLI fallback, no browser needed)
│   │   └── pdf.rs                 # PDF text extraction
│   │
│   ├── bridge/
│   │   ├── mod.rs
│   │   ├── ws_server.rs           # WebSocket server on localhost:9876
│   │   ├── protocol.rs            # Bridge protocol types (request/response envelope)
│   │   └── connection.rs          # Extension connection manager (reconnect, health check)
│   │
│   ├── extract/
│   │   ├── mod.rs
│   │   ├── readability.rs         # Content extraction from HTML
│   │   ├── markdown.rs            # HTML to markdown
│   │   └── metadata.rs            # Title, description, OG tags
│   │
│   ├── search/
│   │   ├── mod.rs
│   │   ├── engine.rs              # DuckDuckGo scraper + SearXNG client
│   │   └── result.rs              # Search result types
│   │
│   ├── output/
│   │   ├── mod.rs
│   │   ├── text.rs                # Plain text stdout
│   │   ├── json.rs                # JSON structured output
│   │   └── markdown.rs            # Markdown output
│   │
│   └── config.rs                  # ~/.config/exfetch/config.toml
│
├── extension/
│   ├── chrome/
│   │   ├── manifest.json          # Manifest V3
│   │   ├── background.js          # Service worker: WebSocket client, command router
│   │   ├── content.js             # Content script: DOM interaction, extraction, screenshots
│   │   ├── popup.html             # Connection status, settings
│   │   ├── popup.js
│   │   └── icons/
│   │
│   └── safari/
│       ├── ExFetch/               # macOS app wrapper (Xcode project)
│       │   ├── ExFetch.xcodeproj
│       │   └── ExFetch/
│       │       ├── AppDelegate.swift
│       │       ├── ViewController.swift
│       │       └── Info.plist
│       └── Shared/                # Shared extension code (same JS as Chrome)
│           ├── manifest.json
│           ├── background.js
│           ├── content.js
│           └── popup.html
│
├── tests/
│   ├── engine_tests.rs
│   ├── fetch_tests.rs
│   ├── extract_tests.rs
│   ├── bridge_tests.rs
│   ├── mcp_tests.rs
│   └── search_tests.rs
│
└── README.md
```

## 5. Key Dependencies

```toml
[dependencies]
# CLI
clap = { version = "4", features = ["derive"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP
reqwest = { version = "0.12", features = ["rustls-tls", "cookies", "gzip", "brotli", "deflate"] }

# WebSocket server
tokio-tungstenite = "0.24"

# MCP server
# Use rmcp or hand-roll with axum SSE + stdio
axum = "0.8"
tower = "0.5"

# HTML parsing + extraction
scraper = "0.20"
html2text = "0.12"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
url = "2"
toml = "0.8"

# PDF
lopdf = "0.34"

# Error handling
thiserror = "1"    # library modules
anyhow = "1"       # main.rs, CLI

# Logging
tracing = "0.1"
tracing-subscriber = "0.3"

# Crypto (for future DetGate receipts)
sha2 = "0.10"

[features]
default = []
detgate = []       # Future: enforcement receipts, Merkle anchoring
```

## 6. Bridge Protocol

Communication between Rust binary and browser extension over WebSocket.

**Envelope format (JSON):**
```json
{
  "id": "uuid-v4",
  "type": "request|response|event",
  "command": "fetch_page|click|type|read_dom|screenshot|navigate|get_cookies|list_tabs|switch_tab",
  "params": {},
  "timestamp": "ISO8601"
}
```

**Commands the extension executes:**

| Command | Params | Returns |
|---|---|---|
| fetch_page | `{url, extract: bool, format: "text"|"markdown"|"html"}` | `{content, title, url, status_code}` |
| read_dom | `{selector: "css selector", attribute?: string}` | `{elements: [{text, html, attributes}]}` |
| click | `{selector: string}` | `{success: bool, error?: string}` |
| type_text | `{selector: string, text: string}` | `{success: bool}` |
| navigate | `{url: string}` | `{final_url, status_code}` |
| screenshot | `{selector?: string, full_page?: bool}` | `{image_base64, width, height}` |
| get_cookies | `{domain?: string}` | `{cookies: [{name, value, domain, path}]}` |
| list_tabs | `{}` | `{tabs: [{id, url, title, active}]}` |
| switch_tab | `{tab_id: number}` | `{success: bool}` |
| execute_js | `{code: string}` | `{result: any}` |

**Extension connection lifecycle:**
1. Extension starts, connects to `ws://localhost:9876`
2. Sends `{"type": "event", "command": "connected", "params": {"browser": "chrome", "version": "..."}}`
3. Binary acknowledges, marks extension as available
4. If WebSocket drops, extension retries every 5 seconds
5. Binary tracks connection state. If extension disconnected, routes fall back to HTTP fetcher where possible, or return error for browser-only commands.

## 7. MCP Server Interface

**Transport:** stdio (for Claude Code) + SSE over HTTP (for claude.ai remote MCP)

**Tools exposed:**

### fetch_page
```json
{
  "name": "fetch_page",
  "description": "Fetch and extract content from any URL. Uses authenticated browser session when available, falls back to direct HTTP.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "url": {"type": "string", "description": "URL to fetch"},
      "format": {"type": "string", "enum": ["text", "markdown", "html", "json"], "default": "markdown"},
      "use_browser": {"type": "boolean", "default": true, "description": "Route through browser extension for authenticated access"},
      "max_length": {"type": "integer", "description": "Truncate content to N characters"},
      "selector": {"type": "string", "description": "CSS selector to extract specific content"}
    },
    "required": ["url"]
  }
}
```

### search_web
```json
{
  "name": "search_web",
  "description": "Search the web and optionally fetch content from results.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {"type": "string", "description": "Search query"},
      "num_results": {"type": "integer", "default": 5},
      "fetch_results": {"type": "boolean", "default": false, "description": "Also fetch content from top results"},
      "fetch_count": {"type": "integer", "default": 3, "description": "How many results to fetch content from"},
      "format": {"type": "string", "enum": ["text", "markdown", "json"], "default": "markdown"}
    },
    "required": ["query"]
  }
}
```

### browser_action
```json
{
  "name": "browser_action",
  "description": "Interact with the user's browser. Click, type, read DOM, take screenshots, navigate. Requires browser extension connection.",
  "inputSchema": {
    "type": "object",
    "properties": {
      "action": {"type": "string", "enum": ["click", "type", "read_dom", "screenshot", "navigate", "list_tabs", "switch_tab", "get_cookies", "execute_js"]},
      "selector": {"type": "string", "description": "CSS selector for target element"},
      "text": {"type": "string", "description": "Text to type (for type action)"},
      "url": {"type": "string", "description": "URL to navigate to (for navigate action)"},
      "tab_id": {"type": "integer", "description": "Tab ID (for switch_tab action)"},
      "code": {"type": "string", "description": "JavaScript to execute (for execute_js action)"},
      "full_page": {"type": "boolean", "default": false, "description": "Full page screenshot (for screenshot action)"}
    },
    "required": ["action"]
  }
}
```

### connection_status
```json
{
  "name": "connection_status",
  "description": "Check if browser extension is connected and get available capabilities.",
  "inputSchema": {
    "type": "object",
    "properties": {}
  }
}
```

## 8. CLI Interface

```
exfetch [OPTIONS] <COMMAND>

COMMANDS:
  <URL>                     Fetch and extract content from URL
  search <QUERY>            Search the web and return results
  serve                     Start MCP server + WebSocket bridge (background service mode)
  status                    Check extension connection and service status

FETCH OPTIONS:
  --raw                     Return raw HTML
  --markdown, -m            Return as markdown
  --json, -j                Return structured JSON
  --browser, -b             Force route through browser extension
  --no-browser              Force direct HTTP fetch, skip extension
  --selector <CSS>          Extract content matching CSS selector only
  --timeout <SECS>          Request timeout [default: 15]
  --max-length <CHARS>      Truncate output to N characters
  --user-agent <STRING>     Custom user agent [default: "exfetch/0.1"]
  --quiet, -q               Suppress stderr status
  --verbose, -v             Show request details on stderr

SEARCH OPTIONS:
  --fetch, -f               Also fetch content from top results
  --results <N>             Number of results [default: 5]
  --engine <ENGINE>         Search engine: duckduckgo, searxng [default: duckduckgo]
  --searxng-url <URL>       SearXNG instance URL

SERVE OPTIONS:
  --port <PORT>             WebSocket port [default: 9876]
  --mcp-stdio               MCP over stdio (for Claude Code)
  --mcp-sse <PORT>          MCP over SSE on HTTP port (for claude.ai)
  --daemon, -d              Run as background daemon
```

## 9. Enforcement Checkpoint (engine/policy.rs)

Every command flows through the enforcement checkpoint before execution. Initially lightweight, structured for future DetGate integration.

**V1 enforcement (ship now):**
```rust
pub struct PolicyEngine {
    allowed_domains: Option<Vec<String>>,   // None = allow all
    blocked_domains: Vec<String>,           // Always blocked
    allowed_actions: HashSet<BrowserAction>, // Which browser_action commands are permitted
    log_all: bool,                          // Log every command to file
    max_requests_per_minute: u32,           // Rate limiting
}

pub enum PolicyVerdict {
    Allow,
    Block(String),     // reason
}
```

**Config in ~/.config/exfetch/config.toml:**
```toml
[policy]
blocked_domains = ["bank.com", "*.gov"]
allowed_actions = ["fetch_page", "read_dom", "screenshot", "navigate", "list_tabs"]
# execute_js and type and click require explicit opt-in
log_all = true
max_requests_per_minute = 60

[policy.detgate]
enabled = false    # Future: full DetGate receipt chain
```

**Future DetGate integration (behind feature flag):**
- Every command gets a receipt with SHA-256 hash of request + response
- Merkle tree anchoring for verifiable audit trail
- Policy DSL rules instead of simple allowlists
- Threshold-based escalation (N blocked commands triggers session pause)

## 10. Edge Cases & Failure Modes

| Scenario | Behavior |
|---|---|
| Extension not connected, browser command requested | If fetch_page: fall back to HTTP. If browser_action: return error "Browser extension not connected. Install extension and ensure exfetch service is running." |
| Extension connected but target tab navigated away | Re-query tabs, find best match by URL pattern. If no match, return error with current tab list. |
| WebSocket connection drops mid-command | Timeout after 10s. Return error. Extension auto-reconnects within 5s. |
| Multiple browser instances / profiles | Extension connects per-profile. Binary tracks multiple WebSocket connections. Route command to the connection whose tabs match the target URL. |
| MCP client sends malformed request | Return MCP error response with clear message. Never crash. |
| Rate limit hit (self-imposed) | Queue the command with estimated wait time in response. |
| Extension receives execute_js but policy blocks it | Return PolicyVerdict::Block with reason. Log the attempt. |
| PDF URL fetched via browser route | Extension fetches, binary detects Content-Type, falls back to HTTP fetch + lopdf extraction. |
| Search engine returns empty results | Try alternate engine. If both fail, return empty results with error context. |
| Binary started but port 9876 already in use | Try ports 9877-9886. Report actual port in status. Write port to ~/.config/exfetch/port file so extension can discover it. |
| Config file missing or malformed | Use defaults. Warn on stderr. Never fail to start. |
| Large page content (>1MB) | Truncate at 500K chars by default for MCP responses. CLI gets full content unless --max-length set. |

## 11. Security Considerations

- **SSRF prevention.** Block fetches to private IP ranges (10.x, 172.16-31.x, 192.168.x, 127.x, ::1) by default. --allow-private flag for explicit override.
- **WebSocket is localhost only.** Bind to 127.0.0.1, never 0.0.0.0. No remote connections to the bridge.
- **Extension origin validation.** WebSocket server validates that connecting client sends a known extension ID in the handshake.
- **No credential storage.** Cookies are read-only from browser, never written to disk by the binary.
- **execute_js is opt-in.** Blocked by default in policy. Requires explicit config to enable. When enabled, logged with full code content.
- **TLS for MCP SSE.** When exposing MCP over HTTP for claude.ai, require HTTPS or localhost only.
- **Output sanitization.** Strip null bytes, control characters from all content before returning.
- **Log file permissions.** Command log at ~/.local/share/exfetch/commands.log, chmod 600.
- **No auto-update.** Binary does not phone home or self-update. User controls updates.
- **Extension permissions are minimal.** Request only activeTab, storage, and host permissions for WebSocket localhost. No blanket <all_urls> in manifest.

## 12. Execution Steps

| # | Task | Files | Done When | Effort |
|---|---|---|---|---|
| 1 | Scaffold crate with clap CLI + serve/status subcommands | main.rs, cli/, Cargo.toml | `exfetch --help` and `exfetch serve --help` print usage | XS |
| 2 | HTTP fetcher with reqwest | fetch/http.rs | `exfetch https://httpbin.org/html` returns body | S |
| 3 | Content extraction (readability) | extract/readability.rs | Wikipedia article returns clean text, no nav/sidebar | M |
| 4 | Output formatters (text, markdown, JSON) | output/ | All three formats work with --markdown, --json flags | S |
| 5 | PDF text extraction | fetch/pdf.rs | `exfetch <pdf-url>` returns extracted text | S |
| 6 | DuckDuckGo search scraper | search/ | `exfetch search "query"` returns results | M |
| 7 | Search + fetch combo | search/mod.rs | `exfetch search "query" --fetch` returns content | S |
| 8 | Config file support | config.rs | Reads ~/.config/exfetch/config.toml, falls back to defaults | XS |
| 9 | WebSocket server | bridge/ws_server.rs, bridge/protocol.rs, bridge/connection.rs | Server starts on port 9876, accepts connections, routes messages | M |
| 10 | Command engine + router | engine/ | Commands route to HTTP or WebSocket based on connection state | M |
| 11 | Policy enforcement checkpoint | engine/policy.rs | Domain blocking, action gating, rate limiting, command logging all work | M |
| 12 | MCP server (stdio transport) | mcp/ | Claude Code connects via stdio, all 4 tools respond correctly | L |
| 13 | MCP server (SSE transport) | mcp/server.rs | claude.ai connects via HTTP SSE, same tools work | M |
| 14 | Chrome extension (Manifest V3) | extension/chrome/ | Extension connects to WebSocket, executes fetch_page and read_dom commands | L |
| 15 | Chrome extension browser actions | extension/chrome/content.js | click, type, screenshot, navigate, execute_js all work through bridge | L |
| 16 | Safari Web Extension wrapper | extension/safari/ | Xcode project builds, extension works in Safari with same JS | M |
| 17 | SSRF protection + input validation | fetch/http.rs, engine/policy.rs | Private IPs blocked, malformed URLs rejected | S |
| 18 | Edge case handling | All modules | All 12 edge cases from section 10 handled | M |
| 19 | Unit + integration tests | tests/ | Full coverage: HTTP fetch, extraction, bridge protocol, MCP tools, policy, search | L |
| 20 | README + Claude Code integration guide | README.md | Install, usage, CLAUDE.md snippet, MCP config for Claude Code + claude.ai | S |
| 21 | SearXNG search client (optional) | search/engine.rs | Works against self-hosted SearXNG instance | S |
| 22 | Daemon mode | main.rs | `exfetch serve --daemon` backgrounds the process, writes PID file | S |

## 13. Testing Requirements

**Unit tests:**
- HTTP fetcher: mock 200, 301, 403, 404, 429, 500, timeout responses
- Content extraction: known HTML in, assert article text out, junk stripped
- Markdown conversion: headers, links, code blocks preserved
- PDF extraction: small test PDF fixture
- Search result parsing: mock DuckDuckGo HTML
- Policy engine: domain blocking, action gating, rate limiting
- Bridge protocol: serialize/deserialize all command types
- SSRF: private IPs rejected, public IPs allowed
- URL normalization: bare domain gets https://
- MCP tool handlers: valid requests produce correct responses, invalid requests produce errors

**Integration tests:**
- Full CLI fetch flow: URL in, text out, exit 0
- Full search flow: query in, results out
- WebSocket handshake + command round-trip (mock extension client)
- MCP stdio session: connect, call tools, get responses
- Policy enforcement: blocked domain returns Block verdict, allowed domain returns Allow
- Router: browser connected routes to WebSocket, disconnected falls back to HTTP

**Manual QA checklist:**
- [ ] Install Chrome extension, verify WebSocket connects to running service
- [ ] Use Claude Code with MCP config, verify fetch_page returns content
- [ ] Fetch authenticated page (GitHub private repo, logged-in dashboard) via browser route
- [ ] Verify CLI fallback works when extension is not connected
- [ ] Verify blocked domain produces clear error
- [ ] Verify execute_js blocked by default, works when policy enables it
- [ ] Test Safari extension in macOS
- [ ] `exfetch serve --daemon` starts background, `exfetch status` shows running
- [ ] Large page (>1MB) truncated correctly in MCP response
- [ ] Multiple Chrome profiles: commands route to correct profile

## 14. Dependencies & Blockers

**Depends on:**
- Nothing external for core functionality
- Chrome installed for Chrome extension testing
- Xcode installed for Safari extension build
- Optional: SearXNG instance for self-hosted search

**Blocks:**
- Future DetGate integration (behind feature flag, not blocking v1)
- Future ExecLayer product packaging (after v1 validation)

**Open questions:**
- **Tab targeting strategy.** When LLM says "fetch this URL" and the URL is already open in a tab, should the extension reuse that tab or open a new one? Recommend: reuse if exact URL match, new tab if different URL. Configurable.
- **Multi-agent support.** If two Claude Code sessions connect to the same MCP server, should commands be serialized or parallelized? Recommend: serialize with queue. Defer concurrent sessions to v2.
- **Extension store publishing.** Chrome Web Store and Safari App Store review processes. Defer to after internal validation. Distribute as unpacked extension initially.

## 15. Definition of Done
- [ ] All acceptance criteria from section 2 pass
- [ ] All 22 execution steps completed
- [ ] All unit and integration tests pass
- [ ] All 12 edge cases handled
- [ ] Policy enforcement verified (domain blocking, action gating, logging)
- [ ] SSRF protection verified
- [ ] Chrome extension connects and executes commands through bridge
- [ ] MCP server works via stdio (Claude Code) and SSE (claude.ai)
- [ ] CLI fallback works without extension
- [ ] Binary compiles on macOS (aarch64) and Linux (x86_64)
- [ ] `cargo clippy` clean, `cargo fmt` clean
- [ ] README includes install, usage, MCP config, and CLAUDE.md integration
- [ ] Published to BMC-INC/exfetch on GitHub
- [ ] Safari extension builds in Xcode (stretch goal for v1)
