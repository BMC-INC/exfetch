use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The type of a bridge message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Request,
    Response,
    Event,
}

/// A message exchanged between the CLI and the browser extension over WebSocket.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    pub id: String,
    pub msg_type: MessageType,
    pub command: String,
    pub params: serde_json::Value,
    pub timestamp: String,
}

impl BridgeMessage {
    /// Create a new request message.
    pub fn new_request(command: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            msg_type: MessageType::Request,
            command: command.into(),
            params,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Create a response message for a given request id.
    pub fn new_response(
        id: impl Into<String>,
        command: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            msg_type: MessageType::Response,
            command: command.into(),
            params,
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}
