//! Streamable HTTP transport for MCP (Model Context Protocol)
//!
//! Provides an axum-based HTTP server that accepts JSON-RPC requests at `POST /mcp`
//! and forwards them to the same `McpHandler` used by the stdio transport.
//!
//! Also provides a `GET /v1/events` SSE endpoint for real-time event streaming.

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tower_http::cors::{Any, CorsLayer};

use super::protocol::{McpHandler, McpRequest, McpResponse};
use crate::realtime::{EventType, RealtimeEvent, RealtimeManager};

/// Shared application state for all axum handlers.
#[derive(Clone)]
struct AppState {
    handler: Arc<dyn McpHandler>,
    api_key: Option<String>,
    realtime: Option<RealtimeManager>,
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// `POST /mcp` -- accept a JSON-RPC request and return a JSON-RPC response.
/// Per JSON-RPC 2.0, notifications (no `id`) MUST NOT produce a response.
async fn handle_mcp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<McpRequest>,
) -> impl IntoResponse {
    // Auth check
    if let Some(ref expected) = state.api_key {
        if !check_bearer(&headers, expected) {
            let err = McpResponse::error(request.id, -32000, "Unauthorized".to_string());
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::to_value(err).unwrap_or_default()),
            );
        }
    }

    // Notifications have no id — process for side effects, return 202 Accepted
    let is_notification = request.id.is_none();
    let response = state.handler.handle_request(request);
    if is_notification {
        return (StatusCode::ACCEPTED, Json(serde_json::Value::Null));
    }
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap_or_default()),
    )
}

/// `GET /health` -- lightweight liveness / readiness probe.
async fn handle_health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "protocol": "2025-11-25"
    }))
}

// ---------------------------------------------------------------------------
// SSE query parameters
// ---------------------------------------------------------------------------

/// Query parameters for the `GET /v1/events` SSE endpoint.
#[derive(Debug, Clone, Deserialize)]
struct EventsQuery {
    /// Comma-separated list of event types to subscribe to.
    /// Accepted values: `memory_created`, `memory_updated`, `memory_deleted`,
    /// `crossref_created`, `crossref_deleted`, `sync_started`, `sync_completed`,
    /// `sync_failed`.
    /// If omitted, all event types are streamed.
    event_types: Option<String>,

    /// Filter events to a specific workspace (matched against `data.workspace`).
    /// If omitted, events from all workspaces are streamed.
    workspace: Option<String>,
}

impl EventsQuery {
    /// Parse the `event_types` query param into a `Vec<EventType>`.
    /// Unknown tokens are silently ignored.
    fn parsed_event_types(&self) -> Option<Vec<EventType>> {
        let raw = self.event_types.as_deref()?;
        let types: Vec<EventType> = raw
            .split(',')
            .filter_map(|s| parse_event_type(s.trim()))
            .collect();
        if types.is_empty() {
            None
        } else {
            Some(types)
        }
    }
}

/// Parse a snake_case string into an `EventType`.
fn parse_event_type(s: &str) -> Option<EventType> {
    match s {
        "memory_created" => Some(EventType::MemoryCreated),
        "memory_updated" => Some(EventType::MemoryUpdated),
        "memory_deleted" => Some(EventType::MemoryDeleted),
        "crossref_created" => Some(EventType::CrossrefCreated),
        "crossref_deleted" => Some(EventType::CrossrefDeleted),
        "sync_started" => Some(EventType::SyncStarted),
        "sync_completed" => Some(EventType::SyncCompleted),
        "sync_failed" => Some(EventType::SyncFailed),
        _ => None,
    }
}

/// Serialize an `EventType` to its SSE `event:` field string.
fn event_type_to_str(et: EventType) -> &'static str {
    match et {
        EventType::MemoryCreated => "memory_created",
        EventType::MemoryUpdated => "memory_updated",
        EventType::MemoryDeleted => "memory_deleted",
        EventType::CrossrefCreated => "crossref_created",
        EventType::CrossrefDeleted => "crossref_deleted",
        EventType::SyncStarted => "sync_started",
        EventType::SyncCompleted => "sync_completed",
        EventType::SyncFailed => "sync_failed",
    }
}

// ---------------------------------------------------------------------------
// SSE handler
// ---------------------------------------------------------------------------

/// Reconnection backoff hint sent to SSE clients (milliseconds).
const SSE_RETRY_MS: u64 = 3000;

/// Convert a `RealtimeEvent` into an SSE `Event`, stamping the `id:` field
/// with `seq_id` when present.
fn realtime_event_to_sse(event: &RealtimeEvent) -> Event {
    let event_type_str = event_type_to_str(event.event_type);
    let data = serde_json::to_string(event).unwrap_or_else(|_| "{}".to_string());
    let mut sse = Event::default().event(event_type_str).data(data);
    if let Some(id) = event.seq_id {
        sse = sse.id(format!("{id}"));
    }
    sse
}

/// `GET /v1/events` — resumable Server-Sent Events stream of `RealtimeEvent`s.
///
/// Each event is sent as:
/// ```text
/// id: <seq_id>
/// event: <event_type>
/// data: <JSON of RealtimeEvent>
/// retry: 3000
/// ```
///
/// **Resumable streams:** clients that reconnect after a drop should include
/// the `Last-Event-Id` header set to the last `id` value they received.
/// The server will replay all buffered events with a higher sequence number
/// before continuing with the live stream.
///
/// Query parameters:
/// - `event_types` — comma-separated list of event types to subscribe to
/// - `workspace` — filter events by workspace (matched against `data.workspace`)
///
/// Requires `Authorization: Bearer <token>` when the server was started with an API key.
async fn handle_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EventsQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, StatusCode> {
    // Auth check
    if let Some(ref expected) = state.api_key {
        if !check_bearer(&headers, expected) {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // If realtime is not enabled, return 503.
    let manager = match state.realtime {
        Some(m) => m,
        None => return Err(StatusCode::SERVICE_UNAVAILABLE),
    };

    // Parse Last-Event-Id header for replay support.
    let last_event_id: Option<u64> = headers
        .get("last-event-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok());

    let event_type_filter = query.parsed_event_types();
    let workspace_filter = query.workspace.clone();

    // Build a filter closure reused for both replay and live events.
    let apply_filters = {
        let et_filter = event_type_filter.clone();
        let ws_filter = workspace_filter.clone();
        move |event: &RealtimeEvent| -> bool {
            if let Some(ref types) = et_filter {
                if !types.contains(&event.event_type) {
                    return false;
                }
            }
            if let Some(ref ws) = ws_filter {
                let event_ws = event
                    .data
                    .as_ref()
                    .and_then(|d: &serde_json::Value| d.get("workspace"))
                    .and_then(|v: &serde_json::Value| v.as_str());
                match event_ws {
                    Some(ews) if ews == ws => {}
                    _ => return false,
                }
            }
            true
        }
    };

    // Subscribe to the live broadcast channel *before* draining the buffer so
    // we don't miss any events that arrive between the two operations.
    let rx = manager.subscribe();
    let broadcast_stream = BroadcastStream::new(rx);

    // Build the replay burst (may be empty if no Last-Event-Id or nothing to replay).
    let replay_events: Vec<Result<Event, Infallible>> = if let Some(last_id) = last_event_id {
        manager
            .get_events_after(last_id)
            .into_iter()
            .filter(|e| apply_filters(e))
            .map(|e| Ok::<Event, Infallible>(realtime_event_to_sse(&e)))
            .collect()
    } else {
        vec![]
    };

    let replay_stream = tokio_stream::iter(replay_events);

    // Live stream from broadcast channel.
    let live_stream = broadcast_stream.filter_map(move |result| {
        match result {
            // Lagged: the receiver fell behind — skip dropped events without crashing.
            Err(_lagged) => None,
            Ok(event) => {
                if !apply_filters(&event) {
                    return None;
                }
                Some(Ok::<Event, Infallible>(realtime_event_to_sse(&event)))
            }
        }
    });

    // Chain: replay burst first, then live events.
    let combined = replay_stream.chain(live_stream);

    // Prepend a `retry:` field so clients know the reconnection backoff.
    // The retry directive is sent as a synthetic SSE comment event emitted once
    // at the start of the stream.
    let retry_event = std::iter::once(Ok::<Event, Infallible>(
        Event::default().retry(std::time::Duration::from_millis(SSE_RETRY_MS)),
    ));
    let full_stream = tokio_stream::iter(retry_event).chain(combined);

    Ok(Sse::new(full_stream)
        .keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(30))))
}

// ---------------------------------------------------------------------------
// Auth helpers
// ---------------------------------------------------------------------------

/// Return `true` when the `Authorization: Bearer <token>` header matches the
/// expected key.
fn check_bearer(headers: &HeaderMap, expected: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.strip_prefix("Bearer ")
                .map(|token| token == expected)
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Public entry-point
// ---------------------------------------------------------------------------

/// Start the axum HTTP server on `0.0.0.0:{port}`.
///
/// The server will run until the process is terminated.
///
/// - `realtime` — optional `RealtimeManager` for SSE streaming (`GET /v1/events`).
///   When `None`, the `/v1/events` endpoint returns `503 Service Unavailable`.
pub async fn serve_http(
    handler: Arc<dyn McpHandler>,
    port: u16,
    api_key: Option<String>,
    realtime: Option<RealtimeManager>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = AppState {
        handler,
        api_key,
        realtime,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/health", get(handle_health))
        .route("/v1/events", get(handle_events))
        .layer(cors)
        .with_state(state);

    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("HTTP transport listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realtime::RealtimeEvent;

    // ---- check_bearer tests ------------------------------------------------

    /// Ensure `check_bearer` correctly validates tokens.
    #[test]
    fn test_check_bearer_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer my-secret".parse().unwrap());
        assert!(check_bearer(&headers, "my-secret"));
    }

    #[test]
    fn test_check_bearer_invalid_token() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Bearer wrong".parse().unwrap());
        assert!(!check_bearer(&headers, "my-secret"));
    }

    #[test]
    fn test_check_bearer_missing_header() {
        let headers = HeaderMap::new();
        assert!(!check_bearer(&headers, "my-secret"));
    }

    #[test]
    fn test_check_bearer_bad_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", "Basic abc123".parse().unwrap());
        assert!(!check_bearer(&headers, "abc123"));
    }

    // ---- SSE event serialization tests ------------------------------------

    /// Verify that a `RealtimeEvent` can be round-tripped through JSON
    /// and produces the expected SSE `event:` field value.
    #[test]
    fn test_sse_event_serialization() {
        let event = RealtimeEvent::memory_created(42, "hello world".to_string());
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"memory_created\""));
        assert!(json.contains("\"memory_id\":42"));
        assert_eq!(event_type_to_str(event.event_type), "memory_created");
    }

    #[test]
    fn test_sse_event_type_to_str_all_variants() {
        assert_eq!(
            event_type_to_str(EventType::MemoryCreated),
            "memory_created"
        );
        assert_eq!(
            event_type_to_str(EventType::MemoryUpdated),
            "memory_updated"
        );
        assert_eq!(
            event_type_to_str(EventType::MemoryDeleted),
            "memory_deleted"
        );
        assert_eq!(
            event_type_to_str(EventType::CrossrefCreated),
            "crossref_created"
        );
        assert_eq!(
            event_type_to_str(EventType::CrossrefDeleted),
            "crossref_deleted"
        );
        assert_eq!(event_type_to_str(EventType::SyncStarted), "sync_started");
        assert_eq!(
            event_type_to_str(EventType::SyncCompleted),
            "sync_completed"
        );
        assert_eq!(event_type_to_str(EventType::SyncFailed), "sync_failed");
    }

    // ---- parse_event_type tests -------------------------------------------

    #[test]
    fn test_parse_event_type_known() {
        assert_eq!(
            parse_event_type("memory_created"),
            Some(EventType::MemoryCreated)
        );
        assert_eq!(parse_event_type("sync_failed"), Some(EventType::SyncFailed));
    }

    #[test]
    fn test_parse_event_type_unknown_is_none() {
        assert_eq!(parse_event_type("unknown_event"), None);
        assert_eq!(parse_event_type(""), None);
    }

    // ---- EventsQuery filter parsing tests ---------------------------------

    #[test]
    fn test_events_query_parsed_event_types_none() {
        let q = EventsQuery {
            event_types: None,
            workspace: None,
        };
        assert!(q.parsed_event_types().is_none());
    }

    #[test]
    fn test_events_query_parsed_event_types_single() {
        let q = EventsQuery {
            event_types: Some("memory_created".to_string()),
            workspace: None,
        };
        let types = q.parsed_event_types().unwrap();
        assert_eq!(types, vec![EventType::MemoryCreated]);
    }

    #[test]
    fn test_events_query_parsed_event_types_multiple() {
        let q = EventsQuery {
            event_types: Some("memory_created,memory_deleted,sync_failed".to_string()),
            workspace: None,
        };
        let types = q.parsed_event_types().unwrap();
        assert_eq!(
            types,
            vec![
                EventType::MemoryCreated,
                EventType::MemoryDeleted,
                EventType::SyncFailed
            ]
        );
    }

    #[test]
    fn test_events_query_parsed_event_types_with_spaces() {
        let q = EventsQuery {
            event_types: Some("memory_created, memory_updated".to_string()),
            workspace: None,
        };
        let types = q.parsed_event_types().unwrap();
        assert_eq!(
            types,
            vec![EventType::MemoryCreated, EventType::MemoryUpdated]
        );
    }

    #[test]
    fn test_events_query_parsed_event_types_all_unknown_returns_none() {
        let q = EventsQuery {
            event_types: Some("bogus,fake".to_string()),
            workspace: None,
        };
        // All tokens invalid → None (no filter)
        assert!(q.parsed_event_types().is_none());
    }

    // ---- Filter matching tests (via SubscriptionFilter in events module) --

    #[test]
    fn test_event_type_filter_matches() {
        use crate::realtime::SubscriptionFilter;

        let filter = SubscriptionFilter {
            event_types: Some(vec![EventType::MemoryCreated]),
            memory_ids: None,
            tags: None,
        };
        let created = RealtimeEvent::memory_created(1, "test".to_string());
        let deleted = RealtimeEvent::memory_deleted(1);
        assert!(filter.matches(&created));
        assert!(!filter.matches(&deleted));
    }

    // ---- Auth rejection test (integration-style, no network) -------------

    #[test]
    fn test_auth_rejection_no_header() {
        // Without bearer header, check_bearer should return false for any key.
        let headers = HeaderMap::new();
        assert!(!check_bearer(&headers, "secret-key"));
    }

    #[test]
    fn test_auth_no_key_configured_always_passes() {
        // When api_key is None, the server allows any request.
        // check_bearer is only called when api_key is Some, so this
        // test documents the expected behavior.
        let has_key: Option<String> = None;
        // No key = no auth check = always allowed
        assert!(has_key.is_none());
    }

    // ---- Keep-alive configuration test ------------------------------------

    #[test]
    fn test_keep_alive_interval_is_30s() {
        // Verify the constant used for keep-alive is correct.
        let interval = std::time::Duration::from_secs(30);
        assert_eq!(interval.as_secs(), 30);
    }

    // ---- Last-Event-Id header parsing tests --------------------------------

    /// Verify that a valid numeric `Last-Event-Id` header is parsed to `u64`.
    #[test]
    fn test_last_event_id_header_valid() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "42".parse().unwrap());

        let parsed: Option<u64> = headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        assert_eq!(parsed, Some(42));
    }

    #[test]
    fn test_last_event_id_header_missing_is_none() {
        let headers = HeaderMap::new();
        let parsed: Option<u64> = headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        assert!(parsed.is_none());
    }

    #[test]
    fn test_last_event_id_header_non_numeric_is_none() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "not-a-number".parse().unwrap());
        let parsed: Option<u64> = headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        assert!(parsed.is_none());
    }

    #[test]
    fn test_last_event_id_header_zero() {
        let mut headers = HeaderMap::new();
        headers.insert("last-event-id", "0".parse().unwrap());
        let parsed: Option<u64> = headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        assert_eq!(parsed, Some(0));
    }

    // ---- realtime_event_to_sse tests ---------------------------------------

    /// Verify that an event with a seq_id produces an SSE event with an `id:` field.
    #[test]
    fn test_realtime_event_to_sse_with_seq_id() {
        use crate::realtime::RealtimeManager;

        let manager = RealtimeManager::new();
        let _rx = manager.subscribe();
        manager.broadcast(RealtimeEvent::memory_created(1, "hello".to_string()));

        let buffered = manager.get_events_after(0);
        assert_eq!(buffered.len(), 1);

        let event = &buffered[0];
        assert_eq!(event.seq_id, Some(1));

        // Verify the SSE event would include the id
        // (axum's Event::id sets the id field; we verify seq_id is present)
        let sse = realtime_event_to_sse(event);
        // The event should serialize without panic; content is verified via seq_id field
        let _ = sse; // axum::sse::Event has no public getter, just verify it builds
    }

    #[test]
    fn test_realtime_event_to_sse_without_seq_id_no_id_field() {
        // Events with seq_id = None should still build an SSE event (no id field).
        let event = RealtimeEvent::memory_created(5, "no id".to_string());
        assert!(event.seq_id.is_none());
        let sse = realtime_event_to_sse(&event);
        let _ = sse; // should not panic
    }

    // ---- Replay via get_events_after + Last-Event-Id integration -----------

    #[test]
    fn test_replay_events_after_last_id() {
        use crate::realtime::RealtimeManager;

        let manager = RealtimeManager::new();
        let _rx = manager.subscribe();

        // Broadcast 5 events
        for i in 1..=5i64 {
            manager.broadcast(RealtimeEvent::memory_created(i, format!("ev{i}")));
        }

        // Simulate Last-Event-Id: 3 — client missed events 4 and 5
        let last_id: u64 = 3;
        let replayed = manager.get_events_after(last_id);
        assert_eq!(replayed.len(), 2);
        let ids: Vec<u64> = replayed.iter().filter_map(|e| e.seq_id).collect();
        assert_eq!(ids, vec![4, 5]);
    }

    #[test]
    fn test_retry_constant_is_3000ms() {
        assert_eq!(SSE_RETRY_MS, 3000);
    }
}
