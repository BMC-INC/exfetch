use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};

/// Represents a single connected browser extension.
pub struct ExtensionConnection {
    pub id: String,
    pub browser: String,
    pub profile: String,
    pub sender: mpsc::UnboundedSender<String>,
    pub connected_at: String,
}

/// Thread-safe manager for active extension connections.
#[derive(Clone)]
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, ExtensionConnection>>>,
}

impl ConnectionManager {
    /// Create a new, empty connection manager.
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
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
}

impl Default for ConnectionManager {
    fn default() -> Self {
        Self::new()
    }
}
