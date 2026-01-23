//! WebSocket server for real-time updates

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use parking_lot::RwLock;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::events::{RealtimeEvent, SubscriptionFilter};

/// Connection ID
pub type ConnectionId = String;

/// Manages WebSocket connections
pub struct RealtimeManager {
    /// Broadcast channel for events
    tx: broadcast::Sender<RealtimeEvent>,
    /// Connected clients with their filters
    clients: Arc<RwLock<HashMap<ConnectionId, SubscriptionFilter>>>,
}

impl RealtimeManager {
    /// Create a new realtime manager
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            tx,
            clients: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Broadcast an event to all matching clients
    pub fn broadcast(&self, event: RealtimeEvent) {
        // The broadcast channel handles delivery to all subscribers
        let _ = self.tx.send(event);
    }

    /// Get number of connected clients
    pub fn client_count(&self) -> usize {
        self.clients.read().len()
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<RealtimeEvent> {
        self.tx.subscribe()
    }

    /// Register a new client
    pub fn register_client(&self, id: ConnectionId, filter: SubscriptionFilter) {
        self.clients.write().insert(id, filter);
    }

    /// Unregister a client
    pub fn unregister_client(&self, id: &str) {
        self.clients.write().remove(id);
    }

    /// Get client filter
    pub fn get_client_filter(&self, id: &str) -> Option<SubscriptionFilter> {
        self.clients.read().get(id).cloned()
    }
}

impl Default for RealtimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RealtimeManager {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            clients: self.clients.clone(),
        }
    }
}

/// WebSocket server
pub struct RealtimeServer {
    manager: RealtimeManager,
    addr: SocketAddr,
}

impl RealtimeServer {
    /// Create a new WebSocket server
    pub fn new(manager: RealtimeManager, port: u16) -> Self {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        Self { manager, addr }
    }

    /// Build the router
    pub fn router(manager: RealtimeManager) -> Router {
        Router::new()
            .route("/ws", get(ws_handler))
            .route("/health", get(health_handler))
            .with_state(manager)
    }

    /// Start the server
    pub async fn start(self) -> std::io::Result<()> {
        let app = Self::router(self.manager);

        tracing::info!("WebSocket server listening on {}", self.addr);

        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

/// Health check endpoint
async fn health_handler(State(manager): State<RealtimeManager>) -> impl IntoResponse {
    serde_json::json!({
        "status": "ok",
        "clients": manager.client_count(),
    })
    .to_string()
}

/// WebSocket upgrade handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(manager): State<RealtimeManager>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, manager))
}

/// Handle an individual WebSocket connection
async fn handle_socket(socket: WebSocket, manager: RealtimeManager) {
    let connection_id = Uuid::new_v4().to_string();
    let filter = SubscriptionFilter::default();

    manager.register_client(connection_id.clone(), filter.clone());
    tracing::info!("Client connected: {}", connection_id);

    let (mut sender, mut receiver) = socket.split();
    let mut rx = manager.subscribe();

    // Task to forward events to client
    let conn_id = connection_id.clone();
    let mgr = manager.clone();
    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            // Check if event matches client's filter
            if let Some(filter) = mgr.get_client_filter(&conn_id) {
                if filter.matches(&event) {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    if sender.send(Message::Text(json)).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Task to handle incoming messages from client
    let conn_id = connection_id.clone();
    let mgr = manager.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Text(text) => {
                    // Try to parse as filter update
                    if let Ok(new_filter) = serde_json::from_str::<SubscriptionFilter>(&text) {
                        mgr.register_client(conn_id.clone(), new_filter);
                        tracing::debug!("Updated filter for client {}", conn_id);
                    }
                }
                Message::Close(_) => {
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for either task to finish
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    manager.unregister_client(&connection_id);
    tracing::info!("Client disconnected: {}", connection_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_realtime_manager() {
        let manager = RealtimeManager::new();
        assert_eq!(manager.client_count(), 0);

        manager.register_client("test".to_string(), SubscriptionFilter::default());
        assert_eq!(manager.client_count(), 1);

        manager.unregister_client("test");
        assert_eq!(manager.client_count(), 0);
    }

    #[test]
    fn test_subscription_filter() {
        let filter = SubscriptionFilter {
            event_types: Some(vec![super::super::events::EventType::MemoryCreated]),
            memory_ids: None,
            tags: None,
        };

        let event = RealtimeEvent::memory_created(1, "test".to_string());
        assert!(filter.matches(&event));

        let event = RealtimeEvent::memory_deleted(1);
        assert!(!filter.matches(&event));
    }
}
