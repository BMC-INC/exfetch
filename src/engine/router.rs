use super::command::Command;
use crate::bridge::connection::ConnectionManager;

/// Where a command should be routed.
#[derive(Debug, PartialEq)]
pub enum Route {
    /// Fulfil via direct HTTP request.
    Http,
    /// Fulfil via browser extension WebSocket.
    Extension,
    /// Cannot route — return error.
    Error(String),
}

/// Routes commands to the appropriate execution backend.
pub struct Router {
    connections: ConnectionManager,
}

impl Router {
    /// Create a new router backed by the given connection manager.
    pub fn new(connections: ConnectionManager) -> Self {
        Self { connections }
    }

    /// Determine the route for a command.
    pub async fn determine_route(&self, command: &Command) -> Route {
        match command {
            Command::FetchPage { use_browser, .. } => {
                if *use_browser && self.connections.has_connections().await {
                    Route::Extension
                } else {
                    Route::Http
                }
            }
            Command::BrowserAction { .. } => {
                if self.connections.has_connections().await {
                    Route::Extension
                } else {
                    Route::Error("browser action requires an extension connection".into())
                }
            }
            Command::SearchWeb { .. } | Command::ConnectionStatus => Route::Http,
        }
    }
}
