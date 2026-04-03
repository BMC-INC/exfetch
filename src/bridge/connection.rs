use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, RwLock};

use super::protocol::BridgeMessage;

/// Represents a single connected browser extension.
pub struct ExtensionConnection {
    pub id: String,
    pub browser: String,
    pub profile: String,
    pub sender: mpsc::UnboundedSender<String>,
    pub connected_at: String,
}

/// Tracks in-flight requests awaiting responses from the extension.
#[derive(Clone, Default)]
pub struct PendingRequests {
    inner: Arc<RwLock<HashMap<String, oneshot::Sender<serde_json::Value>>>>,
}

impl PendingRequests {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a pending request. Returns a receiver that will deliver the
    /// response params when the extension replies.
    pub async fn register(&self, id: String) -> oneshot::Receiver<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        let mut map = self.inner.write().await;
        map.insert(id, tx);
        rx
    }

    /// Complete a pending request by sending the response value to the waiter.
    /// Returns `true` if a waiter was found, `false` otherwise.
    pub async fn complete(&self, id: &str, result: serde_json::Value) -> bool {
        let mut map = self.inner.write().await;
        if let Some(tx) = map.remove(id) {
            // Ignore send errors (the receiver may have been dropped due to timeout)
            let _ = tx.send(result);
            true
        } else {
            false
        }
    }
}

/// Thread-safe manager for active extension connections.
#[derive(Clone)]
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, ExtensionConnection>>>,
    pending: PendingRequests,
}

impl ConnectionManager {
    /// Create a new, empty connection manager.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            pending: PendingRequests::new(),
        }
    }

    /// Get a reference to the pending requests tracker.
    pub fn pending(&self) -> &PendingRequests {
        &self.pending
    }

    /// Register a new connection.
    pub async fn add(&self, conn: ExtensionConnection) {
        let mut map = self.connections.write().await;
        map.insert(conn.id.clone(), conn);
    }

    /// Remove a connection by id.
    pub async fn remove(&self, id: &str) {
        let mut map = self.connections.write().await;
        map.remove(id);
    }

    /// Returns true if at least one extension is connected.
    pub async fn has_connections(&self) -> bool {
        let map = self.connections.read().await;
        !map.is_empty()
    }

    /// Get the id of the first (default) connection, if any.
    pub async fn get_default(&self) -> Option<String> {
        let map = self.connections.read().await;
        map.keys().next().cloned()
    }

    /// Send a text message to the default (first) connection.
    /// Returns Ok(()) if sent, Err if no connections exist.
    pub async fn send_to_default(&self, message: &str) -> anyhow::Result<()> {
        let map = self.connections.read().await;
        if let Some(conn) = map.values().next() {
            conn.sender
                .send(message.to_string())
                .map_err(|e| anyhow::anyhow!("failed to send to extension: {}", e))?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("no extension connections available"))
        }
    }

    /// Send a command to the default extension and wait for a response.
    ///
    /// Creates a `BridgeMessage::new_request`, registers it in `pending`,
    /// sends it over the WebSocket channel, then awaits the response with a
    /// 10-second timeout.
    pub async fn send_command(
        &self,
        command: impl Into<String>,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let msg = BridgeMessage::new_request(command, params);
        let id = msg.id.clone();

        // Register before sending so we never miss the response
        let rx = self.pending.register(id.clone()).await;

        // Serialize and send
        let json = serde_json::to_string(&msg)
            .map_err(|e| anyhow::anyhow!("failed to serialize bridge message: {}", e))?;
        self.send_to_default(&json).await?;

        // Wait for response with timeout
        match tokio::time::timeout(Duration::from_secs(10), rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => Err(anyhow::anyhow!(
                "bridge response channel closed for request {}",
                id
            )),
            Err(_) => {
                // Clean up the pending entry on timeout
                self.pending.complete(&id, serde_json::Value::Null).await;
                Err(anyhow::anyhow!(
                    "bridge command timed out after 10s (request {})",
                    id
                ))
            }
        }
    }
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}
