use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use exfetch::bridge::ws_server;

#[tokio::test]
async fn test_ws_server_accepts_authenticated_connection() {
    let (port, token) = ws_server::start_test_server().await;

    let url = format!("ws://127.0.0.1:{}", port);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("failed to connect");

    // Send valid auth
    let auth = serde_json::json!({
        "token": token,
        "browser": "test",
        "profile": "default"
    });
    ws.send(Message::Text(auth.to_string()))
        .await
        .expect("failed to send auth");

    // Read acknowledgement
    let msg = ws
        .next()
        .await
        .expect("expected a message")
        .expect("message should be ok");

    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => panic!("expected text message, got: {:?}", other),
    };

    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        parsed.get("status").and_then(|s| s.as_str()),
        Some("authenticated"),
        "server should acknowledge valid auth, got: {}",
        text
    );
    assert!(
        parsed.get("connection_id").is_some(),
        "response should include connection_id"
    );
}

#[tokio::test]
async fn test_ws_server_rejects_bad_token() {
    let (port, _token) = ws_server::start_test_server().await;

    let url = format!("ws://127.0.0.1:{}", port);
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .expect("failed to connect");

    // Send invalid auth
    let auth = serde_json::json!({
        "token": "wrong-token-value",
        "browser": "test",
    });
    ws.send(Message::Text(auth.to_string()))
        .await
        .expect("failed to send auth");

    // Read rejection
    let msg = ws
        .next()
        .await
        .expect("expected a message")
        .expect("message should be ok");

    let text = match msg {
        Message::Text(t) => t.to_string(),
        other => panic!("expected text message, got: {:?}", other),
    };

    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        parsed.get("status").and_then(|s| s.as_str()),
        Some("rejected"),
        "server should reject bad token, got: {}",
        text
    );
}
