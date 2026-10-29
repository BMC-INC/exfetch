use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;
use tracing;

use crate::bridge::connection::ConnectionManager;
use crate::engine::command::BrowserActionType;
use crate::engine::policy::{PolicyEngine, PolicyVerdict};
use crate::extract::readability;
use crate::fetch::http::{fetch_url_with_policy, FetchResponse};
use crate::output;
use crate::search;

use super::tools::get_tool_definitions;
use super::types::{JsonRpcRequest, JsonRpcResponse};

// ---------------------------------------------------------------------------
// Stdio transport
// ---------------------------------------------------------------------------

/// Run the MCP server reading JSON-RPC messages line-by-line from stdin and
/// writing responses to stdout.
pub async fn run_stdio(connections: ConnectionManager, policy: Arc<PolicyEngine>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    tracing::info!("MCP stdio server started");

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let err_resp = JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32700,
                    format!("Parse error: {}", e),
                );
                let out = serde_json::to_string(&err_resp).unwrap_or_default();
                stdout.write_all(out.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        let response = handle_request(&request, &connections, &policy).await;

        // Notifications (no id) get no response per JSON-RPC spec
        if let Some(response) = response {
            let out = serde_json::to_string(&response).unwrap_or_default();
            stdout.write_all(out.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    tracing::info!("MCP stdio server shutting down");
    Ok(())
}

// ---------------------------------------------------------------------------
// SSE transport
// ---------------------------------------------------------------------------

/// Shared state for the SSE transport.
#[derive(Clone)]
struct SseState {
    connections: ConnectionManager,
    policy: Arc<PolicyEngine>,
    tx: broadcast::Sender<String>,
}

/// Run the MCP server over HTTP with SSE streaming and a POST endpoint for
/// JSON-RPC messages.
pub async fn run_sse(
    port: u16,
    connections: ConnectionManager,
    policy: Arc<PolicyEngine>,
) -> Result<()> {
    let (tx, _) = broadcast::channel::<String>(256);

    let state = SseState {
        connections,
        policy,
        tx,
    };

    let app = sse_router(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("MCP SSE server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

fn sse_router(state: SseState) -> Router {
    Router::new()
        .route("/", get(sse_handler).post(mcp_handler))
        .route("/sse", get(sse_handler))
        .route("/mcp", post(mcp_handler))
        .route("/message", post(message_handler))
        .with_state(state)
        .layer(axum::middleware::from_fn(
            crate::bridge::ws_server::require_loopback_host,
        ))
}

/// SSE endpoint using a raw streaming body to ensure immediate flush
/// through Cloudflare and other reverse proxies.
async fn sse_handler(
    State(state): State<SseState>,
    req: axum::http::Request<axum::body::Body>,
) -> impl IntoResponse {
    // Build the full message endpoint URL from the incoming request
    let scheme = req
        .headers()
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost:9877");
    let message_url = format!("{}://{}/message", scheme, host);

    let rx = state.tx.subscribe();

    let stream = async_stream::stream! {
        // Send endpoint event immediately
        yield Ok::<_, std::convert::Infallible>(
            format!("event: endpoint\ndata: {}\n\n", message_url)
        );

        // Send a padding comment to force Cloudflare to flush
        // Cloudflare buffers small responses; sending >1KB forces a flush
        let padding = ": ".to_string() + &" ".repeat(2048) + "\n\n";
        yield Ok(padding);

        // Keep-alive + response events
        let mut rx = rx;
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Ok(data) => {
                            yield Ok(format!("event: message\ndata: {}\n\n", data));
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(_) => break,
                    }
                }
                _ = interval.tick() => {
                    yield Ok(": keepalive\n\n".to_string());
                }
            }
        }
    };

    let body = axum::body::Body::from_stream(stream);

    axum::http::Response::builder()
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache, no-transform")
        .header("x-accel-buffering", "no")
        .header("connection", "keep-alive")
        .body(body)
        .unwrap()
}

/// POST /message endpoint: receives JSON-RPC requests and dispatches them.
/// Responses are both returned inline and broadcast to SSE clients.
async fn message_handler(
    State(state): State<SseState>,
    Json(request): Json<JsonRpcRequest>,
) -> impl IntoResponse {
    let response = handle_request(&request, &state.connections, &state.policy).await;

    if let Some(ref resp) = response {
        if let Ok(json_str) = serde_json::to_string(resp) {
            // Best-effort broadcast to SSE subscribers
            let _ = state.tx.send(json_str);
        }
    }

    match response {
        Some(resp) => Json(serde_json::to_value(&resp).unwrap_or(serde_json::Value::Null)),
        None => Json(serde_json::json!({"status": "ok"})),
    }
}

/// Streamable HTTP MCP handler (newer transport).
/// Accepts POST with JSON-RPC, returns response as SSE stream or JSON.
/// This is the transport claude.ai prefers for remote MCP servers.
async fn mcp_handler(
    State(state): State<SseState>,
    req: axum::http::Request<axum::body::Body>,
) -> impl IntoResponse {
    // Parse the body as JSON-RPC
    let body_bytes = match axum::body::to_bytes(req.into_body(), 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return axum::http::Response::builder()
                .status(400)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","error":{"code":-32700,"message":"Parse error"}}"#,
                ))
                .unwrap();
        }
    };

    let request: JsonRpcRequest = match serde_json::from_slice(&body_bytes) {
        Ok(r) => r,
        Err(e) => {
            return axum::http::Response::builder()
                .status(400)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(format!(
                    r#"{{"jsonrpc":"2.0","error":{{"code":-32700,"message":"Parse error: {}"}}}}"#,
                    e
                )))
                .unwrap();
        }
    };

    let response = handle_request(&request, &state.connections, &state.policy).await;

    // For notifications (no id), return 202 Accepted
    if request.id.is_none() {
        return axum::http::Response::builder()
            .status(202)
            .body(axum::body::Body::empty())
            .unwrap();
    }

    // Return as SSE stream (single event, then close) — this is what Streamable HTTP expects
    let resp_json = match response {
        Some(resp) => serde_json::to_string(&resp).unwrap_or_default(),
        None => {
            return axum::http::Response::builder()
                .status(202)
                .body(axum::body::Body::empty())
                .unwrap()
        }
    };

    let sse_body = format!("event: message\ndata: {}\n\n", resp_json);

    axum::http::Response::builder()
        .status(200)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .body(axum::body::Body::from(sse_body))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Request dispatcher
// ---------------------------------------------------------------------------

/// Dispatch a single JSON-RPC request. Returns `None` for notifications
/// (requests without an id).
async fn handle_request(
    req: &JsonRpcRequest,
    connections: &ConnectionManager,
    policy: &PolicyEngine,
) -> Option<JsonRpcResponse> {
    let id = match &req.id {
        Some(id) => id.clone(),
        None => {
            // Notification — no response required
            tracing::debug!("received notification: {}", req.method);
            return None;
        }
    };

    let response = match req.method.as_str() {
        "initialize" => handle_initialize(id),
        "notifications/initialized" => {
            // Client acknowledges initialization; this is a notification but
            // some clients send it with an id — respond with success either way.
            JsonRpcResponse::success(id, serde_json::json!({}))
        }
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, &req.params, connections, policy).await,
        _ => JsonRpcResponse::error(id, -32601, format!("Method not found: {}", req.method)),
    };

    Some(response)
}

// ---------------------------------------------------------------------------
// Method handlers
// ---------------------------------------------------------------------------

fn handle_initialize(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "exfetch",
                "version": env!("CARGO_PKG_VERSION")
            }
        }),
    )
}

fn handle_tools_list(id: serde_json::Value) -> JsonRpcResponse {
    let tools = get_tool_definitions();
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "tools": tools
        }),
    )
}

async fn handle_tools_call(
    id: serde_json::Value,
    params: &serde_json::Value,
    connections: &ConnectionManager,
    policy: &PolicyEngine,
) -> JsonRpcResponse {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    if tool_name != "connection_status" {
        if let PolicyVerdict::Block(reason) = policy.check_rate_limit() {
            return JsonRpcResponse::error(id, -32000, reason);
        }
        policy.record_request();
        if let Some(url) = arguments.get("url").and_then(|value| value.as_str()) {
            if let PolicyVerdict::Block(reason) = policy.check_domain(url) {
                return JsonRpcResponse::error(id, -32000, reason);
            }
        }
    }
    match tool_name {
        "fetch_page" => handle_fetch_page(id, &arguments, connections, policy).await,
        "search_web" => handle_search_web(id, &arguments, policy).await,
        "browser_action" => handle_browser_action(id, &arguments, connections, policy).await,
        "connection_status" => handle_connection_status(id, connections).await,
        _ => JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name)),
    }
}

// ---------------------------------------------------------------------------
// Tool handlers
// ---------------------------------------------------------------------------

async fn handle_fetch_page(
    id: serde_json::Value,
    args: &serde_json::Value,
    connections: &ConnectionManager,
    policy: &PolicyEngine,
) -> JsonRpcResponse {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return JsonRpcResponse::error(id, -32602, "Missing required parameter: url"),
    };

    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");

    let max_length = Some(
        args.get("max_length")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(500_000),
    );

    let config = match crate::config::load() {
        Ok(config) => config,
        Err(error) => return JsonRpcResponse::error(id, -32000, error.to_string()),
    };
    let timeout = Duration::from_secs(config.defaults.timeout);

    let use_browser = args
        .get("use_browser")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let response = if use_browser {
        if !connections.has_connections().await {
            return JsonRpcResponse::error(
                id,
                -32000,
                "Browser fetching requires a connected extension in this MCP process",
            );
        }
        match connections
            .send_command("fetch_page", serde_json::json!({"url": url, "blocked_domains": policy.blocked_domains, "allowed_domains": policy.allowed_domains, "reuse_strategy": config.tabs.reuse_strategy, "open_in_background": config.tabs.open_in_background}))
            .await
        {
            Ok(result) => {
                if result.get("success").and_then(|value| value.as_bool()) == Some(false) {
                    return JsonRpcResponse::error(
                        id,
                        -32000,
                        result
                            .get("error")
                            .and_then(|value| value.as_str())
                            .unwrap_or("Browser fetch failed"),
                    );
                }
                let Some(html) = result.get("html").and_then(|value| value.as_str()) else {
                    return JsonRpcResponse::error(id, -32000, "Browser returned no HTML");
                };
                Ok(FetchResponse {
                    body: html.to_string(),
                    status: 200,
                    content_type: "text/html".into(),
                    final_url: result
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or(url)
                        .to_string(),
                    headers: Vec::new(),
                    fetch_time_ms: 0,
                })
            }
            Err(error) => return JsonRpcResponse::error(id, -32000, error.to_string()),
        }
    } else {
        fetch_url_with_policy(url, timeout, &config.defaults.user_agent, policy).await
    };

    match response {
        Ok(mut resp) => {
            if let PolicyVerdict::Block(reason) = policy.check_domain(&resp.final_url) {
                return JsonRpcResponse::error(id, -32000, reason);
            }
            if let Some(selector) = args.get("selector").and_then(|value| value.as_str()) {
                match crate::extract::select_html(&resp.body, selector) {
                    Ok(html) => resp.body = html,
                    Err(error) => return JsonRpcResponse::error(id, -32602, error),
                }
            }
            let content = match format {
                "html" | "raw" => output::text::format_raw(&resp.body, max_length),
                "text" => {
                    let extracted = readability::extract(&resp.body);
                    output::text::format_raw(&extracted, max_length)
                }
                "json" => output::json::format(&resp, max_length),
                // default: markdown
                _ => output::markdown::format(&resp.body, max_length),
            };

            JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": content
                    }]
                }),
            )
        }
        Err(e) => JsonRpcResponse::error(id, -32000, format!("Fetch failed: {}", e)),
    }
}

async fn handle_search_web(
    id: serde_json::Value,
    args: &serde_json::Value,
    policy: &PolicyEngine,
) -> JsonRpcResponse {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return JsonRpcResponse::error(id, -32602, "Missing required parameter: query"),
    };

    let num_results = args
        .get("num_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(5) as usize;

    let fetch_results = args
        .get("fetch_results")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let fetch_count = args
        .get("fetch_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(3) as usize;

    let config = match crate::config::load() {
        Ok(config) => config,
        Err(error) => return JsonRpcResponse::error(id, -32000, error.to_string()),
    };
    let timeout = Duration::from_secs(config.defaults.timeout);
    let results = match config.defaults.search_engine.as_str() {
        "duckduckgo" => search::engine::search_ddg(query, num_results, timeout).await,
        "searxng" => match config.defaults.searxng_url.as_deref() {
            Some(instance) => {
                search::engine::search_searxng(query, num_results, timeout, instance).await
            }
            None => {
                return JsonRpcResponse::error(
                    id,
                    -32602,
                    "Configure defaults.searxng_url before choosing SearXNG",
                )
            }
        },
        _ => return JsonRpcResponse::error(id, -32602, "Unknown configured search engine"),
    };
    let results = match results {
        Ok(results) => results,
        Err(error) => {
            return JsonRpcResponse::error(id, -32000, format!("Search failed: {}", error))
        }
    };
    let format = args
        .get("format")
        .and_then(|value| value.as_str())
        .unwrap_or("markdown");
    let text = if fetch_results {
        match search::fetch_search_results_with_policy(results, fetch_count, timeout, Some(policy))
            .await
        {
            Ok(results) => match format {
                "json" => serde_json::to_string_pretty(&results).unwrap_or_default(),
                "markdown" => search::format_fetched_results_markdown(&results),
                "html" => search::format_fetched_results_html(&results),
                "text" => search::format_fetched_results_text(&results),
                _ => return JsonRpcResponse::error(id, -32602, "Unsupported output format"),
            },
            Err(error) => {
                return JsonRpcResponse::error(id, -32000, format!("Fetch failed: {}", error))
            }
        }
    } else {
        match format {
            "json" => serde_json::to_string_pretty(&results).unwrap_or_default(),
            "markdown" => search::format_results_markdown(&results),
            "html" => search::format_results_html(&results),
            "text" => search::format_results_text(&results),
            _ => return JsonRpcResponse::error(id, -32602, "Unsupported output format"),
        }
    };
    JsonRpcResponse::success(
        id,
        serde_json::json!({"content": [{"type": "text", "text": text}]}),
    )
}

async fn handle_browser_action(
    id: serde_json::Value,
    args: &serde_json::Value,
    connections: &ConnectionManager,
    policy: &PolicyEngine,
) -> JsonRpcResponse {
    if !connections.has_connections().await {
        return JsonRpcResponse::error(
            id,
            -32000,
            "No browser extension connected. Start the server with `exfetch serve` and connect the Chrome extension first.",
        );
    }

    // Extract the action name — this maps directly to a bridge command name
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => {
            return JsonRpcResponse::error(id, -32602, "Missing required parameter: action");
        }
    };

    if args.get("full_page").and_then(|value| value.as_bool()) == Some(true) {
        return JsonRpcResponse::error(
            id,
            -32602,
            "Full-page screenshots are unavailable; the extension captures the visible viewport",
        );
    }
    let action_type: BrowserActionType = match serde_json::from_value(serde_json::json!(action)) {
        Ok(action) => action,
        Err(_) => return JsonRpcResponse::error(id, -32602, "Unknown browser action"),
    };
    if let PolicyVerdict::Block(reason) = policy.check_action(&action_type) {
        return JsonRpcResponse::error(id, -32000, reason);
    }

    if action_type == BrowserActionType::GetCookies {
        let Some(url) = args.get("url").and_then(|value| value.as_str()) else {
            return JsonRpcResponse::error(id, -32602, "Cookie reading requires an explicit URL");
        };
        if let PolicyVerdict::Block(reason) = policy.check_cookie_url(url) {
            return JsonRpcResponse::error(id, -32000, reason);
        }
    } else if policy.has_domain_restrictions()
        && (matches!(
            action_type,
            BrowserActionType::SwitchTab | BrowserActionType::ListTabs
        ) || args.get("url").and_then(|value| value.as_str()).is_none())
    {
        return JsonRpcResponse::error(id, -32602, "Domain-restricted browser actions require an explicit URL; active-tab and tab-id actions are unavailable under this policy");
    }

    // Build bridge command params from the MCP arguments.
    // The bridge command name matches the action name.
    // Pass through all relevant parameters.
    let bridge_params = serde_json::json!({
        "selector": args.get("selector"),
        "text": args.get("text"),
        "url": args.get("url"),
        "tab_id": args.get("tab_id"),
        "code": args.get("code"),
        "full_page": args.get("full_page"),
        "domain": args.get("domain"),
        "blocked_domains": policy.blocked_domains,
        "allowed_domains": policy.allowed_domains,
    });

    match connections.send_command(action, bridge_params).await {
        Ok(result) => {
            // Check if the extension reported an error
            if let Some(false) = result.get("success").and_then(|v| v.as_bool()) {
                let err_msg = result
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Extension returned an error");
                return JsonRpcResponse::error(id, -32000, err_msg);
            }

            JsonRpcResponse::success(
                id,
                serde_json::json!({
                    "content": [{
                        "type": "text",
                        "text": serde_json::to_string(&result).unwrap_or_default()
                    }]
                }),
            )
        }
        Err(e) => JsonRpcResponse::error(id, -32000, format!("Bridge command failed: {}", e)),
    }
}

async fn handle_connection_status(
    id: serde_json::Value,
    connections: &ConnectionManager,
) -> JsonRpcResponse {
    let connected = connections.has_connections().await;
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "content": [{
                "type": "text",
                "text": serde_json::json!({
                    "extension_connected": connected,
                    "version": env!("CARGO_PKG_VERSION")
                }).to_string()
            }]
        }),
    )
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::bridge::connection::ExtensionConnection;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn http_transport_initializes_and_rejects_unexpected_hosts() {
        use tower::ServiceExt;
        let (tx, _) = broadcast::channel(8);
        let app = sse_router(SseState {
            connections: ConnectionManager::new(),
            policy: Arc::new(PolicyEngine::new()),
            tx,
        });
        let request = |host| {
            axum::http::Request::builder()
                .method("POST")
                .uri("/mcp")
                .header("host", host)
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
                ))
                .unwrap()
        };
        let response = app
            .clone()
            .oneshot(request("127.0.0.1:9876"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = axum::body::to_bytes(response.into_body(), 10000)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&body)
            .unwrap()
            .contains("protocolVersion"));
        assert_eq!(
            app.oneshot(request("attacker.example"))
                .await
                .unwrap()
                .status(),
            403
        );
    }

    #[tokio::test]
    async fn policy_blocks_execute_js_before_the_extension_receives_it() {
        let connections = ConnectionManager::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        connections
            .add(ExtensionConnection {
                id: "test".into(),
                browser: "test".into(),
                profile: "test".into(),
                sender,
                connected_at: String::new(),
            })
            .await;
        let response = handle_tools_call(serde_json::json!(1), &serde_json::json!({
            "name": "browser_action", "arguments": {"action": "execute_js", "code": "document.cookie"}
        }), &connections, &PolicyEngine::new()).await;
        assert!(response.error.unwrap().message.contains("not allowed"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn restricted_actions_cannot_bypass_policy_with_active_tabs_or_fake_urls() {
        let connections = ConnectionManager::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        connections
            .add(ExtensionConnection {
                id: "test".into(),
                browser: "test".into(),
                profile: "test".into(),
                sender,
                connected_at: String::new(),
            })
            .await;
        let mut policy = PolicyEngine::new();
        policy.blocked_domains.push("blocked.example".into());
        for args in [
            serde_json::json!({"action": "read_dom"}),
            serde_json::json!({"action": "switch_tab", "tab_id": 123, "url": "https://allowed.example"}),
        ] {
            let response =
                handle_browser_action(serde_json::json!(1), &args, &connections, &policy).await;
            assert!(response
                .error
                .unwrap()
                .message
                .contains("Domain-restricted"));
        }
        policy.allow_cookie_read = true;
        let response = handle_browser_action(
            serde_json::json!(1),
            &serde_json::json!({"action": "get_cookies"}),
            &connections,
            &policy,
        )
        .await;
        assert!(response.error.unwrap().message.contains("explicit URL"));
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn explicit_browser_fetch_does_not_silently_use_http() {
        let response = handle_fetch_page(
            serde_json::json!(1),
            &serde_json::json!({
                "url": "https://example.com", "use_browser": true
            }),
            &ConnectionManager::new(),
            &PolicyEngine::new(),
        )
        .await;
        assert!(response
            .error
            .unwrap()
            .message
            .contains("requires a connected extension"));
    }

    #[tokio::test]
    async fn blocked_domain_is_rejected_before_fetching() {
        let mut policy = PolicyEngine::new();
        policy.blocked_domains.push("example.com".into());
        let response = handle_tools_call(
            serde_json::json!(1),
            &serde_json::json!({
                "name": "fetch_page", "arguments": {"url": "https://example.com"}
            }),
            &ConnectionManager::new(),
            &policy,
        )
        .await;
        assert!(response
            .error
            .unwrap()
            .message
            .contains("blocked by policy"));
    }

    #[tokio::test]
    async fn browser_fetch_extracts_requested_elements() {
        let connections = ConnectionManager::new();
        let (sender, mut receiver) = mpsc::unbounded_channel();
        connections
            .add(ExtensionConnection {
                id: "test".into(),
                browser: "test".into(),
                profile: "test".into(),
                sender,
                connected_at: String::new(),
            })
            .await;
        let pending = connections.pending().clone();
        tokio::spawn(async move {
            let message: serde_json::Value =
                serde_json::from_str(&receiver.recv().await.unwrap()).unwrap();
            assert_eq!(message["command"], "fetch_page");
            pending.complete(message["id"].as_str().unwrap(), serde_json::json!({
                "success": true, "html": "<h1>Heading</h1><p>Body</p>", "url": "https://example.com"
            })).await;
        });
        let response = handle_fetch_page(serde_json::json!(1), &serde_json::json!({
            "url": "https://example.com", "use_browser": true, "selector": "h1", "format": "html"
        }), &connections, &PolicyEngine::new()).await;
        assert_eq!(
            response.result.unwrap()["content"][0]["text"],
            "<h1>Heading</h1>"
        );
    }
}
