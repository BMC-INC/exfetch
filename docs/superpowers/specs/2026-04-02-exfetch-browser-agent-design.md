# exfetch — Browser Agent Layer Design Spec
**Date:** 2026-04-02 | **Status:** Approved | **Source PRD:** exfetch-prd-v2.md

## Overview

exfetch is a browser-aware web execution layer that gives any LLM agent full authenticated web access through the user's real browser sessions. It replaces Anthropic's unreliable built-in WebFetch/WebSearch tools with a self-hosted solution the user controls end-to-end.

Three interfaces: CLI for direct use, MCP server for Claude Code/claude.ai native integration, and a browser extension bridge for authenticated page interaction.

**Core principle:** The Rust binary is the single brain. The browser extension is a thin executor. Every command flows through the binary, creating a natural enforcement checkpoint for governance, logging, and policy gating.

## Architecture

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
    │  │  Thin Executor (on-demand inject) │  │
    │  │  - chrome.scripting.executeScript  │  │
    │  │  - DOM read/extract               │  │
    │  │  - Click/type/navigate            │  │
    │  │  - Screenshot capture             │  │
    │  │  - Cookie/session access          │  │
    │  │  - Tab management                 │  │
    │  └───────────────────────────────────┘  │
    └─────────────────────────────────────────┘
```

## Design Decisions (divergences from V2 PRD)

### 1. Extension permissions: `<all_urls>` required for autonomous mode

The V2 PRD specified `activeTab` only. This is insufficient — `activeTab` requires a physical user click on the extension icon to grant temporary access. Since the entire point is autonomous LLM-driven browsing, the extension manifest **must** declare `<all_urls>` host permission (or `*://*/*`).

**Sign-off:** PRD author explicitly approved this override: "yes whatever is needed to make sure autonomous mode is working." The PRD's `activeTab`-only constraint was Sonnet being cautious; the user overrides for full autonomy.

Security is enforced at the **policy engine** layer in the Rust binary, not at the browser permission level. The extension trusts authenticated commands from the binary (see WebSocket Authentication below). The binary enforces domain blocking, action gating, and rate limiting before any command reaches the extension.

**Chrome manifest.json permissions:**
```json
{
  "permissions": ["scripting", "tabs", "cookies", "storage", "activeTab"],
  "host_permissions": ["*://*/*"],
  "optional_host_permissions": []
}
```

**Safari:** Equivalent entitlements in the Xcode project. Safari Web Extensions support the same `host_permissions` model.

### 2. On-demand script injection, not persistent content scripts

The V2 PRD lists `content.js` as a content script injected into every page. This is wasteful and fragile (CSP conflicts, memory overhead on every tab).

Instead: the background service worker uses `chrome.scripting.executeScript()` to inject interaction code **only when a command targets a specific tab**. This:
- Avoids CSP conflicts on restrictive sites
- Zero overhead on tabs Claude isn't interacting with
- Each injection is a fresh, isolated execution context
- Works on pages that were loaded before the extension was installed

The extension ships a library of injectable functions (click, type, read DOM, screenshot, etc.) that the background worker injects on demand.

### 3. claude.ai MCP connectivity requires a tunnel

The MCP SSE transport binds to localhost. claude.ai runs in Anthropic's cloud and cannot reach localhost directly. Users connecting claude.ai must use one of:
- **Cloudflare Tunnel** (recommended): `cloudflared tunnel --url http://localhost:9877`
- **ngrok**: `ngrok http 9877`
- **Anthropic Remote MCP** (if/when available)

The README must document this as a required setup step for claude.ai users. Claude Code users (stdio transport) are unaffected.

## Components

### Rust Binary (`exfetch`)

**CLI module** (`cli/`): clap derive interface. Subcommands: fetch URL, search, serve, status. All flags from PRD section 8.

**MCP module** (`mcp/`): Dual transport MCP server.
- stdio transport: launched by Claude Code directly (`exfetch serve --mcp-stdio`)
- SSE transport: HTTP server on configurable port (`exfetch serve --mcp-sse 9877`)
- Four tools: `fetch_page`, `search_web`, `browser_action`, `connection_status`

**Engine module** (`engine/`): Command routing and policy enforcement.
- `router.rs`: If extension connected and command is browser-capable → WebSocket. Otherwise → HTTP fetcher. `--browser` flag forces extension route (error if not connected). `--no-browser` forces HTTP.
- `policy.rs`: Every command passes through policy check before execution. Domain blocking, action gating, rate limiting, command logging.
- `command.rs`: Canonical command types shared across CLI, MCP, and bridge.

**Bridge module** (`bridge/`): WebSocket server on localhost:9876.
- JSON envelope protocol (id, type, command, params, timestamp)
- Connection manager with health checks and reconnect tracking
- Port fallback (9876-9886) with port file discovery

**Fetch module** (`fetch/`): Direct HTTP fetching via reqwest (CLI fallback). PDF extraction via lopdf.

**Extract module** (`extract/`): Readability-style content extraction, HTML→markdown, metadata extraction.

**Search module** (`search/`): DuckDuckGo HTML scraper (default), SearXNG client (optional).

**Output module** (`output/`): Text, markdown, JSON formatters for stdout.

**Config** (`config.rs`): `~/.config/exfetch/config.toml` for defaults, policy rules, engine preferences.

### Browser Extension

**Shared code** (used by both Chrome and Safari):

- `background.js` — Service worker. WebSocket client connecting to `ws://localhost:9876`. Receives commands, dispatches to appropriate handler, returns results. Manages tab targeting. Uses `chrome.scripting.executeScript()` for all DOM interactions.

- `injectors/` — Library of injectable functions:
  - `read_dom.js` — Extract content, query selectors, get attributes
  - `interact.js` — Click elements, type text, scroll, wait for elements
  - `capture.js` — Triggers `chrome.tabs.captureVisibleTab()` from background worker for viewport screenshots. Full-page screenshots use scroll-and-stitch: scroll viewport, capture, repeat, composite. Element screenshots use selector bounding rect to crop.
  - `extract.js` — Readability-style extraction running in-page (for authenticated content the binary can't fetch directly)

- `popup.html/popup.js` — Status UI. Shows connection state, recent commands, quick settings.

**Chrome** (`extension/chrome/`): Manifest V3. `manifest.json` with `<all_urls>` host permissions, `scripting`, `tabs`, `cookies` permissions.

**Safari** (`extension/safari/`): macOS app wrapper via Xcode. Shared JS from Chrome build. Safari Web Extension API is compatible with Chrome MV3 for the APIs we need (scripting, tabs, cookies, storage). Stretch goal for v1.

### Bridge Protocol

JSON over WebSocket. Same as PRD section 6:

| Command | Params | Returns |
|---|---|---|
| fetch_page | `{url, extract, format}` | `{content, title, url, status_code}` |
| read_dom | `{selector, attribute?}` | `{elements: [{text, html, attributes}]}` |
| click | `{selector}` | `{success, error?}` |
| type_text | `{selector, text}` | `{success}` |
| navigate | `{url}` | `{final_url, status_code}` |
| screenshot | `{selector?, full_page?}` | `{image_base64, width, height}` |
| get_cookies | `{domain?}` | `{cookies: [...]}` |
| list_tabs | `{}` | `{tabs: [...]}` |
| switch_tab | `{tab_id}` | `{success}` |
| execute_js | `{code}` | `{result}` |

### Policy Engine (V1)

```
allowed_domains: Option<Vec<String>>     # None = allow all
blocked_domains: Vec<String>             # Always blocked
allowed_actions: HashSet<BrowserAction>  # Permitted browser_action commands
log_all: bool                            # Log every command
max_requests_per_minute: u32             # Rate limiting
```

Default policy: all domains allowed, `execute_js` blocked (opt-in), all other actions allowed, logging enabled, 60 req/min limit. When `execute_js` is enabled via policy, all invocations (both allowed and blocked) are logged with full code content to the command log.

Future: DetGate integration behind `--features detgate` flag.

## WebSocket Authentication

Extension ID alone is insufficient — any local process can spoof it. The handshake uses a **per-session shared token:**

1. On `exfetch serve` startup, binary generates a random 256-bit token.
2. Token is written to `~/.config/exfetch/session_token` with `chmod 600`.
3. The extension reads this token via a one-time `fetch("http://localhost:9876/.well-known/exfetch-token")` endpoint that serves the token once, then disables itself.
4. Extension includes the token in the WebSocket upgrade request as `Sec-WebSocket-Protocol: exfetch-v1-{token}`.
5. Binary validates the token on every WebSocket connection. Rejects without valid token.
6. Token rotates on every binary restart.

**Race condition mitigation:** A local attacker could theoretically probe the token endpoint before the extension. To narrow this window: the token endpoint only responds to requests that include the extension's `Origin` header (`chrome-extension://<id>`). The binary validates this header before serving the token. This is spoofable by a determined local attacker, but raises the bar beyond casual exploitation. For higher security, a future version can switch to filesystem-based token exchange via Native Messaging.

This prevents other local processes from impersonating the extension. The token endpoint is localhost-only and single-use.

## Cookie Access Policy

`get_cookies` is **disabled by default** in the policy engine. Users must explicitly opt in:

```toml
[policy]
allow_cookie_read = false   # Default: disabled
cookie_allowed_domains = [] # Allowlist when enabled: ["github.com", "myapp.com"]
```

When enabled, cookie values are redacted in command logs (only domain/name/path are logged, not values). MCP responses containing cookies include a warning: `"⚠ Cookie data included. This response may be logged by your MCP client."`

## Port Discovery

The binary writes the actual listening port to `~/.config/exfetch/port` (plain text, e.g., "9876"). The extension cannot read the filesystem, so it uses a sequential probe strategy:

1. Try `ws://localhost:9876` through `ws://localhost:9886`
2. On each attempt, send the auth token in the upgrade header
3. First successful authenticated handshake wins
4. Cache the working port in `chrome.storage.local`
5. On reconnect failure, clear cache and re-probe

## MV3 Service Worker Keepalive

Chrome MV3 service workers are terminated after ~30 seconds of inactivity. The WebSocket connection keeps the worker alive, but Chrome's behavior is inconsistent. Strategy:

1. **Primary:** The WebSocket connection itself acts as keepalive — Chrome extends the lifetime to 5 minutes for active connections.
2. **Heartbeat:** Binary sends a `ping` frame every 25 seconds. Extension responds with `pong`. This ensures the connection is never "idle."
3. **Recovery:** If Chrome kills the worker despite the WebSocket, the `onConnect` event from a new tab navigation restarts it. The worker re-establishes the WebSocket connection within 5 seconds. The binary queues any in-flight commands and replays them after reconnection.
4. **chrome.alarms API:** Register a 25-second recurring alarm as a fallback wakeup mechanism.

## Tab Targeting (Resolved)

When a command includes a URL:
1. Query all tabs for an exact URL match. If found, reuse that tab.
2. If no exact match, query for same-origin match. If found and the command is `navigate`, reuse that tab.
3. Otherwise, open a new tab in the background.
4. Commands without a URL (click, type, read_dom, screenshot) target the most recently used tab unless `tab_id` is specified.

Configurable in `config.toml`:
```toml
[tabs]
reuse_strategy = "exact"  # "exact", "origin", or "never"
open_in_background = true
```

## Multi-Profile Routing

Each extension instance (one per browser profile) includes its profile name in the WebSocket handshake metadata:
```json
{"type": "event", "command": "connected", "params": {"browser": "chrome", "profile": "Default", "version": "..."}}
```

The binary tags each WebSocket connection with its profile. When routing a command:
1. If the command specifies a `tab_id`, route to the connection that owns that tab.
2. If the command specifies a URL, query all connections for a tab matching that URL.
3. If no match, route to the most recently active connection.
4. If multiple connections, the user can set a default profile in config.toml.

## Restricted Pages

`chrome.scripting.executeScript()` cannot inject into:
- `chrome://` pages, `chrome-extension://` pages, Chrome Web Store
- `about:`, `data:`, `blob:` URLs
- Safari equivalents: `safari-extension://`, `safari-web-extension://`

When a command targets a restricted page, the extension returns a specific error: `{"error": "restricted_page", "message": "Cannot interact with browser internal pages (chrome://...)"}`. The binary passes this through to the caller with a clear message.

## SSRF Protection

Applies to the HTTP fetcher (reqwest) path only — the browser extension route uses the browser's own DNS and network stack, which is not controllable.

**Blocked ranges (HTTP fetcher):**
- `127.0.0.0/8`, `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- `::1`, `fe80::/10`, `fc00::/7`
- `169.254.0.0/16` (link-local)
- `0.0.0.0`

**DNS rebinding protection:** Resolve the hostname, check the IP against blocked ranges, then fetch using the resolved IP (not the hostname). This prevents domains that resolve to private IPs after initial resolution.

**Browser route limitation:** Acknowledged — the browser will fetch any URL the user could navigate to, including internal network resources. This is by design (the user is already authenticated and can access these resources). The policy engine's `blocked_domains` list is the control layer for the browser route.

## Edge Cases

All 12 edge cases from PRD Section 10, plus additions:

| # | Scenario | Behavior |
|---|---|---|
| 1 | Extension not connected, browser command requested | fetch_page: fall back to HTTP. browser_action: return error "Browser extension not connected." |
| 2 | Extension connected but target tab navigated away | Re-query tabs, find best match by URL. If no match, return error with current tab list. |
| 3 | WebSocket drops mid-command | Timeout after 10s. Return error. Extension auto-reconnects within 5s. |
| 4 | Multiple browser profiles | Route to connection whose tabs match target URL. See Multi-Profile Routing. |
| 5 | MCP client sends malformed request | Return MCP error response. Never crash. |
| 6 | Rate limit hit | Queue command, return estimated wait time in response. |
| 7 | Policy blocks execute_js | Return PolicyVerdict::Block with reason. Log the attempt with full code content. |
| 8 | PDF URL via browser route | Extension fetches, binary detects Content-Type, falls back to HTTP + lopdf. |
| 9 | Search engine returns empty | Try alternate engine. Both fail → return empty with error context. |
| 10 | Port 9876 in use | Try 9877-9886. Write actual port to `~/.config/exfetch/port`. |
| 11 | Config file missing/malformed | Use defaults. Warn on stderr. Never fail to start. |
| 12 | Large page (>1MB) | MCP responses truncated at 500K chars. CLI gets full content unless --max-length. |
| 13 | Command targets restricted page (chrome://) | Return `restricted_page` error with clear message. |
| 14 | Service worker killed by Chrome | Alarm-based wakeup, re-establish WebSocket, replay queued commands. |
| 15 | Simultaneous commands from multiple MCP clients | Serialize with FIFO queue. One command at a time per tab. |

## CLI Design Invariant

The CLI works independently of the extension and the MCP server. `exfetch <url>` and `exfetch search <query>` use the HTTP fetcher directly. `--browser` flag opts into extension routing. If the service is not running, CLI commands that don't require `--browser` succeed normally.

## MCP Auto-Discovery (Claude Code)

Claude Code connects to the MCP server via a config entry in `~/.claude/settings.json` or the project's `.mcp.json`:
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
Claude Code launches the binary on demand. "Automatically" means: once the config entry exists, Claude Code starts the MCP server on first tool use without user intervention. The user adds the config entry once during setup.

## MCP SSE Transport Note

The MCP SSE transport is bidirectional: server-to-client via SSE stream, client-to-server via HTTP POST to a companion endpoint. This follows the MCP SSE transport specification. The binary runs both on the same HTTP port (e.g., 9877): `GET /sse` for the event stream, `POST /message` for client requests.

## Crate Structure and Dependencies

See PRD Sections 4 and 5 for the canonical directory layout and dependency list. The spec's `injectors/` directory replaces the PRD's `content.js` files in both `extension/chrome/` and `extension/safari/Shared/`.

## Testing Requirements

See PRD Section 13 for full unit test, integration test, and manual QA specifications. Additional tests for spec-specific changes:
- WebSocket auth: valid token accepted, invalid token rejected, token rotation on restart
- Cookie policy: get_cookies blocked by default, allowed with opt-in, values redacted in logs
- Port discovery: binary falls back to alternate ports, writes port file
- Service worker recovery: simulated worker kill, command replay after reconnect
- Tab targeting: exact match reuse, origin match, new tab creation
- Multi-profile routing: correct connection selected based on tab ownership

## Execution Sequence

22 steps from PRD section 12, grouped into phases:

**Phase 1 — CLI Core (steps 1-8):** Scaffold, HTTP fetch, extraction, formatters, PDF, search, config. This produces a working CLI tool identical to V1 PRD functionality.

**Phase 2 — Bridge + Engine (steps 9-11):** WebSocket server, command router, policy engine. The binary can now receive and route commands.

**Phase 3 — MCP Server (steps 12-13):** stdio and SSE transports. Claude Code and claude.ai can connect.

**Phase 4 — Chrome Extension (steps 14-15):** Extension connects to bridge, executes all browser commands. This is the critical integration milestone.

**Phase 5 — Hardening (steps 16-19):** Safari extension, SSRF protection, edge cases, tests.

**Phase 6 — Ship (steps 20-22):** README, daemon mode, SearXNG.

## Testing Strategy

Unit tests for every module. Integration tests for full flows (CLI fetch, MCP session, WebSocket round-trip, policy enforcement). Mock extension client for bridge tests.

Manual QA: Chrome extension end-to-end, authenticated page fetch, Claude Code MCP integration, Safari extension, daemon mode.

Full details in PRD sections 13 and 15.

## Resolved Questions (from PRD)

1. **Tab targeting:** Resolved. Exact URL → reuse, same origin + navigate → reuse, otherwise new tab. Configurable via `config.toml`. See Tab Targeting section.
2. **Multi-agent:** Serialize with FIFO queue per tab. Concurrent sessions deferred to v2.
3. **Extension store:** Distribute as unpacked/developer extension initially. Store publishing after validation.

## Remaining Open Item

- **Safari extension parity:** Safari Web Extension API is largely compatible with Chrome MV3 but edge cases may surface during implementation (particularly around `browser.scripting.executeScript` and cookie access). Safari is a stretch goal for v1. Ship Chrome first, validate, then port.
