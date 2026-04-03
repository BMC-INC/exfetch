use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use futures_util::stream::Stream;
use futures_util::StreamExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::broadcast;
use tracing;

use crate::bridge::connection::ConnectionManager;
use crate::engine::policy::PolicyEngine;
use crate::extract::readability;
use crate::fetch::http::fetch_url;
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

    let app = Router::new()
        .route("/", get(sse_handler.clone()).post(mcp_handler.clone()))
        .route("/sse", get(sse_handler))
        .route("/mcp", post(mcp_handler.clone()))
        .route("/message", post(message_handler).post(mcp_handler))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("MCP SSE server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
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
                .body(axum::body::Body::from(
                    format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32700,"message":"Parse error: {}"}}}}"#, e),
                ))
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
        None => return axum::http::Response::builder()
            .status(202)
            .body(axum::body::Body::empty())
            .unwrap(),
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
    _policy: &PolicyEngine,
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
        "tools/call" => handle_tools_call(id, &req.params, connections).await,
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
) -> JsonRpcResponse {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    match tool_name {
        "fetch_page" => handle_fetch_page(id, &arguments).await,
        "search_web" => handle_search_web(id, &arguments).await,
        "browser_action" => handle_browser_action(id, &arguments, connections).await,
        "connection_status" => handle_connection_status(id, connections).await,
        _ => JsonRpcResponse::error(id, -32602, format!("Unknown tool: {}", tool_name)),
    }
}

// ---------------------------------------------------------------------------
// Tool handlers
// ---------------------------------------------------------------------------

async fn handle_fetch_page(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
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

    let timeout = Duration::from_secs(15);

    match fetch_url(url, timeout, "exfetch/0.1").await {
        Ok(resp) => {
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

async fn handle_search_web(id: serde_json::Value, args: &serde_json::Value) -> JsonRpcResponse {
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

    let timeout = Duration::from_secs(15);

    if fetch_results {
        match search::search_and_fetch(query, num_results, fetch_count, timeout).await {
            Ok(results) => {
                let text = search::format_fetched_results_text(&results);
                JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": text
                        }]
                    }),
                )
            }
            Err(e) => JsonRpcResponse::error(id, -32000, format!("Search failed: {}", e)),
        }
    } else {
        match search::engine::search_ddg(query, num_results, timeout).await {
            Ok(results) => {
                let text = search::format_results_text(&results);
                JsonRpcResponse::success(
                    id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": text
                        }]
                    }),
                )
            }
            Err(e) => JsonRpcResponse::error(id, -32000, format!("Search failed: {}", e)),
        }
    }
}

async fn handle_browser_action(
    id: serde_json::Value,
    args: &serde_json::Value,
    connections: &ConnectionManager,
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
