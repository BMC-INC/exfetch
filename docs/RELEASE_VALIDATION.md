# 0.2.0 release validation

Tested on 2026-10-29 with the optimized macOS Intel release binary.

| Check | Result |
| --- | --- |
| Official MCP Python SDK 1.30.0 | Initialized protocol 2024-11-05, discovered four tools, fetched HTTPS content and selected a paragraph. |
| Installed Claude Code 2.1.306 | Reported the release server Connected from disposable client settings. No model request was made. |
| Real Chrome for Testing 145 and 156 | Loaded the actual Chrome extension in a disposable profile and authenticated with an isolated bridge. |
| Browser workflow | Rendered-page retrieval, standalone CLI browser retrieval, click/read DOM and a valid PNG viewport screenshot passed. |
| Browser rejection paths | Cookie reading and JavaScript execution were denied by default; blocked redirect and non-web page targets were rejected. |
| Shared extension regressions | Chrome and Safari JavaScript policy tests passed; this is not Safari device acceptance. |

The real-browser test changes only the discovery-port constants in a disposable extension copy so an existing personal bridge cannot be selected. It uses synthetic local pages, no browser login and no personal profile. It closes only its own browser/server processes. The official client test uses an ephemeral bridge port and separate configuration.

Reproduce with `scripts/smoke_mcp_sdk.py --binary target/release/exfetch` in a virtual environment containing `mcp==1.30.0`, and `node scripts/smoke_browser.cjs target/release/exfetch` with Playwright installed. The CI workflow installs the pinned test dependencies and runs these checks alongside Rust and extension policy tests.

Upgrading an unpacked extension requires reloading that extension in Chrome. The changed `<all_urls>` permission may require reviewing or granting site access. This release does not modify a running browser, installed extension, live bridge, or client configuration. Chrome Web Store approval, signed Safari packaging and full desktop GUI acceptance are not claimed.
