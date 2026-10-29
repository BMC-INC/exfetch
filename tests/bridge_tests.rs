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

#[tokio::test]
async fn test_cli_browser_fetch_uses_authenticated_bridge_and_selector() {
    use exfetch::bridge::protocol::BridgeMessage;
    let (port, token) = ws_server::start_test_server().await;
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}", port))
        .await
        .unwrap();
    ws.send(Message::Text(
        serde_json::json!({"token": token, "browser": "test"}).to_string(),
    ))
    .await
    .unwrap();
    ws.next().await.unwrap().unwrap();
    let extension = tokio::spawn(async move {
        let request = ws.next().await.unwrap().unwrap().into_text().unwrap();
        let request: BridgeMessage = serde_json::from_str(&request).unwrap();
        assert_eq!(request.command, "fetch_page");
        assert_eq!(request.params["url"], "https://example.com");
        let reply = BridgeMessage::new_response(
            request.id,
            "fetch_page",
            serde_json::json!({
                "success": true, "html": "<h1>Rendered page</h1><p>Other content</p>", "url": "https://example.com/"
            }),
        );
        ws.send(Message::Text(serde_json::to_string(&reply).unwrap()))
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    });
    let directory = std::env::temp_dir().join(format!("exfetch-cli-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join("port"), port.to_string()).unwrap();
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_exfetch"))
        .env("EXFETCH_CONFIG_DIR", &directory)
        .args([
            "fetch",
            "https://example.com",
            "--browser",
            "--selector",
            "h1",
            "--raw",
        ])
        .output()
        .await
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    extension.await.unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        "<h1>Rendered page</h1>"
    );
}

#[tokio::test]
async fn test_bridge_fetch_rejects_missing_auth_and_rebinding_host() {
    let (port, _) = ws_server::start_test_server().await;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("http://127.0.0.1:{}/fetch", port))
        .header("content-type", "application/json")
        .body(r#"{"url":"https://example.com"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let response = client
        .get(format!("http://127.0.0.1:{}/token", port))
        .header("host", "attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn test_cli_loads_policy_configuration_and_rejects_invalid_config() {
    let directory =
        std::env::temp_dir().join(format!("exfetch-config-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("config.toml"),
        "[policy]\nblocked_domains = [\"example.com\"]\n",
    )
    .unwrap();
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_exfetch"))
        .env("EXFETCH_CONFIG_DIR", &directory)
        .args(["fetch", "https://example.com"])
        .output()
        .await
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("blocked by policy"));
    std::fs::write(directory.join("config.toml"), "invalid = [").unwrap();
    let result = tokio::process::Command::new(env!("CARGO_BIN_EXE_exfetch"))
        .env("EXFETCH_CONFIG_DIR", &directory)
        .args(["fetch", "https://example.com"])
        .output()
        .await
        .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("Invalid configuration"));
}
