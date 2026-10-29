use std::net::SocketAddr;
use std::sync::Arc;

use crate::engine::policy::{PolicyEngine, PolicyVerdict};
use anyhow::Result;
use axum::extract::ws::{Message as AxumMessage, WebSocket};
use axum::extract::Request;
use axum::extract::{State, WebSocketUpgrade};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::{self, Next};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::connection::{ConnectionManager, ExtensionConnection};
use super::protocol::{BridgeMessage, MessageType};

/// Shared state for the hybrid HTTP+WebSocket server.
#[derive(Clone)]
struct ServerState {
    token: String,
    connections: ConnectionManager,
    policy: Arc<PolicyEngine>,
}

/// Generate a cryptographically random authentication token (32 random bytes, hex-encoded).
pub fn generate_token() -> String {
    let mut rng = rand::rng();
    let mut bytes = [0u8; 32];
    rng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

/// Start the hybrid HTTP + WebSocket server.
///
/// Serves `/health` and `/token` as HTTP endpoints, and upgrades
/// WebSocket connections on `/ws` and `/` (fallback).
/// Binds to `127.0.0.1` and tries ports in the range `port..port+10`.
/// Writes the actual port to `~/.config/exfetch/port`.
/// Returns the actual port the server bound to.
pub async fn start(port: u16, token: String, connections: ConnectionManager) -> Result<u16> {
    start_with_policy(port, token, connections, Arc::new(PolicyEngine::new())).await
}

pub async fn start_with_policy(
    port: u16,
    token: String,
    connections: ConnectionManager,
    policy: Arc<PolicyEngine>,
) -> Result<u16> {
    let state = Arc::new(ServerState {
        token: token.clone(),
        connections: connections.clone(),
        policy,
    });

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/token", get(token_handler))
        .route("/fetch", post(fetch_handler))
        .route("/ws", get(ws_handler))
        .fallback(get(ws_handler))
        .with_state(state)
        .layer(middleware::from_fn(require_loopback_host));

    let listener = bind_with_fallback(port).await?;
    let actual_port = listener.local_addr()?.port();
    if port != 0 {
        write_port_file(actual_port)?;
    }

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[exfetch] server error: {}", e);
        }
    });

    Ok(actual_port)
}

/// Reject unexpected hosts to prevent DNS rebinding into the local bridge.
pub async fn require_loopback_host(
    request: Request,
    next: Next,
) -> Result<axum::response::Response, StatusCode> {
    let host = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let hostname = host.split(':').next().unwrap_or("");
    if hostname != "127.0.0.1" && hostname != "localhost" {
        return Err(StatusCode::FORBIDDEN);
    }
    if let Some(origin) = request
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
    {
        if !origin.starts_with("chrome-extension://")
            && !origin.starts_with("safari-web-extension://")
        {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(request).await)
}

async fn fetch_handler(
    State(state): State<Arc<ServerState>>,
    headers: HeaderMap,
    Json(mut params): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let expected = format!("Bearer {}", state.token);
    if headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(expected.as_str())
    {
        return Err((StatusCode::UNAUTHORIZED, "Invalid bridge token".into()));
    }
    let url = params
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "Missing URL".to_string()))?;
    if let PolicyVerdict::Block(reason) = state.policy.check_domain(url) {
        return Err((StatusCode::FORBIDDEN, reason));
    }
    if let PolicyVerdict::Block(reason) = state.policy.check_rate_limit() {
        return Err((StatusCode::TOO_MANY_REQUESTS, reason));
    }
    state.policy.record_request();
    let settings = crate::config::load()
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    params["reuse_strategy"] = serde_json::json!(settings.tabs.reuse_strategy);
    params["open_in_background"] = serde_json::json!(settings.tabs.open_in_background);
    params["blocked_domains"] = serde_json::json!(state.policy.blocked_domains);
    params["allowed_domains"] = serde_json::json!(state.policy.allowed_domains);
    let result = state
        .connections
        .send_command("fetch_page", params)
        .await
        .map_err(|error| (StatusCode::BAD_GATEWAY, error.to_string()))?;
    if let Some(final_url) = result.get("url").and_then(|value| value.as_str()) {
        if let PolicyVerdict::Block(reason) = state.policy.check_domain(final_url) {
            return Err((StatusCode::FORBIDDEN, reason));
        }
    }
    Ok(Json(result))
}

/// Fetch rendered HTML through the running local bridge and its extension.
pub async fn fetch_from_running_bridge(
    url: &str,
    timeout: std::time::Duration,
) -> Result<crate::fetch::http::FetchResponse> {
    let port_path = crate::config::config_dir().join("port");
    let port: u16 = std::fs::read_to_string(&port_path)
        .map_err(|_| {
            anyhow::anyhow!(
                "Start `exfetch serve` and connect the browser extension before using --browser"
            )
        })?
        .trim()
        .parse()?;
    fetch_from_bridge_port(port, url, timeout).await
}

pub async fn fetch_from_bridge_port(
    port: u16,
    url: &str,
    timeout: std::time::Duration,
) -> Result<crate::fetch::http::FetchResponse> {
    let start = std::time::Instant::now();
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(timeout)
        .build()?;
    let base = format!("http://127.0.0.1:{}", port);
    let token: serde_json::Value = serde_json::from_str(
        &client
            .get(format!("{}/token", base))
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?,
    )?;
    let token = token
        .get("token")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid bridge token response"))?;
    let response = client
        .post(format!("{}/fetch", base))
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(serde_json::json!({"url": url}).to_string())
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("Browser bridge: {}", response.text().await?);
    }
    let result: serde_json::Value = serde_json::from_str(&response.text().await?)?;
    if result.get("success").and_then(|value| value.as_bool()) == Some(false) {
        anyhow::bail!(
            "Browser fetch: {}",
            result
                .get("error")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown error")
        );
    }
    let body = result
        .get("html")
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow::anyhow!("Browser returned no HTML"))?
        .to_string();
    Ok(crate::fetch::http::FetchResponse {
        body,
        status: 200,
        content_type: "text/html".into(),
        headers: Vec::new(),
        final_url: result
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or(url)
            .to_string(),
        fetch_time_ms: start.elapsed().as_millis() as u64,
    })
}

async fn health_handler() -> impl IntoResponse {
    Json(
        serde_json::json!({"status": "ok", "service": "exfetch", "version": env!("CARGO_PKG_VERSION")}),
    )
}

async fn token_handler(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    Json(serde_json::json!({"token": state.token}))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<ServerState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, state))
}

/// Start a test server on a random port. Returns (port, token).
pub async fn start_test_server() -> (u16, String) {
    let token = generate_token();
    let connections = ConnectionManager::new();
    let port = start(0, token.clone(), connections).await.unwrap();
    (port, token)
}

async fn bind_with_fallback(base_port: u16) -> Result<tokio::net::TcpListener> {
    for offset in 0..10u16 {
        let port = base_port.saturating_add(offset);
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        match tokio::net::TcpListener::bind(addr).await {
            Ok(listener) => return Ok(listener),
            Err(_) if offset < 9 => continue,
            Err(e) => return Err(e.into()),
        }
    }
    unreachable!()
}

fn write_port_file(port: u16) -> Result<()> {
    let config_dir = crate::config::config_dir();
    std::fs::create_dir_all(&config_dir)?;
    let port_file = config_dir.join("port");
    std::fs::write(&port_file, port.to_string())?;
    Ok(())
}

async fn handle_ws_connection(socket: WebSocket, state: Arc<ServerState>) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let connections = &state.connections;
    let token = &state.token;

    // Wait up to 5 seconds for auth message
    let auth_result =
        tokio::time::timeout(std::time::Duration::from_secs(5), ws_receiver.next()).await;

    let auth_msg = match auth_result {
        Ok(Some(Ok(AxumMessage::Text(text)))) => text.to_string(),
        _ => {
            let reject = serde_json::json!({
                "status": "error",
                "reason": "auth timeout or invalid message"
            });
            let _ = ws_sender
                .send(AxumMessage::Text(reject.to_string().into()))
                .await;
            let _ = ws_sender.close().await;
            return;
        }
    };

    // Parse and validate auth
    let auth: serde_json::Value =
        serde_json::from_str(&auth_msg).unwrap_or(serde_json::Value::Null);

    let provided_token = auth.get("token").and_then(|t| t.as_str()).unwrap_or("");
    if provided_token != *token {
        let reject = serde_json::json!({
            "status": "rejected",
            "reason": "invalid token"
        });
        let _ = ws_sender
            .send(AxumMessage::Text(reject.to_string().into()))
            .await;
        let _ = ws_sender.close().await;
        return;
    }

    // Auth succeeded — register connection
    let conn_id = Uuid::new_v4().to_string();
    let browser = auth
        .get("browser")
        .and_then(|b| b.as_str())
        .unwrap_or("unknown")
        .to_string();
    let profile = auth
        .get("profile")
        .and_then(|p| p.as_str())
        .unwrap_or("default")
        .to_string();

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    let conn = ExtensionConnection {
        id: conn_id.clone(),
        browser: browser.clone(),
        profile,
        sender: tx,
        connected_at: chrono::Utc::now().to_rfc3339(),
    };
    connections.add(conn).await;

    // Send acknowledgement
    let ack = serde_json::json!({
        "status": "authenticated",
        "connection_id": conn_id,
    });
    let _ = ws_sender
        .send(AxumMessage::Text(ack.to_string().into()))
        .await;

    eprintln!("[exfetch] extension connected: {} ({})", browser, conn_id);

    // Bidirectional message forwarding
    loop {
        tokio::select! {
            incoming = ws_receiver.next() => {
                match incoming {
                    Some(Ok(AxumMessage::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<BridgeMessage>(text.as_str()) {
                            if msg.msg_type == MessageType::Response {
                                let completed = connections.pending().complete(&msg.id, msg.params.clone()).await;
                                if !completed {
                                    eprintln!(
                                        "[exfetch] orphan response from {} for '{}' (id={})",
                                        browser, msg.command, msg.id
                                    );
                                }
                            }
                        }
                    }
                    Some(Ok(AxumMessage::Close(_))) | None => break,
                    Some(Err(e)) => {
                        eprintln!("[exfetch] ws read error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
            outgoing = rx.recv() => {
                match outgoing {
                    Some(text) => {
                        if ws_sender.send(AxumMessage::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    connections.remove(&conn_id).await;
    eprintln!("[exfetch] extension disconnected: {}", conn_id);
}
