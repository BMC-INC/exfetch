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
pub async fn run_stdio(
    connections: ConnectionManager,
    policy: Arc<PolicyEngine>,
) -> Result<()> {
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
        .route("/sse", get(sse_handler))
        .route("/message", post(message_handler))
        .with_state(state);

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("MCP SSE server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// SSE endpoint: clients connect here to receive JSON-RPC responses as
/// server-sent events.
async fn sse_handler(
    State(state): State<SseState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.tx.subscribe();

    let stream = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|msg| async move {
        match msg {
            Ok(data) => Some(Ok(Event::default().data(data))),
            Err(_) => None,
        }
    });

    Sse::new(stream)
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
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

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

async fn handle_fetch_page(
    id: serde_json::Value,
    args: &serde_json::Value,
) -> JsonRpcResponse {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(u) => u,
        None => return JsonRpcResponse::error(id, -32602, "Missing required parameter: url"),
    };

    let format = args
        .get("format")
        .and_then(|v| v.as_str())
        .unwrap_or("markdown");

    let max_length = args.get("max_length").and_then(|v| v.as_u64()).map(|n| n as usize);

    let timeout = Duration::from_secs(15);

    match fetch_url(url, timeout, "exfetch/0.1").await {
        Ok(resp) => {
            let content = match format {
                "html" | "raw" => {
                    output::text::format_raw(&resp.body, max_length)
                }
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
    _args: &serde_json::Value,
    connections: &ConnectionManager,
) -> JsonRpcResponse {
    if !connections.has_connections().await {
        return JsonRpcResponse::error(
            id,
            -32000,
            "No browser extension connected. Start the server with `exfetch serve` and connect the Chrome extension first.",
        );
    }

    // Bridge routing will be wired in a later task — for now, acknowledge
    // that the extension is connected but the action pipeline is not yet
    // implemented.
    JsonRpcResponse::error(
        id,
        -32000,
        "Browser action bridge routing not yet implemented",
    )
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
