# exfetch

Structured web retrieval for AI agents, from a terminal or an MCP client.

[![CI](https://github.com/BMC-INC/exfetch/actions/workflows/ci.yml/badge.svg)](https://github.com/BMC-INC/exfetch/actions/workflows/ci.yml)

[Quick start](#quick-start) · [Browser access](#browser-access) · [MCP](#mcp-setup) · [Configuration](#configuration) · [Development](#development)

## What it does

exfetch brings HTTP fetching, text and Markdown extraction, PDF text extraction, and web search into one Rust command-line tool. Its optional browser extension retrieves rendered pages using the browser session you control. MCP tools expose retrieval and browser actions to a local assistant.

| Capability | Entry point |
| --- | --- |
| Fetch HTML and structured text | `exfetch fetch` |
| Extract selected HTML elements | `exfetch fetch --selector` |
| Read a rendered page through Chrome | `exfetch fetch --browser` or MCP `fetch_page` |
| Search DuckDuckGo or a chosen SearXNG instance | `exfetch search` |
| Connect an assistant | `exfetch serve --mcp-stdio` |
| Use local browser actions | MCP `browser_action` |

**Status:** version 0.2.0, a working local developer tool with automated Rust and extension-policy tests. Browser access requires an installed extension and a running bridge. It is not a hosted browser service or a public multi-user gateway.

## Quick start

Requires a current stable Rust toolchain. The background-daemon option uses Unix APIs; macOS and Linux are the intended development environments.

```bash
git clone https://github.com/BMC-INC/exfetch.git
cd exfetch
cargo install --locked --path .
exfetch fetch https://example.com --markdown
```

No browser extension is needed for direct HTTP retrieval.

```bash
exfetch fetch https://example.com --json
exfetch fetch https://example.com --selector p --raw
exfetch fetch https://example.com --max-length 5000
exfetch search 'rust programming language' --results 5
exfetch search 'rust programming language' --fetch --json
```

`--max-length` is a byte limit for extracted content; truncation respects UTF-8 boundaries. PDF text extraction does not perform OCR. A PDF response is printed as extracted text, even when an HTML output-format flag is supplied.

To use SearXNG, supply the URL of an instance you operate or are permitted to use with `--engine searxng --searxng-url`. The same selected engine is used with `--fetch`. Search availability depends on the provider; DuckDuckGo may rate-limit requests or return a challenge.

## Browser access

1. Open `chrome://extensions`, enable Developer mode, and load `extension/chrome/` as an unpacked extension.
2. Start `exfetch serve` in a terminal. The extension discovers the loopback bridge and authenticates locally.
3. In another terminal, run:

```bash
exfetch fetch https://example.com --browser --markdown
```

The CLI sends the request to the running bridge recorded in its configuration directory. It fails clearly when the bridge or extension is unavailable; it does not silently substitute an unauthenticated HTTP fetch. Keep one bridge active while evaluating the CLI workflow.

For an MCP client, start the bridge through the MCP process as described below. Connections belong to that process; a separate daemon does not share its browser session. `connection_status` reports whether that MCP process has an extension.

The Safari folder contains shared extension source, not a prebuilt installable app. See the [Safari build notes](extension/safari/README.md).

## MCP setup

Install `exfetch` on the executable search path visible to your MCP client, or configure the actual full path to the binary. For clients using an `mcpServers` configuration:

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

| Tool | Purpose |
| --- | --- |
| `fetch_page` | Fetch a URL as Markdown, text, HTML, or JSON; optional browser retrieval and CSS selector |
| `search_web` | Search the configured engine and optionally fetch result content |
| `browser_action` | Read DOM, navigate, interact, capture a screenshot, or inspect tabs under the configured policy |
| `connection_status` | Check this process's extension connection |

The implementation negotiates MCP protocol `2024-11-05`. Local stdio is the recommended integration. The optional `--mcp-sse PORT` HTTP transport is for controlled local development and has no complete shared-service authentication model; do not expose it through an unrestricted public tunnel.

## Configuration

Configuration is read from the platform's configuration directory under `exfetch/config.toml`: typically `~/.config/exfetch` on Linux and `~/Library/Application Support/exfetch` on macOS. Set `EXFETCH_CONFIG_DIR` to use a dedicated directory. Invalid configuration, including unknown or misspelled fields, stops startup instead of silently resetting policy.

```toml
[defaults]
timeout = 15
user_agent = "exfetch/0.1"
search_engine = "duckduckgo"

[policy]
blocked_domains = []
allowed_actions = ["click", "type_text", "read_dom", "screenshot", "navigate", "list_tabs", "switch_tab"]
allow_cookie_read = false
cookie_allowed_domains = []
max_requests_per_minute = 60
```

CLI flags override the corresponding fetch/search defaults. Set `defaults.searxng_url` when choosing SearXNG as the default engine. MCP fetch/search operations also use the configured defaults. The server loads its browser policy at startup; restart it after policy changes.

Cookie reading requires both `allow_cookie_read = true` and a matching `cookie_allowed_domains` entry, plus an explicit URL in the request. Arbitrary JavaScript execution is disabled unless `execute_js` is explicitly included in `allowed_actions`.

Domain restrictions are checked for direct requests and redirects, and for browser targets and returned pages. Under a domain-restricted browser policy, actions must identify a URL; broad tab listing and tab-ID switching are rejected because their targets cannot be safely inferred from the supplied URL. Browser navigation can still trigger network requests before the final page is checked: these controls are not a browser firewall or isolation boundary.

Set `[tabs]` `reuse_strategy` to `exact` (default), `domain`, or `none`, and `open_in_background` to `true` (default) or `false` for browser fetches. `policy.log_all` controls server accepted-request count logging; it does not log page content or cookies.

## Trust and data handling

HTTP retrieval validates and pins each destination's resolved IP addresses, including redirects, and rejects private/reserved targets. This also applies to PDF downloads. Direct fetches bypass system proxy configuration so validation refers to the connection's actual destination.

Browser retrieval uses the permissions, cookies, and network access of your browser. The extension requests `<all_urls>` because Chrome requires that grant for assistant-triggered viewport screenshots without a separate toolbar click. Browser page actions still accept only HTTP/HTTPS targets; file, data and browser-internal pages are rejected. Treat the extension and connected assistant as trusted local software. Fetched content is untrusted input; an assistant should not treat instructions inside a page as authorization for further actions.

The bridge binds to loopback, rejects unexpected Host/Origin headers, and uses a local token for browser connections and CLI bridge requests. The token is available to local clients for pairing, so it is not a boundary against other software running as your user. No claim of enterprise isolation, complete SSRF protection across browser actions, or production security certification is made.

## Development

```bash
cargo fmt --all --check
cargo test --locked
node --test tests/extension_policy_tests.cjs
cargo audit
```

Rust tests cover extraction, PDF parsing, MCP dispatch, configuration, direct-request policy, and authenticated CLI-to-extension round trips. Some HTTP tests contact `httpbin.org` and require network access. Node tests validate Chrome/Safari shared policy behavior. A separate real Chromium extension test exercises rendered pages, browser actions, screenshots and policy rejection in a disposable profile. See [release validation](docs/RELEASE_VALIDATION.md) for tested clients and upgrade notes. Signed Safari packaging still requires separate device validation. Screenshots capture the visible viewport; full-page requests return an explicit unsupported-operation error.

| Directory | Responsibility |
| --- | --- |
| `src/fetch`, `src/extract`, `src/output` | Retrieval and content conversion |
| `src/search` | Search engines and result fetching |
| `src/bridge`, `extension/` | Local browser connection and extension commands |
| `src/mcp`, `src/engine` | MCP transport, tool dispatch, and policy |
| `tests/` | Regression tests and fixtures |

Open an [issue](https://github.com/BMC-INC/exfetch/issues) with reproduction steps or submit a focused pull request with relevant tests.

## License

[MIT](LICENSE), matching the existing package license declaration.

Part of the [BMC-INC developer-tools portfolio](https://github.com/BMC-INC).
