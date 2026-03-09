//! WebSocket server for real-time updates

use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// Default maximum number of events retained in the replay ring buffer.
const DEFAULT_MAX_BUFFERED_EVENTS: usize = 500;

/// Manages WebSocket connections and SSE subscriptions.
///
/// Each event broadcast through [`RealtimeManager::broadcast`] is:
/// 1. Assigned a monotonically-increasing `seq_id`.
/// 2. Pushed into an in-memory ring buffer (capacity [`DEFAULT_MAX_BUFFERED_EVENTS`]).
/// 3. Sent over the tokio broadcast channel for live subscribers.
///
/// Clients that reconnect with a `Last-Event-Id` header can call
/// [`RealtimeManager::get_events_after`] to retrieve buffered events they missed.
pub struct RealtimeManager {
    /// Broadcast channel for live delivery
    tx: broadcast::Sender<RealtimeEvent>,
    /// Connected clients with their filters
    clients: Arc<RwLock<HashMap<ConnectionId, SubscriptionFilter>>>,
    /// Monotonically-increasing sequence counter (starts at 1)
    next_seq_id: Arc<AtomicU64>,
    /// In-memory ring buffer for replay
    buffer: Arc<RwLock<VecDeque<RealtimeEvent>>>,
    /// Maximum number of events kept in the buffer
    max_buffered_events: usize,
}

impl RealtimeManager {
    /// Create a new realtime manager with the default buffer size (500 events).
    pub fn new() -> Self {
        Self::with_buffer_size(DEFAULT_MAX_BUFFERED_EVENTS)
    }

    /// Create a realtime manager with a custom ring-buffer size.
    pub fn with_buffer_size(max_buffered_events: usize) -> Self {
        let (tx, _) = broadcast::channel(1000);
        Self {
            tx,
            clients: Arc::new(RwLock::new(HashMap::new())),
            next_seq_id: Arc::new(AtomicU64::new(1)),
            buffer: Arc::new(RwLock::new(VecDeque::with_capacity(
                max_buffered_events.min(4096),
            ))),
            max_buffered_events,
        }
    }

    /// Broadcast an event to all matching clients.
    ///
    /// The event is stamped with a sequential `seq_id`, pushed into the ring
    /// buffer, and sent over the broadcast channel.
    pub fn broadcast(&self, mut event: RealtimeEvent) {
        // Stamp with sequential ID (fetch-and-increment, wraps at u64::MAX which
        // is effectively never for any real-world workload).
        let seq = self.next_seq_id.fetch_add(1, Ordering::Relaxed);
        event.seq_id = Some(seq);

        // Push into ring buffer, evicting the oldest entry when full.
        {
            let mut buf = self.buffer.write();
            if buf.len() >= self.max_buffered_events {
                buf.pop_front();
            }
            buf.push_back(event.clone());
        }

        // Deliver to live subscribers (errors are expected when no subscriber
        // is registered yet — ignore them).
        let _ = self.tx.send(event);
    }

    /// Return all buffered events whose `seq_id` is strictly greater than
    /// `last_seq_id`, in ascending order. Used to replay missed events for
    /// reconnecting clients.
    pub fn get_events_after(&self, last_seq_id: u64) -> Vec<RealtimeEvent> {
        self.buffer
            .read()
            .iter()
            .filter(|e| e.seq_id.is_some_and(|id| id > last_seq_id))
            .cloned()
            .collect()
    }

    /// Return the current value of the sequence counter (next ID to be issued).
    /// Mainly useful for tests.
    pub fn current_seq(&self) -> u64 {
        self.next_seq_id.load(Ordering::Relaxed)
    }

    /// Get number of connected clients
    pub fn client_count(&self) -> usize {
        self.clients.read().len()
    }

    /// Subscribe to live events
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
            next_seq_id: self.next_seq_id.clone(),
            buffer: self.buffer.clone(),
            max_buffered_events: self.max_buffered_events,
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

    // --- Sequential event ID tests ------------------------------------------

    #[test]
    fn test_broadcast_stamps_sequential_ids() {
        let manager = RealtimeManager::new();
        let _rx = manager.subscribe(); // keep channel alive

        manager.broadcast(RealtimeEvent::memory_created(1, "first".to_string()));
        manager.broadcast(RealtimeEvent::memory_created(2, "second".to_string()));
        manager.broadcast(RealtimeEvent::memory_deleted(3));

        // IDs should be 1, 2, 3 (counter starts at 1)
        let buf = manager.buffer.read();
        let ids: Vec<u64> = buf.iter().filter_map(|e| e.seq_id).collect();
        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn test_seq_id_starts_at_one() {
        let manager = RealtimeManager::new();
        assert_eq!(manager.current_seq(), 1);

        let _rx = manager.subscribe();
        manager.broadcast(RealtimeEvent::memory_created(1, "hello".to_string()));
        assert_eq!(manager.current_seq(), 2); // next id to be issued
    }

    // --- Ring buffer eviction tests -----------------------------------------

    #[test]
    fn test_ring_buffer_evicts_oldest_when_full() {
        let max = 3;
        let manager = RealtimeManager::with_buffer_size(max);
        let _rx = manager.subscribe();

        for i in 1..=5u64 {
            manager.broadcast(RealtimeEvent::memory_created(i as i64, format!("m{i}")));
        }

        let buf = manager.buffer.read();
        assert_eq!(buf.len(), max, "buffer should be at capacity");
        // The first two events (seq 1, 2) should have been evicted
        let ids: Vec<u64> = buf.iter().filter_map(|e| e.seq_id).collect();
        assert_eq!(ids, vec![3, 4, 5]);
    }

    #[test]
    fn test_ring_buffer_does_not_exceed_max_size() {
        let max = 10;
        let manager = RealtimeManager::with_buffer_size(max);
        let _rx = manager.subscribe();

        for i in 1..=20u64 {
            manager.broadcast(RealtimeEvent::memory_deleted(i as i64));
        }

        assert_eq!(manager.buffer.read().len(), max);
    }

    // --- Replay / get_events_after tests ------------------------------------

    #[test]
    fn test_get_events_after_returns_correct_subset() {
        let manager = RealtimeManager::new();
        let _rx = manager.subscribe();

        manager.broadcast(RealtimeEvent::memory_created(1, "a".to_string())); // seq 1
        manager.broadcast(RealtimeEvent::memory_created(2, "b".to_string())); // seq 2
        manager.broadcast(RealtimeEvent::memory_deleted(3)); // seq 3

        let replayed = manager.get_events_after(1);
        assert_eq!(replayed.len(), 2);
        let ids: Vec<u64> = replayed.iter().filter_map(|e| e.seq_id).collect();
        assert_eq!(ids, vec![2, 3]);
    }

    #[test]
    fn test_get_events_after_zero_returns_all() {
        let manager = RealtimeManager::new();
        let _rx = manager.subscribe();

        manager.broadcast(RealtimeEvent::memory_created(1, "x".to_string()));
        manager.broadcast(RealtimeEvent::memory_created(2, "y".to_string()));

        let replayed = manager.get_events_after(0);
        assert_eq!(replayed.len(), 2);
    }

    #[test]
    fn test_get_events_after_last_id_returns_empty() {
        let manager = RealtimeManager::new();
        let _rx = manager.subscribe();

        manager.broadcast(RealtimeEvent::memory_created(1, "only".to_string())); // seq 1

        // Requesting events after the last known ID → nothing new
        let replayed = manager.get_events_after(1);
        assert!(replayed.is_empty());
    }

    #[test]
    fn test_get_events_after_large_id_returns_empty() {
        let manager = RealtimeManager::new();
        let _rx = manager.subscribe();

        manager.broadcast(RealtimeEvent::memory_created(1, "ev".to_string()));

        let replayed = manager.get_events_after(9999);
        assert!(replayed.is_empty());
    }

    // --- Clone shares same state --------------------------------------------

    #[test]
    fn test_clone_shares_buffer() {
        let manager = RealtimeManager::new();
        let cloned = manager.clone();
        let _rx = manager.subscribe();

        manager.broadcast(RealtimeEvent::memory_created(1, "shared".to_string()));

        // cloned should see the same buffer
        assert_eq!(cloned.buffer.read().len(), 1);
        let replayed = cloned.get_events_after(0);
        assert_eq!(replayed.len(), 1);
    }
}
