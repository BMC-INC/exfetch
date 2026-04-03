use std::net::SocketAddr;

use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use rand::Rng;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

use super::connection::{ConnectionManager, ExtensionConnection};
use super::protocol::{BridgeMessage, MessageType};

/// Generate a cryptographically random authentication token (32 random bytes, hex-encoded).
pub fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    let mut bytes = [0u8; 32];
    rng.fill(&mut bytes);
    hex::encode(bytes)
}

/// Start the WebSocket server.
///
/// Binds to `127.0.0.1` and tries ports in the range `port..port+10`.
/// Writes the actual port to `~/.config/exfetch/port`.
/// Returns the actual port the server bound to.
pub async fn start(port: u16, token: String, connections: ConnectionManager) -> Result<u16> {
    let listener = bind_with_fallback(port).await?;
    let actual_port = listener.local_addr()?.port();

    // Write port file
    write_port_file(actual_port)?;

    // Spawn accept loop
    let tok = token.clone();
    let conns = connections.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let tok = tok.clone();
                    let conns = conns.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_connection(stream, addr, tok, conns).await {
                            eprintln!("[exfetch] ws connection error from {}: {}", addr, e);
                        }
                    });
                }
                Err(e) => {
                    eprintln!("[exfetch] ws accept error: {}", e);
                }
            }
        }
    });

    Ok(actual_port)
}

/// Start a test server on a random port. Returns (port, token).
pub async fn start_test_server() -> (u16, String) {
    let token = generate_token();
    let connections = ConnectionManager::new();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("failed to bind test server");
    let port = listener.local_addr().unwrap().port();

    let tok = token.clone();
    let conns = connections.clone();
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let tok = tok.clone();
                    let conns = conns.clone();
                    tokio::spawn(async move {
                        let _ = handle_connection(stream, addr, tok, conns).await;
                    });
                }
                Err(_) => break,
            }
        }
    });

    (port, token)
}

async fn bind_with_fallback(base_port: u16) -> Result<TcpListener> {
    for offset in 0..10u16 {
        let port = base_port.saturating_add(offset);
        let addr: SocketAddr = ([127, 0, 0, 1], port).into();
        match TcpListener::bind(addr).await {
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

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    token: String,
    connections: ConnectionManager,
) -> Result<()> {
    let ws_stream = tokio_tungstenite::accept_async(stream).await?;
    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    // Wait up to 5 seconds for auth message
    let auth_result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        ws_receiver.next(),
    )
    .await;

    let auth_msg = match auth_result {
        Ok(Some(Ok(Message::Text(text)))) => text,
        _ => {
            let reject = serde_json::json!({
                "status": "error",
                "reason": "auth timeout or invalid message"
            });
            let _ = ws_sender.send(Message::Text(reject.to_string().into())).await;
            let _ = ws_sender.close().await;
            return Err(anyhow::anyhow!("auth timeout from {}", addr));
        }
    };

    // Parse and validate auth
    let auth: serde_json::Value =
        serde_json::from_str(&auth_msg).unwrap_or(serde_json::Value::Null);

    let provided_token = auth.get("token").and_then(|t| t.as_str()).unwrap_or("");
    if provided_token != token {
        let reject = serde_json::json!({
            "status": "rejected",
            "reason": "invalid token"
        });
        let _ = ws_sender.send(Message::Text(reject.to_string().into())).await;
        let _ = ws_sender.close().await;
        return Err(anyhow::anyhow!("invalid token from {}", addr));
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
    ws_sender.send(Message::Text(ack.to_string().into())).await?;

    // Bidirectional message forwarding
    loop {
        tokio::select! {
            // Messages from the extension -> CLI
            incoming = ws_receiver.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        // Try to parse as BridgeMessage and route responses
                        if let Ok(msg) = serde_json::from_str::<BridgeMessage>(&text) {
                            if msg.msg_type == MessageType::Response {
                                let completed = connections.pending().complete(&msg.id, msg.params.clone()).await;
                                if !completed {
                                    eprintln!(
                                        "[exfetch] received orphan response from {} for command '{}' (id={})",
                                        browser, msg.command, msg.id
                                    );
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(e)) => {
                        eprintln!("[exfetch] ws read error: {}", e);
                        break;
                    }
                    _ => {} // ping/pong/binary — ignore
                }
            }
            // Messages from CLI -> extension
            outgoing = rx.recv() => {
                match outgoing {
                    Some(text) => {
                        if ws_sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    connections.remove(&conn_id).await;
    Ok(())
}
