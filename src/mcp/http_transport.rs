//! Streamable HTTP transport for MCP (Model Context Protocol)
//!
//! Provides an axum-based HTTP server that accepts JSON-RPC requests at `POST /mcp`
//! and forwards them to the same `McpHandler` used by the stdio transport.

use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use tower_http::cors::{Any, CorsLayer};

use super::protocol::{McpHandler, McpRequest, McpResponse};

/// Shared application state for all axum handlers.
#[derive(Clone)]
struct AppState {
    handler: Arc<dyn McpHandler>,
    api_key: Option<String>,
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
            return (StatusCode::UNAUTHORIZED, Json(serde_json::to_value(err).unwrap_or_default()));
        }
    }

    // Notifications have no id — process for side effects, return 202 Accepted
    let is_notification = request.id.is_none();
    let response = state.handler.handle_request(request);
    if is_notification {
        return (StatusCode::ACCEPTED, Json(serde_json::Value::Null));
    }
    (StatusCode::OK, Json(serde_json::to_value(response).unwrap_or_default()))
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
pub async fn serve_http(
    handler: Arc<dyn McpHandler>,
    port: u16,
    api_key: Option<String>,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let state = AppState { handler, api_key };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/mcp", post(handle_mcp))
        .route("/health", get(handle_health))
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
}
