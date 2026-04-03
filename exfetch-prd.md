# PRD: exfetch
**Project:** exfetch (BMC-INC/exfetch) | **Stack:** Rust CLI binary | **Status:** Draft

## 1. Feature Objective
A standalone Rust CLI tool that fetches, extracts, and returns clean text content from any URL on the web, with optional self-hosted search, so any LLM terminal tool (Claude Code, etc.) can consume web content through a single reliable command instead of flaky MCP servers or protocol abstractions.

## 2. User Story
**As a** developer using LLM coding agents in the terminal, **I want to** fetch any URL or search query and get clean extracted text on stdout, **so that** I can pipe web content into any tool without depending on unreliable MCP servers, browser extensions, or third-party APIs.

**Acceptance criteria:**
- [ ] `exfetch https://example.com` returns clean extracted text to stdout, exit 0
- [ ] `exfetch https://example.com --raw` returns full HTML to stdout, exit 0
- [ ] `exfetch https://example.com --markdown` returns content as clean markdown
- [ ] `exfetch search "rust async patterns"` returns top 5 result URLs with titles and snippets
- [ ] `exfetch search "rust async patterns" --fetch` returns search results AND fetched content from top 3
- [ ] `exfetch https://example.com/doc.pdf` extracts text from PDF, returns to stdout
- [ ] Handles JS-rendered pages via headless browser when `--render` flag is passed
- [ ] Returns structured JSON when `--json` flag is passed
- [ ] Exits non-zero with clear error message on network failure, 4xx, 5xx, timeout
- [ ] Completes simple fetch in under 2 seconds, JS-rendered fetch in under 10 seconds
- [ ] Zero external API keys required for basic operation
- [ ] Single static binary, no runtime dependencies

## 3. Technical Architecture

**Binary crate structure:**
```
exfetch/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point (clap)
│   ├── fetch/
│   │   ├── mod.rs
│   │   ├── http.rs           # reqwest-based HTTP fetcher
│   │   ├── render.rs         # Headless Chromium for JS pages
│   │   └── pdf.rs            # PDF text extraction
│   ├── extract/
│   │   ├── mod.rs
│   │   ├── readability.rs    # Content extraction, strip nav/ads/junk
│   │   ├── markdown.rs       # HTML to clean markdown conversion
│   │   └── metadata.rs       # Title, description, OG tags
│   ├── search/
│   │   ├── mod.rs
│   │   ├── engine.rs         # SearXNG client OR fallback scraper
│   │   └── result.rs         # Search result types
│   ├── output/
│   │   ├── mod.rs
│   │   ├── text.rs           # Plain text formatter
│   │   ├── json.rs           # JSON structured output
│   │   └── markdown.rs       # Markdown formatter
│   └── config.rs             # Config file (~/.config/exfetch/config.toml)
├── tests/
│   ├── fetch_tests.rs
│   ├── extract_tests.rs
│   └── search_tests.rs
└── README.md
```

**Key dependencies:**
```toml
[dependencies]
clap = { version = "4", features = ["derive"] }
reqwest = { version = "0.12", features = ["rustls-tls", "cookies", "gzip", "brotli", "deflate"] }
tokio = { version = "1", features = ["full"] }
scraper = "0.20"               # HTML parsing + CSS selectors
html2text = "0.12"             # HTML to plain text
url = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
anyhow = "1"
lopdf = "0.34"                 # PDF text extraction
headless_chrome = "1"          # Optional, behind feature flag
toml = "0.8"                   # Config parsing
indicatif = "0.17"             # Progress bars (stderr only)

[features]
default = []
render = ["headless_chrome"]   # Opt-in JS rendering, keeps default binary small
```

**No workspace. Single crate. This is a standalone tool, not ExecLayer infrastructure.**

## 4. CLI Interface (clap derive)

```
exfetch [OPTIONS] <COMMAND>

COMMANDS:
  <URL>                     Fetch and extract content from URL
  search <QUERY>            Search the web and return results

GLOBAL OPTIONS:
  --raw                     Return raw HTML instead of extracted text
  --markdown, -m            Return content as clean markdown
  --json, -j                Return structured JSON output
  --render, -r              Use headless browser for JS-rendered pages (requires `render` feature)
  --timeout <SECS>          Request timeout in seconds [default: 15]
  --max-length <CHARS>      Truncate output to N characters [default: none]
  --headers                 Include response headers in output
  --user-agent <STRING>     Custom user agent [default: "exfetch/0.1"]
  --cookie <KEY=VALUE>      Send cookie with request (repeatable)
  --quiet, -q               Suppress progress/status on stderr
  --verbose, -v             Show request details on stderr

SEARCH OPTIONS:
  --fetch, -f               Also fetch content from top results
  --results <N>             Number of search results [default: 5]
  --engine <ENGINE>         Search engine: searxng, duckduckgo [default: duckduckgo]
  --searxng-url <URL>       SearXNG instance URL (if using searxng engine)
```

**Example flows:**
```bash
# Simple fetch, clean text
exfetch https://docs.rs/tokio/latest/tokio

# Fetch as markdown, pipe to clipboard
exfetch https://some-article.com -m | pbcopy

# Fetch JS-heavy page
exfetch https://spa-app.com --render

# Get structured JSON (title, content, metadata, links)
exfetch https://news-site.com --json

# Search and show results
exfetch search "EU AI Act enforcement timeline 2026"

# Search, fetch top 3, return all content as JSON
exfetch search "Rust async runtime comparison" --fetch --results 3 --json

# PDF extraction
exfetch https://arxiv.org/pdf/2301.00001.pdf

# Truncate for LLM context management
exfetch https://long-article.com --max-length 50000

# Claude Code usage in CLAUDE.md:
# "Use `exfetch <url>` to fetch web content. Use `exfetch search <query>` to search."
```

## 5. Edge Cases & Failure Modes

| Scenario | System Behavior |
|---|---|
| URL returns 403/429 (rate limited) | Retry once with 2s delay and browser-like UA. If still blocked, exit 1 with clear message: "Blocked by server (HTTP 429). Try --render or wait." |
| URL requires JavaScript to render content | Without --render: return whatever static HTML contains, warn on stderr "Page may require --render for full content." With --render: use headless Chromium, wait for network idle. |
| URL points to binary file (image, zip, etc.) | Detect via Content-Type header. Exit 1: "Binary content (image/png). exfetch only handles text, HTML, and PDF." |
| PDF is scanned images, no text layer | lopdf returns empty/garbage. Exit 1: "PDF contains no extractable text (likely scanned images)." |
| URL redirects through 10+ hops | Follow up to 10 redirects. Beyond that, exit 1: "Too many redirects (>10)." Print final URL on stderr. |
| Network timeout | Exit 1: "Request timed out after {N}s." Suggest increasing --timeout. |
| SearXNG instance is down/unreachable | Fall back to DuckDuckGo HTML scraper automatically. Warn on stderr. |
| Extracted content is empty after readability | Return raw text fallback (html2text on full body). Warn on stderr: "Content extraction returned empty, falling back to raw text." |
| URL has no scheme (user passes "example.com") | Auto-prepend https://. If that fails, try http://. |
| Character encoding is not UTF-8 | Detect charset from Content-Type header or meta tag. Transcode to UTF-8. If detection fails, try UTF-8 then Latin-1. |
| Content-Type is application/json | Pretty-print the JSON directly. No extraction needed. |
| --max-length truncation | Truncate at word boundary nearest to limit. Append "[truncated at {N} chars, full content {M} chars]" |

## 6. Security Considerations

- **No credential storage.** Cookies are per-invocation only via --cookie flag. Nothing persists.
- **No arbitrary code execution.** --render runs Chromium in sandbox mode. No --no-sandbox flag exposed.
- **SSRF prevention.** Reject fetches to private IP ranges (10.x, 172.16-31.x, 192.168.x, 127.x, ::1) unless --allow-private flag is explicitly passed. This prevents LLM agents from using exfetch to probe internal networks.
- **Output sanitization.** Strip null bytes and control characters from output. Replace with space.
- **TLS only by default.** HTTP URLs produce a warning on stderr. Add --allow-http to suppress.
- **No eval/exec of fetched content.** Content is always treated as data, never code.
- **Rate self-limiting.** When called in a loop (search --fetch), add 500ms delay between fetches to avoid hammering targets.
- **User agent honesty.** Default UA identifies as exfetch. Do not impersonate browsers by default. --user-agent flag exists for cases where sites block bots.

## 7. Execution Steps

| # | Task | Files | Done When | Effort |
|---|---|---|---|---|
| 1 | Scaffold crate with clap derive CLI | main.rs, Cargo.toml | `exfetch --help` prints usage, `exfetch --version` prints version | XS |
| 2 | HTTP fetcher with reqwest | fetch/http.rs, fetch/mod.rs | `exfetch https://httpbin.org/html` returns HTML body, respects timeout, follows redirects, handles error codes | S |
| 3 | Content extraction (readability) | extract/readability.rs, extract/mod.rs | `exfetch https://en.wikipedia.org/wiki/Rust_(programming_language)` returns article text without nav, sidebar, footer, ads | M |
| 4 | Plain text output formatter | output/text.rs, output/mod.rs | Clean text output on stdout, no HTML tags, readable paragraphs, proper whitespace | S |
| 5 | Markdown output formatter | extract/markdown.rs, output/markdown.rs | `exfetch <url> --markdown` returns clean markdown with headers, links, code blocks preserved | M |
| 6 | JSON structured output | output/json.rs, extract/metadata.rs | `exfetch <url> --json` returns `{"url", "title", "description", "content", "word_count", "links", "fetch_time_ms"}` | S |
| 7 | PDF text extraction | fetch/pdf.rs | `exfetch https://example.com/doc.pdf` extracts and returns text from PDF | S |
| 8 | DuckDuckGo HTML search scraper | search/engine.rs, search/result.rs, search/mod.rs | `exfetch search "query"` returns title, URL, snippet for top N results | M |
| 9 | Search + fetch combo | search/mod.rs | `exfetch search "query" --fetch` returns search results with fetched content from top 3 | S |
| 10 | SearXNG search client | search/engine.rs | `exfetch search "query" --engine searxng --searxng-url http://localhost:8080` works against a SearXNG instance | S |
| 11 | Config file support | config.rs | `~/.config/exfetch/config.toml` for default engine, searxng URL, timeout, user agent | XS |
| 12 | Headless browser rendering (feature-gated) | fetch/render.rs | `exfetch <url> --render` fetches JS-rendered page content with `render` feature enabled | M |
| 13 | SSRF protection + input validation | fetch/http.rs | Private IPs blocked, scheme validation, redirect limit enforced | S |
| 14 | Edge case handling (encoding, binary detection, empty extraction fallback) | fetch/http.rs, extract/readability.rs | All 12 edge cases from section 5 handled with correct exit codes and messages | M |
| 15 | Integration tests | tests/*.rs | Tests cover: static HTML fetch, content extraction quality, JSON output schema, PDF extraction, search results parsing, error cases, SSRF blocking | M |
| 16 | README with Claude Code integration guide | README.md | README includes install, usage, CLAUDE.md snippet for teaching Claude Code to use exfetch | XS |

## 8. Testing Requirements

**Unit tests:**
- HTTP fetcher: mock responses for 200, 301, 403, 404, 429, 500, timeout
- Content extraction: feed known HTML, assert article text extracted, nav/footer stripped
- Markdown conversion: verify headers, links, code blocks survive conversion
- PDF extraction: test against a real small PDF in test fixtures
- Search result parsing: mock DuckDuckGo HTML, assert correct title/URL/snippet extraction
- SSRF: assert private IPs rejected, public IPs allowed
- URL normalization: "example.com" becomes "https://example.com"
- Encoding: test Latin-1, UTF-8, Shift-JIS detection and transcoding
- Truncation: assert word-boundary truncation and suffix message

**Integration tests:**
- Full fetch flow: URL in, clean text out, exit 0
- Full search flow: query in, results out, exit 0
- Search + fetch flow: query in, results with content out
- Error flow: bad URL, exit 1 with message
- JSON output: validate schema matches spec

**Manual QA checklist:**
- [ ] Fetch a JavaScript-heavy SPA with --render, verify content appears
- [ ] Fetch a paywalled article, verify graceful failure message
- [ ] Fetch a 10MB page, verify --max-length truncation works
- [ ] Run `exfetch search "test" --fetch --json` and verify complete pipeline
- [ ] Add exfetch to CLAUDE.md, verify Claude Code uses it correctly in a session
- [ ] Cross-platform: test on macOS and Linux

## 9. Dependencies & Blockers

**Depends on:**
- Nothing. Standalone tool. Zero external service dependencies for core fetch.
- Optional: SearXNG instance for self-hosted search (DuckDuckGo scraper is the default fallback)

**Blocks:**
- Nothing directly. This is a dev tool, not a production dependency.

**External deps:**
- Chromium must be installed for --render feature. headless_chrome can auto-download.
- DuckDuckGo HTML scraping may break if they change markup. Low risk, easy to patch.

**Open questions:**
- Consider adding `--cache` flag with local disk cache (SQLite) to avoid re-fetching same URL within N minutes. Saves tokens when Claude re-reads a page. **Defer to v0.2.**
- Consider `exfetch pipe` mode that reads URLs from stdin, one per line, fetches all in parallel, outputs all. Useful for bulk research. **Defer to v0.2.**

## 10. Definition of Done
- [ ] All acceptance criteria from section 2 pass
- [ ] All 16 execution steps completed
- [ ] All unit and integration tests pass
- [ ] All 12 edge cases handled with correct exit codes
- [ ] SSRF protection verified
- [ ] Binary compiles on macOS (aarch64) and Linux (x86_64)
- [ ] `cargo clippy` clean, `cargo fmt` clean
- [ ] README includes install instructions, usage examples, and CLAUDE.md integration snippet
- [ ] Published to BMC-INC/exfetch on GitHub
- [ ] Binary size under 20MB without render feature, under 50MB with
