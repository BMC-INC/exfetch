# exfetch Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a browser-aware web execution layer that gives LLM agents full authenticated web access through the user's real browser sessions, with CLI, MCP server, and Chrome/Safari extension interfaces.

**Architecture:** Rust binary is the single brain — handles CLI, MCP server, policy enforcement, and command routing. Browser extension is a thin executor connected via WebSocket on localhost. Commands flow: LLM Agent → MCP/CLI → Rust binary → Policy check → HTTP fetcher or WebSocket → Extension → Browser DOM.

**Tech Stack:** Rust (clap, reqwest, tokio, tokio-tungstenite, axum, scraper, html2text, lopdf, serde_json, tracing), JavaScript (Chrome MV3 extension, Safari Web Extension)

**Spec:** `docs/superpowers/specs/2026-04-02-exfetch-browser-agent-design.md`
**PRD:** `exfetch-prd-v2.md`

---

## File Map

### Rust Binary
| File | Responsibility |
|---|---|
| `Cargo.toml` | Dependencies, features, metadata |
| `src/main.rs` | Entry point: parse CLI, dispatch to fetch/search/serve/status |
| `src/lib.rs` | Library root: re-export all modules |
| `src/cli/mod.rs` | Re-export commands |
| `src/cli/commands.rs` | clap derive CLI structs (FetchArgs, SearchArgs, ServeArgs, StatusArgs) |
| `src/fetch/mod.rs` | Re-export fetcher traits and implementations |
| `src/fetch/http.rs` | reqwest HTTP client with SSRF protection, redirect handling, encoding detection |
| `src/fetch/pdf.rs` | PDF text extraction via lopdf |
| `src/extract/mod.rs` | Re-export extractors |
| `src/extract/readability.rs` | Content extraction — strip nav/ads/junk, return article body |
| `src/extract/markdown.rs` | HTML to clean markdown (headers, links, code blocks preserved) |
| `src/extract/metadata.rs` | Title, description, OG tags, word count |
| `src/search/mod.rs` | Search dispatcher — pick engine, combine with fetch |
| `src/search/engine.rs` | DuckDuckGo HTML scraper + SearXNG client |
| `src/search/result.rs` | SearchResult type (title, url, snippet) |
| `src/output/mod.rs` | Re-export formatters |
| `src/output/text.rs` | Plain text formatter for stdout |
| `src/output/json.rs` | JSON structured output (url, title, content, metadata, links, fetch_time_ms) |
| `src/output/markdown.rs` | Markdown formatter for stdout |
| `src/config.rs` | Config file parser (~/.config/exfetch/config.toml), defaults |
| `src/engine/mod.rs` | Re-export engine components |
| `src/engine/command.rs` | Canonical command types shared across CLI, MCP, bridge |
| `src/engine/router.rs` | Route commands to HTTP fetcher or WebSocket bridge |
| `src/engine/policy.rs` | Policy enforcement: domain blocking, action gating, rate limiting, logging |
| `src/bridge/mod.rs` | Re-export bridge components |
| `src/bridge/ws_server.rs` | WebSocket server on localhost, token auth, port fallback |
| `src/bridge/protocol.rs` | Bridge protocol types: request/response envelope, command types |
| `src/bridge/connection.rs` | Connection manager: track extensions, health check, reconnect, multi-profile |
| `src/mcp/mod.rs` | Re-export MCP components |
| `src/mcp/server.rs` | MCP server: stdio transport + SSE/HTTP transport via axum |
| `src/mcp/tools.rs` | MCP tool definitions: fetch_page, search_web, browser_action, connection_status |
| `src/mcp/types.rs` | MCP JSON-RPC request/response types |

### Chrome Extension
| File | Responsibility |
|---|---|
| `extension/chrome/manifest.json` | MV3 manifest with host_permissions, scripting, tabs, cookies permissions |
| `extension/chrome/background.js` | Service worker: WebSocket client, command dispatch, tab targeting, keepalive |
| `extension/chrome/injectors/read_dom.js` | Injectable: DOM querying, content extraction |
| `extension/chrome/injectors/interact.js` | Injectable: click, type, scroll, wait for element |
| `extension/chrome/injectors/capture.js` | Screenshot via captureVisibleTab, scroll-stitch for full page |
| `extension/chrome/injectors/extract.js` | In-page readability extraction for authenticated content |
| `extension/chrome/popup.html` | Status popup HTML |
| `extension/chrome/popup.js` | Popup logic: connection status, recent commands |
| `extension/chrome/icons/` | Extension icons (16, 32, 48, 128px) |

### Tests
| File | Responsibility |
|---|---|
| `tests/fetch_tests.rs` | HTTP fetcher unit tests (mock responses, SSRF, encoding) |
| `tests/extract_tests.rs` | Content extraction tests (HTML fixtures) |
| `tests/search_tests.rs` | Search result parsing tests (mock DuckDuckGo HTML) |
| `tests/bridge_tests.rs` | WebSocket protocol tests (mock extension client) |
| `tests/engine_tests.rs` | Router + policy engine tests |
| `tests/mcp_tests.rs` | MCP tool handler tests |
| `tests/fixtures/` | Test HTML files, PDFs, mock responses |

---

## Chunk 1: Phase 1 — CLI Core

### Task 1: Scaffold Crate with clap CLI

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`
- Create: `src/cli/mod.rs`
- Create: `src/cli/commands.rs`

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "exfetch"
version = "0.1.0"
edition = "2021"
description = "Browser-aware web execution layer for LLM agents"
license = "MIT"
repository = "https://github.com/BMC-INC/exfetch"

[lib]
name = "exfetch"
path = "src/lib.rs"

[[bin]]
name = "exfetch"
path = "src/main.rs"

[dependencies]
clap = { version = "4", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"

[features]
default = []
```

- [ ] **Step 2: Create src/cli/commands.rs with clap derive**

Define the full CLI interface: `Cli` parser with subcommands `Fetch(FetchArgs)`, `Search(SearchArgs)`, `Serve(ServeArgs)`, `Status`. Include all flags from PRD section 8:

FetchArgs: `url`, `--raw`, `--markdown/-m`, `--json/-j`, `--browser/-b`, `--no-browser`, `--selector`, `--timeout` (default 15), `--max-length`, `--user-agent` (default "exfetch/0.1"), `--quiet/-q`, `--verbose/-v`

SearchArgs: `query`, `--fetch/-f`, `--results` (default 5), `--engine` (default "duckduckgo"), `--searxng-url`, `--json/-j`, `--markdown/-m`, `--timeout` (default 15), `--quiet/-q`

ServeArgs: `--port` (default 9876), `--mcp-stdio`, `--mcp-sse <PORT>`, `--daemon/-d`

- [ ] **Step 3: Create src/cli/mod.rs**

```rust
pub mod commands;
```

- [ ] **Step 4: Create src/lib.rs**

```rust
pub mod cli;
```

- [ ] **Step 5: Create src/main.rs**

Entry point: parse CLI with `Cli::parse()`, match on `Commands` enum, print placeholder messages for each subcommand.

- [ ] **Step 6: Build and verify CLI**

Run: `cargo build 2>&1`
Expected: Compiles successfully

Run: `cargo run -- --help`
Expected: Shows usage with Fetch, Search, Serve, Status subcommands

Run: `cargo run -- --version`
Expected: Shows `exfetch 0.1.0`

- [ ] **Step 7: Commit scaffold**

```bash
git add Cargo.toml src/
git commit -m "feat: scaffold crate with clap CLI (fetch, search, serve, status)"
```

---

### Task 2: HTTP Fetcher with reqwest

**Files:**
- Modify: `Cargo.toml` (add reqwest, url, thiserror)
- Create: `src/fetch/mod.rs`
- Create: `src/fetch/http.rs`
- Modify: `src/main.rs` (wire up fetch command)
- Create: `tests/fetch_tests.rs`

- [ ] **Step 1: Add dependencies to Cargo.toml**

```toml
reqwest = { version = "0.12", features = ["rustls-tls", "cookies", "gzip", "brotli", "deflate"] }
url = "2"
thiserror = "1"
```

- [ ] **Step 2: Write failing test for HTTP fetch**

Create `tests/fetch_tests.rs` with tests:
- `test_fetch_success_returns_body`: fetch `https://httpbin.org/html`, assert body contains "Herman Melville", status is 200
- `test_fetch_404_returns_error`: fetch `https://httpbin.org/status/404`, assert error contains "404"
- `test_fetch_auto_prepends_https`: fetch `httpbin.org/html` (no scheme), assert success
- `test_fetch_timeout`: fetch `https://httpbin.org/delay/5` with 1s timeout, assert error

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --test fetch_tests 2>&1`
Expected: Compilation error — `exfetch::fetch::http` doesn't exist yet

- [ ] **Step 4: Implement HTTP fetcher**

Create `src/fetch/http.rs` with:
- `FetchResponse` struct: `body`, `status`, `content_type`, `final_url`, `headers`, `fetch_time_ms`
- `FetchError` enum: `HttpError`, `Timeout`, `TooManyRedirects`, `BinaryContent`, `Network`, `InvalidUrl`
- `normalize_url(raw)` — try parse, auto-prepend `https://` if no scheme
- `fetch_url(url, timeout, user_agent)` — build reqwest client, follow up to 10 redirects, reject 4xx/5xx, reject binary content types, return FetchResponse

Create `src/fetch/mod.rs`:
```rust
pub mod http;
```

Add `pub mod fetch;` to `src/lib.rs`.

- [ ] **Step 5: Wire fetch command into main.rs**

In the `Commands::Fetch` arm, call `fetch::http::fetch_url`, print `response.body`.

- [ ] **Step 6: Run tests**

Run: `cargo test --test fetch_tests 2>&1`
Expected: All 4 tests pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: HTTP fetcher with reqwest (URL normalization, timeout, redirect, binary detection)"
```

---

### Task 3: Content Extraction (Readability)

**Files:**
- Modify: `Cargo.toml` (add scraper, html2text)
- Create: `src/extract/mod.rs`
- Create: `src/extract/readability.rs`
- Create: `src/extract/metadata.rs`
- Create: `tests/extract_tests.rs`
- Create: `tests/fixtures/wikipedia_sample.html`

- [ ] **Step 1: Add dependencies**

```toml
scraper = "0.20"
html2text = "0.12"
```

- [ ] **Step 2: Create test fixture**

Create `tests/fixtures/wikipedia_sample.html` — HTML page with `<nav>`, `<div id="sidebar">`, `<main id="content">` with article text, and `<footer>`. Include `<title>`, `<meta name="description">`, and `<meta property="og:title">` tags.

- [ ] **Step 3: Write failing tests**

Create `tests/extract_tests.rs`:
- `test_extract_article_content`: assert article text present, nav/sidebar/footer text absent
- `test_extract_metadata`: assert title, description, og_title correctly extracted
- `test_extract_empty_html_returns_fallback`: empty body doesn't crash

- [ ] **Step 4: Run test to verify failure**

Run: `cargo test --test extract_tests 2>&1`
Expected: Compilation error

- [ ] **Step 5: Implement readability extraction**

Create `src/extract/readability.rs`:
- `extract(html) -> String`: parse HTML, try content selectors (`main`, `article`, `[role='main']`, `#content`, etc.) in priority order. Strip junk elements (nav, footer, header, aside, .sidebar, .ad, script, style). Convert remaining HTML to plain text via `html2text::from_read`.

Create `src/extract/metadata.rs`:
- `PageMetadata` struct: title, description, og_title, og_description, og_image, word_count
- `extract(html) -> PageMetadata`: parse title tag, meta description, OG tags

Create `src/extract/mod.rs`:
```rust
pub mod readability;
pub mod metadata;
```

Add `pub mod extract;` to `src/lib.rs`.

- [ ] **Step 6: Run tests**

Run: `cargo test --test extract_tests 2>&1`
Expected: All 3 tests pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: content extraction (readability + metadata) with junk stripping"
```

---

### Task 4: Output Formatters (text, markdown, JSON)

**Files:**
- Modify: `Cargo.toml` (add serde, serde_json)
- Create: `src/output/mod.rs`
- Create: `src/output/text.rs`
- Create: `src/output/json.rs`
- Create: `src/output/markdown.rs`
- Modify: `src/main.rs` (wire formatters to fetch command)

- [ ] **Step 1: Add dependencies**

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: Implement output formatters**

`src/output/text.rs`:
- `format(html, max_length) -> String`: extract via readability, apply truncation
- `format_raw(body, max_length) -> String`: truncate at word boundary, append `[truncated at N chars, full content M chars]`

`src/output/json.rs`:
- `JsonOutput` struct (Serialize): url, title, description, content, word_count, fetch_time_ms
- `format(response: &FetchResponse, max_length) -> String`: extract content + metadata, serialize to pretty JSON

`src/output/markdown.rs`:
- `format(html, max_length) -> String`: delegate to readability extraction for now (proper markdown in Task 5)

Add `pub mod output;` to `src/lib.rs`.

- [ ] **Step 3: Wire formatters into main.rs fetch command**

Dispatch on `args.raw` / `args.json` / `args.markdown` flags.

- [ ] **Step 4: Test truncation**

Add `test_truncation_at_word_boundary` to extract_tests.rs.

- [ ] **Step 5: Run tests and manual verification**

Run: `cargo test 2>&1` — all pass
Run: `cargo run -- fetch https://httpbin.org/html --json` — valid JSON output

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: output formatters (text, JSON, markdown) with truncation"
```

---

### Task 5: Markdown Extraction (HTML to Markdown)

**Files:**
- Create: `src/extract/markdown.rs`
- Modify: `src/output/markdown.rs` (use proper converter)
- Create: `tests/fixtures/article_with_formatting.html`

- [ ] **Step 1: Create test fixture**

HTML with `<h1>`, `<h2>`, `<strong>`, `<pre><code>`, `<ul><li><a href>` inside `<main>`.

- [ ] **Step 2: Write failing test**

`test_markdown_preserves_headers_and_links`: assert headers, code blocks, links survive conversion.

- [ ] **Step 3: Implement HTML-to-markdown**

Create `src/extract/markdown.rs`:
- `to_markdown(html) -> String`: find main content area, use `html2text::from_read` for conversion.

Update `src/output/markdown.rs` to call `extract::markdown::to_markdown`.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: HTML to markdown conversion with header/link/code preservation"
```

---

### Task 6: PDF Text Extraction

**Files:**
- Modify: `Cargo.toml` (add lopdf)
- Create: `src/fetch/pdf.rs`
- Create: `tests/fixtures/sample.pdf`

- [ ] **Step 1: Add lopdf**

```toml
lopdf = "0.34"
```

- [ ] **Step 2: Create test PDF fixture**

Generate or include a small PDF with extractable text.

- [ ] **Step 3: Write failing tests**

- `test_pdf_extraction`: read fixture, extract text, assert non-empty
- `test_pdf_invalid_bytes`: pass garbage bytes, assert error

- [ ] **Step 4: Implement PDF extraction**

Create `src/fetch/pdf.rs`:
- `PdfError` enum: ParseError, NoText
- `extract_text(bytes) -> Result<String, PdfError>`: load PDF from bytes via lopdf, iterate pages, extract text. Return error if empty.

Update `src/fetch/mod.rs` to include `pub mod pdf;`.

- [ ] **Step 5: Wire PDF detection into fetch command**

Check `content_type` for "pdf", if so re-fetch as bytes and extract text.

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: PDF text extraction via lopdf"
```

---

### Task 7: DuckDuckGo Search Scraper

**Files:**
- Create: `src/search/mod.rs`
- Create: `src/search/result.rs`
- Create: `src/search/engine.rs`
- Create: `tests/search_tests.rs`
- Create: `tests/fixtures/ddg_results.html`

- [ ] **Step 1: Add urlencoding dependency**

```toml
urlencoding = "2"
```

- [ ] **Step 2: Create mock DuckDuckGo HTML fixture**

Strip-down of DuckDuckGo HTML results page with `.result`, `.result__a`, `.result__snippet` elements.

- [ ] **Step 3: Write failing test**

`test_parse_ddg_results`: parse fixture, assert non-empty results with title and URL.

- [ ] **Step 4: Implement search**

`src/search/result.rs`: `SearchResult` struct (title, url, snippet) with Serialize.

`src/search/engine.rs`:
- `parse_ddg_html(html) -> Vec<SearchResult>`: parse with scraper, extract from `.result` divs
- `search_ddg(query, num_results, timeout) -> Result<Vec<SearchResult>>`: fetch `https://html.duckduckgo.com/html/?q=...`, parse results

`src/search/mod.rs`: re-export engine and result modules.

Add `pub mod search;` to `src/lib.rs`.

- [ ] **Step 5: Wire search into main.rs**

Handle `Commands::Search` — call `search_ddg`, format as plain text or JSON.

- [ ] **Step 6: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: DuckDuckGo HTML search scraper"
```

---

### Task 8: Search + Fetch Combo and Config File

**Files:**
- Modify: `src/search/mod.rs` (add fetch combo)
- Create: `src/config.rs`
- Modify: `Cargo.toml` (add toml, dirs)
- Modify: `src/main.rs` (wire config, search --fetch)

- [ ] **Step 1: Add dependencies**

```toml
toml = "0.8"
dirs = "5"
```

- [ ] **Step 2: Implement config**

`src/config.rs`:
- `Config` struct (Deserialize, Default): `defaults`, `policy`, `tabs` sub-structs
- `Defaults`: timeout, user_agent, search_engine, searxng_url
- `PolicyConfig`: blocked_domains, allowed_actions, allow_cookie_read, cookie_allowed_domains, log_all, max_requests_per_minute
- `TabConfig`: reuse_strategy, open_in_background
- `config_dir() -> PathBuf` and `load() -> Config` (reads file or returns defaults)

Add `pub mod config;` to `src/lib.rs`.

- [ ] **Step 3: Implement search + fetch combo**

Add `FetchedSearchResult` struct and `search_and_fetch()` function to `src/search/mod.rs`. Fetches top N results with 500ms delay between fetches.

- [ ] **Step 4: Wire into main.rs**

Handle `--fetch` flag in `Commands::Search`.

- [ ] **Step 5: Run tests and manual verify**

Run: `cargo test 2>&1` — all pass
Run: `cargo run -- search "rust async patterns" --json` — returns JSON results

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: search+fetch combo and config file support"
```

**Phase 1 complete.** CLI tool works standalone for all basic operations.

---

## Chunk 2: Phase 2 — Bridge + Engine

### Task 9: WebSocket Server

**Files:**
- Modify: `Cargo.toml` (add tokio-tungstenite, uuid, sha2, rand, hex, chrono)
- Create: `src/bridge/mod.rs`
- Create: `src/bridge/protocol.rs`
- Create: `src/bridge/ws_server.rs`
- Create: `src/bridge/connection.rs`
- Create: `tests/bridge_tests.rs`

- [ ] **Step 1: Add dependencies**

```toml
tokio-tungstenite = "0.24"
uuid = { version = "1", features = ["v4"] }
sha2 = "0.10"
rand = "0.8"
hex = "0.4"
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
futures-util = "0.3"
```

- [ ] **Step 2: Write failing tests**

Create `tests/bridge_tests.rs`:
- `test_ws_server_accepts_authenticated_connection`: start test server, connect with valid token, send auth message, assert acknowledged response
- `test_ws_server_rejects_bad_token`: connect with wrong token, assert rejection/disconnect

- [ ] **Step 3: Implement bridge protocol types**

`src/bridge/protocol.rs`:
- `BridgeMessage` struct: id (uuid), msg_type (Request/Response/Event), command, params (serde_json::Value), timestamp
- `MessageType` enum: Request, Response, Event
- Constructors: `new_request(command, params)`, `new_response(id, command, params)`

- [ ] **Step 4: Implement connection manager**

`src/bridge/connection.rs`:
- `ExtensionConnection` struct: id, browser, profile, sender (mpsc::UnboundedSender<String>), connected_at
- `ConnectionManager`: HashMap of connections behind Arc<RwLock>, methods: add, remove, has_connections, get_default, send_to_default

- [ ] **Step 5: Implement WebSocket server**

`src/bridge/ws_server.rs`:
- `generate_token() -> String`: 32 random bytes, hex-encoded
- `start(port, token, connections) -> Result<u16>`: bind to 127.0.0.1, try ports port..port+10, write port file to `~/.config/exfetch/port`, spawn connection handler
- Connection handler: accept WebSocket, wait for auth message (5s timeout), validate token, set up bidirectional message forwarding, add to ConnectionManager on success
- `start_test_server() -> (u16, String)`: helper for tests (bind port 0 for random)

`src/bridge/mod.rs`:
```rust
pub mod protocol;
pub mod ws_server;
pub mod connection;
```

Add `pub mod bridge;` to `src/lib.rs`.

- [ ] **Step 6: Run tests**

Run: `cargo test --test bridge_tests 2>&1`
Expected: Both tests pass

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: WebSocket server with token auth, connection manager, bridge protocol"
```

---

### Task 10: Command Engine + Router

**Files:**
- Create: `src/engine/mod.rs`
- Create: `src/engine/command.rs`
- Create: `src/engine/router.rs`
- Create: `tests/engine_tests.rs`

- [ ] **Step 1: Write failing test**

`test_router_uses_http_when_no_extension`: create router with empty ConnectionManager, assert `determine_route` returns `Route::Http` for FetchPage.

- [ ] **Step 2: Implement command types**

`src/engine/command.rs`:
- `Command` enum: FetchPage{url, use_browser}, SearchWeb{query, num_results, fetch_results}, BrowserAction{action, selector, text, url, tab_id, code, full_page}, ConnectionStatus
- `BrowserActionType` enum: Click, TypeText, ReadDom, Screenshot, Navigate, ListTabs, SwitchTab, GetCookies, ExecuteJs
- `as_str()` method on BrowserActionType

- [ ] **Step 3: Implement router**

`src/engine/router.rs`:
- `Route` enum: Http, Extension, Error(String)
- `Router` struct with ConnectionManager
- `determine_route(command) -> Route`: FetchPage with browser → Extension if connected else Http; BrowserAction → Extension if connected else Error; SearchWeb → Http; ConnectionStatus → Http

`src/engine/mod.rs`:
```rust
pub mod command;
pub mod router;
pub mod policy;
```

Add `pub mod engine;` to `src/lib.rs`.

- [ ] **Step 4: Run tests**

Run: `cargo test --test engine_tests 2>&1` — pass

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: command engine with router (HTTP/extension routing)"
```

---

### Task 11: Policy Enforcement

**Files:**
- Create: `src/engine/policy.rs`
- Modify: `tests/engine_tests.rs`

- [ ] **Step 1: Write failing tests**

Add to `tests/engine_tests.rs`:
- `test_policy_blocks_domain`: blocked domain returns Block, allowed domain returns Allow
- `test_policy_blocks_execute_js_by_default`: ExecuteJs returns Block, Click returns Allow
- `test_policy_rate_limiting`: exceed limit, assert Block
- `test_policy_blocks_cookies_by_default`: GetCookies returns Block when allow_cookie_read is false

- [ ] **Step 2: Implement policy engine**

`src/engine/policy.rs`:
- `PolicyVerdict` enum: Allow, Block(String)
- `PolicyEngine` struct: allowed_domains, blocked_domains, allowed_actions (HashSet), allow_cookie_read, max_requests_per_minute, request_count (AtomicU32), last_reset (Mutex<Instant>)
- Default: all domains allowed, execute_js NOT in allowed_actions, get_cookies blocked, logging on, 60 req/min
- Methods: `check_domain(url)`, `check_action(action)`, `check_rate_limit()`, `record_request()`
- Domain matching: exact match and wildcard `*.example.com`
- Rate limiting: reset counter every 60 seconds

- [ ] **Step 3: Run tests**

Run: `cargo test --test engine_tests 2>&1` — all pass

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: policy engine (domain blocking, action gating, rate limiting, cookie protection)"
```

**Phase 2 complete.**

---

## Chunk 3: Phase 3 — MCP Server

### Task 12: MCP Server (stdio transport)

**Files:**
- Modify: `Cargo.toml` (add axum, tower, tracing, tracing-subscriber)
- Create: `src/mcp/mod.rs`
- Create: `src/mcp/types.rs`
- Create: `src/mcp/tools.rs`
- Create: `src/mcp/server.rs`
- Create: `tests/mcp_tests.rs`

- [ ] **Step 1: Add dependencies**

```toml
axum = "0.8"
tower = "0.5"
tracing = "0.1"
tracing-subscriber = "0.3"
```

- [ ] **Step 2: Implement MCP types**

`src/mcp/types.rs`:
- `JsonRpcRequest`: jsonrpc, id, method, params
- `JsonRpcResponse`: jsonrpc, id, result?, error?
- `JsonRpcError`: code, message
- `ToolDefinition`: name, description, inputSchema
- Constructors: `JsonRpcResponse::success(id, result)`, `JsonRpcResponse::error(id, code, msg)`

- [ ] **Step 3: Implement tool definitions**

`src/mcp/tools.rs`:
- `get_tool_definitions() -> Vec<ToolDefinition>`: return definitions for `fetch_page`, `search_web`, `browser_action`, `connection_status` with full JSON Schema input schemas matching PRD section 7

- [ ] **Step 4: Implement stdio MCP server**

`src/mcp/server.rs`:
- `run_stdio(connections, policy)`: read JSON-RPC from stdin line-by-line, dispatch to handler, write response to stdout
- `handle_request(req, connections, policy) -> JsonRpcResponse`: route by method name:
  - `initialize` → return server info + capabilities
  - `tools/list` → return tool definitions
  - `tools/call` → dispatch to tool handler by name
  - `notifications/initialized` → ack
- Tool handlers:
  - `handle_fetch_page`: call `fetch::http::fetch_url`, format based on `format` param, return content
  - `handle_search_web`: call search functions, return results
  - `handle_browser_action`: check extension connected, route through bridge (or return error)
  - `handle_connection_status`: return connection state

`src/mcp/mod.rs`:
```rust
pub mod types;
pub mod tools;
pub mod server;
```

Add `pub mod mcp;` to `src/lib.rs`.

- [ ] **Step 5: Wire serve command into main.rs**

Update `Commands::Serve`: if `--mcp-stdio`, run `mcp::server::run_stdio`. Otherwise start WebSocket server + optional SSE, wait for ctrl-c.

- [ ] **Step 6: Write tests**

`tests/mcp_tests.rs`:
- `test_tool_definitions_valid`: assert 4 tools, correct names, valid schemas

- [ ] **Step 7: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: MCP server with stdio transport (fetch_page, search_web, browser_action, connection_status)"
```

---

### Task 13: MCP SSE Transport

**Files:**
- Modify: `Cargo.toml` (add tokio-stream, futures-util)
- Modify: `src/mcp/server.rs` (add SSE transport)

- [ ] **Step 1: Add dependencies**

```toml
tokio-stream = "0.1"
futures-util = "0.3"
```

- [ ] **Step 2: Implement SSE transport**

Add to `src/mcp/server.rs`:
- `run_sse(port, connections, policy)`: create axum Router with `GET /sse` (SSE stream) and `POST /message` (JSON-RPC handler), serve on 127.0.0.1:port
- SSE handler pushes responses via broadcast channel
- POST handler dispatches to same `handle_request` function

- [ ] **Step 3: Wire into serve command**

If `--mcp-sse <port>` is passed, spawn SSE server alongside WebSocket.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: MCP SSE transport via axum (for claude.ai remote connection)"
```

**Phase 3 complete.**

---

## Chunk 4: Phase 4 — Chrome Extension

### Task 14: Chrome Extension (Manifest + Background Worker)

**Files:**
- Create: `extension/chrome/manifest.json`
- Create: `extension/chrome/background.js`
- Create: `extension/chrome/popup.html`
- Create: `extension/chrome/popup.js`
- Create: `extension/chrome/icons/` (placeholder icons)

- [ ] **Step 1: Create manifest.json**

Manifest V3 with:
- permissions: `scripting`, `tabs`, `cookies`, `storage`, `activeTab`, `alarms`
- host_permissions: `*://*/*`
- background service_worker: `background.js`
- action with popup

- [ ] **Step 2: Create background.js**

The service worker is the core of the extension. It handles:

**WebSocket client:**
- Port discovery: try 9876-9886 sequentially, cache working port
- Token acquisition: fetch from `http://127.0.0.1:{port}/.well-known/exfetch-token`
- Auth handshake: send connected event with token, browser, profile
- Reconnection: 5-second retry on disconnect
- Keepalive: `chrome.alarms` every 25 seconds

**Command dispatch:**
- Receive request messages from WebSocket
- Route to command handler by command name
- Send response back via WebSocket

**Command implementations (all use `chrome.scripting.executeScript` for DOM access):**
- `fetch_page`: navigate to URL, wait for load, extract outerHTML + title + URL
- `read_dom`: query selector on target tab, return elements
- `click`: find element by selector, click it
- `type_text`: find element, set value, dispatch input/change events
- `navigate`: update tab URL, wait for load
- `screenshot`: activate tab, use `chrome.tabs.captureVisibleTab()`, return base64
- `get_cookies`: use `chrome.cookies.getAll()`
- `list_tabs`: use `chrome.tabs.query({})`
- `switch_tab`: use `chrome.tabs.update(tabId, {active: true})`
- `execute_js`: use `chrome.scripting.executeScript` with user-provided code string. **SECURITY NOTE: This command is blocked by default in the policy engine. It only executes if the Rust binary's policy check explicitly allows it. The extension trusts authenticated commands from the binary.**

**Tab targeting:**
- Exact URL match → reuse tab
- Same origin match → reuse for navigate
- No match → create new background tab
- No URL specified → use active tab

- [ ] **Step 3: Create popup**

`popup.html`: simple status display (connection dot, status text, port info)
`popup.js`: query background worker for connection status

- [ ] **Step 4: Create placeholder icons**

Simple colored square PNGs at 16x16, 48x48, 128x128.

- [ ] **Step 5: Commit**

```bash
git add extension/
git commit -m "feat: Chrome extension (MV3) with WebSocket client, command dispatch, tab targeting"
```

---

### Task 15: Wire Browser Actions Through the Bridge

**Files:**
- Modify: `src/bridge/connection.rs` (add request/response tracking)
- Modify: `src/bridge/ws_server.rs` (route responses to pending requests)
- Modify: `src/mcp/server.rs` (wire browser_action through bridge)

- [ ] **Step 1: Add pending request tracking**

Add `PendingRequests` to `src/bridge/connection.rs`:
- HashMap of request ID → `oneshot::Sender<serde_json::Value>`
- `register(id) -> oneshot::Receiver` — register a pending request
- `complete(id, result)` — resolve a pending request

Add `send_command(command, params, pending) -> Result<Value>` to ConnectionManager:
- Create BridgeMessage, register pending, send via WebSocket, await response with 10s timeout

- [ ] **Step 2: Route extension responses to pending requests**

In `ws_server.rs` message handler, when a Response type message arrives, call `pending.complete(msg.id, msg.params)`.

- [ ] **Step 3: Wire browser_action MCP handler**

Update `handle_browser_action` in `src/mcp/server.rs` to:
1. Check extension connected
2. Check policy (action gating, domain blocking)
3. Build bridge command from MCP params
4. Send through `connections.send_command`
5. Return result

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: wire browser actions through WebSocket bridge with request/response tracking"
```

**Phase 4 complete.**

---

## Chunk 5: Phase 5 — Hardening

### Task 16: Safari Web Extension (Stretch Goal)

**Files:**
- Create: `extension/safari/` directory
- Create: `extension/safari/README.md`

- [ ] **Step 1: Create Safari extension scaffold**

Create directory structure and README explaining Xcode build steps. Copy shared JS to `extension/safari/Shared/`. Create basic `manifest.json` for Safari.

This is a stretch goal — just the scaffold and build instructions.

- [ ] **Step 2: Commit**

```bash
git add extension/safari/
git commit -m "feat: Safari extension scaffold (stretch goal)"
```

---

### Task 17: SSRF Protection + Input Validation

**Files:**
- Modify: `src/fetch/http.rs`
- Modify: `tests/fetch_tests.rs`

- [ ] **Step 1: Write failing SSRF tests**

- `test_ssrf_blocks_localhost`: fetch `http://127.0.0.1:8080`, assert error
- `test_ssrf_blocks_private_ip`: fetch `http://192.168.1.1`, assert error
- `test_ssrf_blocks_ipv6_loopback`: fetch `http://[::1]:8080`, assert error

- [ ] **Step 2: Implement SSRF protection**

Add `is_private_ip(ip: &IpAddr) -> bool` in `src/fetch/http.rs`:
- Check IPv4: loopback, private, link-local, 0.0.0.0/8
- Check IPv6: loopback, link-local (fe80::/10), unique local (fc00::/7)

Add DNS resolution check before HTTP request in `fetch_url`:
- Resolve hostname to IP
- Check against private ranges
- Reject with `FetchError::SsrfBlocked` if private

- [ ] **Step 3: Run tests**

Run: `cargo test 2>&1` — SSRF tests pass

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat: SSRF protection (block private IPs, DNS rebinding defense)"
```

---

### Task 18: Edge Case Handling

**Files:**
- Modify: `src/fetch/http.rs` (encoding, JSON content)
- Modify: `src/extract/readability.rs` (empty fallback)
- Modify: `src/mcp/server.rs` (MCP truncation default)

- [ ] **Step 1: JSON content-type handling**

If Content-Type is `application/json`, pretty-print directly without extraction.

- [ ] **Step 2: Empty extraction fallback**

If readability returns empty, fall back to `html2text::from_read` on full body. Warn on stderr.

- [ ] **Step 3: MCP large page truncation**

Default `max_length` to 500,000 chars for MCP responses when not specified by caller.

- [ ] **Step 4: Run tests**

Run: `cargo test 2>&1` — all pass

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: edge case handling (JSON content, empty extraction fallback, MCP truncation)"
```

---

### Task 19: Integration Tests

**Files:**
- Modify: `tests/fetch_tests.rs`
- Modify: `tests/bridge_tests.rs`
- Modify: `tests/engine_tests.rs`
- Modify: `tests/mcp_tests.rs`

- [ ] **Step 1: CLI integration tests**

Full fetch flow, search flow, JSON output schema, error cases.

- [ ] **Step 2: Bridge integration tests**

WebSocket handshake + command round-trip with mock client.

- [ ] **Step 3: Policy integration tests**

End-to-end: blocked domain through full pipeline.

- [ ] **Step 4: Run all tests + lint**

Run: `cargo test 2>&1` — all pass
Run: `cargo clippy 2>&1` — no warnings
Run: `cargo fmt --check 2>&1` — clean

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "test: comprehensive integration tests for all modules"
```

---

## Chunk 6: Phase 6 — Ship

### Task 20: README with Claude Code Integration Guide

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write README**

Sections:
- Overview / architecture diagram
- Install (cargo install from source)
- CLI usage examples (fetch, search, serve, status)
- MCP setup for Claude Code (settings.json snippet)
- MCP setup for claude.ai (tunnel instructions — Cloudflare Tunnel / ngrok)
- Chrome extension install (load unpacked from extension/chrome/)
- Config file reference (~/.config/exfetch/config.toml)
- Policy configuration (domain blocking, action gating, cookie access)
- CLAUDE.md integration: `Use exfetch <url> to fetch web content. Use exfetch search <query> to search the web.`

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: README with install, usage, MCP config, CLAUDE.md integration"
```

---

### Task 21: SearXNG Search Client

**Files:**
- Modify: `src/search/engine.rs`

- [ ] **Step 1: Implement SearXNG client**

`search_searxng(query, num_results, timeout, instance_url) -> Result<Vec<SearchResult>>`:
Call `{instance_url}/search?q={query}&format=json`, parse JSON response.

- [ ] **Step 2: Wire engine selection**

Route based on `--engine` flag or config `defaults.search_engine`.

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "feat: SearXNG search client as alternative engine"
```

---

### Task 22: Daemon Mode

**Files:**
- Modify: `Cargo.toml` (add daemonize)
- Modify: `src/main.rs`

- [ ] **Step 1: Add daemonize**

```toml
daemonize = "0.5"
```

- [ ] **Step 2: Implement daemon mode**

When `--daemon` passed to `exfetch serve`:
- Fork to background
- Write PID to `~/.config/exfetch/exfetch.pid`
- Redirect stdout/stderr to `~/.local/share/exfetch/exfetch.log`

- [ ] **Step 3: Implement status command**

Read PID file, check if process running, report status.

- [ ] **Step 4: Final lint**

Run: `cargo clippy 2>&1 && cargo fmt --check 2>&1` — clean

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: daemon mode with PID file, log rotation, status command"
```

---

### Task 23: Push to GitHub

- [ ] **Step 1: Push**

```bash
git push -u origin main
```

- [ ] **Step 2: Verify**

Run: `gh repo view BMC-INC/exfetch --web`

**Implementation complete.** All 22 PRD execution steps covered across 23 plan tasks in 6 phases.
