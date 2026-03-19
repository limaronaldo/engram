//! gRPC transport for the MCP server.
//!
//! Exposes the same `McpHandler` trait used by stdio and HTTP transports through
//! a tonic-based gRPC server, enabling strongly-typed, bidirectional-streaming
//! access to all 200+ Engram MCP tools.
//!
//! # Feature gate
//! This module is compiled only when the `grpc` feature is active.
//!
//! # Design
//! - [`GrpcMcpService`] bridges the generated tonic stubs to `McpHandler`.
//! - Params/results travel as JSON strings (`params_json`, `result_json`) so
//!   the protobuf schema remains stable as the tool catalogue grows.
//! - Auth is checked via the gRPC metadata `authorization` header
//!   (`Bearer <token>`), mirroring the HTTP transport.
//! - Streaming events are sourced from `RealtimeManager::subscribe()` and
//!   pushed through a `tokio_stream::wrappers::BroadcastStream`.

use std::pin::Pin;
use std::sync::Arc;

use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};
use tonic::{metadata::MetadataMap, transport::Server, Request, Response, Status};

use super::protocol::{McpHandler, McpRequest, McpResponse};
use crate::realtime::{EventType, RealtimeManager};

// Include generated tonic stubs.
pub mod proto {
    tonic::include_proto!("engram.mcp");
}

use proto::mcp_service_server::{McpService, McpServiceServer};
use proto::{
    mcp_response, McpError as ProtoMcpError, McpEvent, McpRequest as ProtoRequest,
    McpResponse as ProtoResponse, SubscribeRequest,
};

// ---------------------------------------------------------------------------
// Service implementation
// ---------------------------------------------------------------------------

/// gRPC service that bridges tonic to the `McpHandler` trait.
pub struct GrpcMcpService {
    handler: Arc<dyn McpHandler>,
    api_key: Option<String>,
    realtime: Option<RealtimeManager>,
}

impl GrpcMcpService {
    /// Create a new service.
    pub fn new(
        handler: Arc<dyn McpHandler>,
        api_key: Option<String>,
        realtime: Option<RealtimeManager>,
    ) -> Self {
        Self {
            handler,
            api_key,
            realtime,
        }
    }
}

// ---------------------------------------------------------------------------
// Auth helper
// ---------------------------------------------------------------------------

/// Validate the `Authorization: Bearer <token>` gRPC metadata header.
///
/// Returns `Ok(())` when:
/// - No API key is configured (open access), or
/// - The metadata contains a matching bearer token.
///
/// Returns `Err(Status::unauthenticated(...))` otherwise.
#[allow(clippy::result_large_err)]
fn check_auth(metadata: &MetadataMap, expected: &Option<String>) -> Result<(), Status> {
    let Some(ref key) = expected else {
        return Ok(());
    };

    let token = metadata
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");

    if token == key.as_str() {
        Ok(())
    } else {
        Err(Status::unauthenticated("Invalid or missing Bearer token"))
    }
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Convert a protobuf `McpRequest` to the protocol-layer `McpRequest`.
///
/// The `id` field is stored as a JSON string. An empty `id` represents a
/// JSON-RPC notification (no id).
fn proto_to_handler_request(req: ProtoRequest) -> McpRequest {
    let id = if req.id.is_empty() {
        None
    } else {
        Some(serde_json::Value::String(req.id))
    };

    let params = serde_json::from_str::<serde_json::Value>(&req.params_json)
        .unwrap_or(serde_json::Value::Null);

    McpRequest {
        jsonrpc: "2.0".to_string(),
        id,
        method: req.method,
        params,
    }
}

/// Convert a protocol-layer `McpResponse` to a protobuf `McpResponse`.
fn handler_to_proto_response(resp: McpResponse) -> ProtoResponse {
    let id = resp
        .id
        .as_ref()
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if let Some(result) = resp.result {
        let result_json = serde_json::to_string(&result).unwrap_or_else(|_| "null".to_string());
        ProtoResponse {
            id,
            result: Some(mcp_response::Result::ResultJson(result_json)),
        }
    } else if let Some(err) = resp.error {
        let error = ProtoMcpError {
            code: err.code as i32,
            message: err.message,
            data_json: err
                .data
                .as_ref()
                .map(|d| serde_json::to_string(d).unwrap_or_default())
                .unwrap_or_default(),
        };
        ProtoResponse {
            id,
            result: Some(mcp_response::Result::Error(error)),
        }
    } else {
        // Notification response — empty result
        ProtoResponse { id, result: None }
    }
}

/// Parse event type string into `EventType`.
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

// ---------------------------------------------------------------------------
// Tonic service trait implementation
// ---------------------------------------------------------------------------

type EventStream = Pin<Box<dyn Stream<Item = Result<McpEvent, Status>> + Send>>;

#[tonic::async_trait]
impl McpService for GrpcMcpService {
    /// Handle a unary MCP call — mirrors JSON-RPC request/response semantics.
    async fn call(
        &self,
        request: Request<ProtoRequest>,
    ) -> Result<Response<ProtoResponse>, Status> {
        check_auth(request.metadata(), &self.api_key)?;

        let handler_req = proto_to_handler_request(request.into_inner());
        let handler_resp = self.handler.handle_request(handler_req);
        let proto_resp = handler_to_proto_response(handler_resp);
        Ok(Response::new(proto_resp))
    }

    type SubscribeStream = EventStream;

    /// Open a server-streaming subscription — events are filtered by
    /// `event_types` and `workspace`, then forwarded as `McpEvent` messages.
    async fn subscribe(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeStream>, Status> {
        check_auth(request.metadata(), &self.api_key)?;

        let sub_req = request.into_inner();

        let realtime = self
            .realtime
            .as_ref()
            .ok_or_else(|| Status::unavailable("Real-time events are not enabled on this server"))?;

        // Parse requested event type filters (empty = all).
        let type_filters: Vec<EventType> = sub_req
            .event_types
            .iter()
            .filter_map(|s| parse_event_type(s))
            .collect();

        let workspace_filter = if sub_req.workspace.is_empty() {
            None
        } else {
            Some(sub_req.workspace.clone())
        };

        let rx = realtime.subscribe();
        let stream = BroadcastStream::new(rx).filter_map(move |result| {
            let type_filters = type_filters.clone();
            let workspace_filter = workspace_filter.clone();

            match result {
                Err(_) => None, // Lagged — skip
                Ok(event) => {
                    // Apply event-type filter
                    if !type_filters.is_empty() && !type_filters.contains(&event.event_type) {
                        return None;
                    }

                    // Apply workspace filter (events carry workspace in `data.workspace`)
                    if let Some(ref ws) = workspace_filter {
                        let event_workspace = event
                            .data
                            .as_ref()
                            .and_then(|d| d.get("workspace"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        if event_workspace != ws.as_str() {
                            return None;
                        }
                    }

                    let event_type = format!("{:?}", event.event_type)
                        .chars()
                        .enumerate()
                        .map(|(i, c)| {
                            if c.is_uppercase() && i > 0 {
                                format!("_{}", c.to_lowercase())
                            } else {
                                c.to_lowercase().to_string()
                            }
                        })
                        .collect::<String>();

                    let data_json = serde_json::to_string(&event)
                        .unwrap_or_else(|_| "{}".to_string());

                    Some(Ok(McpEvent {
                        event_type,
                        data_json,
                        sequence_id: event.seq_id.unwrap_or(0),
                    }))
                }
            }
        });

        Ok(Response::new(Box::pin(stream)))
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Start the gRPC server.
///
/// Binds to `0.0.0.0:{port}` and serves until an error occurs or the process
/// is interrupted. Mirrors the signature of `serve_http()` in `http_transport`.
pub async fn serve_grpc(
    handler: Arc<dyn McpHandler>,
    port: u16,
    api_key: Option<String>,
    realtime: Option<RealtimeManager>,
) -> crate::error::Result<()> {
    let addr = format!("0.0.0.0:{port}")
        .parse::<std::net::SocketAddr>()
        .map_err(|e| crate::error::EngramError::Internal(e.to_string()))?;

    let service = GrpcMcpService::new(handler, api_key, realtime);

    tracing::info!(port = port, "gRPC transport listening");

    Server::builder()
        .add_service(McpServiceServer::new(service))
        .serve(addr)
        .await
        .map_err(|e| crate::error::EngramError::Internal(e.to_string()))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct EchoHandler;

    impl McpHandler for EchoHandler {
        fn handle_request(&self, request: McpRequest) -> McpResponse {
            McpResponse::success(request.id, serde_json::json!({"method": request.method}))
        }
    }

    fn make_service() -> GrpcMcpService {
        GrpcMcpService::new(Arc::new(EchoHandler), None, None)
    }

    // --- proto_to_handler_request ---

    #[test]
    fn converts_proto_request_with_id() {
        let proto_req = ProtoRequest {
            id: "42".to_string(),
            method: "tools/list".to_string(),
            params_json: r#"{"cursor":null}"#.to_string(),
        };
        let req = proto_to_handler_request(proto_req);
        assert_eq!(req.id, Some(serde_json::Value::String("42".to_string())));
        assert_eq!(req.method, "tools/list");
        assert_eq!(req.params["cursor"], serde_json::Value::Null);
    }

    #[test]
    fn converts_proto_notification_empty_id() {
        let proto_req = ProtoRequest {
            id: "".to_string(),
            method: "notifications/initialized".to_string(),
            params_json: "{}".to_string(),
        };
        let req = proto_to_handler_request(proto_req);
        assert!(req.id.is_none(), "empty id should map to None");
    }

    #[test]
    fn handles_invalid_params_json_gracefully() {
        let proto_req = ProtoRequest {
            id: "1".to_string(),
            method: "tools/call".to_string(),
            params_json: "not valid json {{".to_string(),
        };
        let req = proto_to_handler_request(proto_req);
        assert_eq!(req.params, serde_json::Value::Null);
    }

    // --- handler_to_proto_response ---

    #[test]
    fn converts_success_response() {
        let resp = McpResponse::success(
            Some(serde_json::Value::String("1".to_string())),
            serde_json::json!({"ok": true}),
        );
        let proto = handler_to_proto_response(resp);
        assert_eq!(proto.id, "1");
        match proto.result {
            Some(proto::mcp_response::Result::ResultJson(json)) => {
                assert!(json.contains("ok"));
            }
            other => panic!("expected ResultJson, got {:?}", other),
        }
    }

    #[test]
    fn converts_error_response() {
        let resp = McpResponse::error(
            Some(serde_json::Value::String("2".to_string())),
            -32601,
            "Method not found".to_string(),
        );
        let proto = handler_to_proto_response(resp);
        match proto.result {
            Some(proto::mcp_response::Result::Error(err)) => {
                assert_eq!(err.code, -32601);
                assert_eq!(err.message, "Method not found");
            }
            other => panic!("expected Error variant, got {:?}", other),
        }
    }

    // --- check_auth ---

    #[test]
    fn auth_passes_when_no_key_configured() {
        let metadata = MetadataMap::new();
        assert!(check_auth(&metadata, &None).is_ok());
    }

    #[test]
    fn auth_fails_when_token_missing() {
        let metadata = MetadataMap::new();
        let key = Some("secret".to_string());
        assert!(check_auth(&metadata, &key).is_err());
    }

    #[test]
    fn auth_passes_with_correct_bearer_token() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "authorization",
            "Bearer secret".parse().unwrap(),
        );
        let key = Some("secret".to_string());
        assert!(check_auth(&metadata, &key).is_ok());
    }

    #[test]
    fn auth_fails_with_wrong_bearer_token() {
        let mut metadata = MetadataMap::new();
        metadata.insert(
            "authorization",
            "Bearer wrong".parse().unwrap(),
        );
        let key = Some("secret".to_string());
        let result = check_auth(&metadata, &key);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    // --- parse_event_type ---

    #[test]
    fn parses_all_known_event_types() {
        let cases = [
            ("memory_created", EventType::MemoryCreated),
            ("memory_updated", EventType::MemoryUpdated),
            ("memory_deleted", EventType::MemoryDeleted),
            ("crossref_created", EventType::CrossrefCreated),
            ("crossref_deleted", EventType::CrossrefDeleted),
            ("sync_started", EventType::SyncStarted),
            ("sync_completed", EventType::SyncCompleted),
            ("sync_failed", EventType::SyncFailed),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_event_type(input), Some(expected), "failed for {input}");
        }
        assert_eq!(parse_event_type("unknown"), None);
    }

    // --- integration: round-trip through service.call() ---

    #[tokio::test]
    async fn grpc_call_round_trip() {
        let svc = make_service();
        let proto_req = ProtoRequest {
            id: "99".to_string(),
            method: "initialize".to_string(),
            params_json: "{}".to_string(),
        };
        let tonic_req = Request::new(proto_req);
        let resp = svc.call(tonic_req).await.expect("call failed");
        let inner = resp.into_inner();
        assert_eq!(inner.id, "99");
        match inner.result {
            Some(proto::mcp_response::Result::ResultJson(json)) => {
                assert!(json.contains("initialize"), "expected method echo in result");
            }
            other => panic!("unexpected result: {:?}", other),
        }
    }

    #[tokio::test]
    async fn grpc_call_rejects_wrong_token() {
        let svc = GrpcMcpService::new(
            Arc::new(EchoHandler),
            Some("correct-token".to_string()),
            None,
        );
        let proto_req = ProtoRequest {
            id: "1".to_string(),
            method: "initialize".to_string(),
            params_json: "{}".to_string(),
        };
        let mut req = Request::new(proto_req);
        req.metadata_mut().insert(
            "authorization",
            "Bearer wrong-token".parse().unwrap(),
        );
        let err = svc.call(req).await.unwrap_err();
        assert_eq!(err.code(), tonic::Code::Unauthenticated);
    }
}
