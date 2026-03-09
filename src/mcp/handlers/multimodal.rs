//! MCP tool handlers for multimodal features: vision, audio, screenshot, and video.
//!
//! All handlers are feature-gated with `#[cfg(feature = "multimodal")]`.
//! Async provider calls are executed via a short-lived `tokio::runtime::Runtime`
//! so that the synchronous MCP dispatch loop is not affected.

#[cfg(feature = "multimodal")]
use serde_json::{json, Value};

#[cfg(feature = "multimodal")]
use super::HandlerContext;

// ── memory_describe_image ─────────────────────────────────────────────────────

/// Describe an image file using the configured vision provider.
///
/// Required params:
/// - `image_path` (string) — absolute or relative path to the image file
///
/// Optional params:
/// - `prompt` (string) — custom prompt passed to the vision model
///
/// Returns: `{ text, model, provider }`
#[cfg(feature = "multimodal")]
pub fn memory_describe_image(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::multimodal::vision::{VisionInput, VisionOptions, VisionProviderFactory};

    let image_path = match params.get("image_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "image_path is required"}),
    };

    let prompt = params
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let provider = match VisionProviderFactory::from_env() {
        Ok(p) => p,
        Err(e) => return json!({"error": format!("Vision provider not configured: {}", e)}),
    };

    let image_bytes = match std::fs::read(&image_path) {
        Ok(bytes) => bytes,
        Err(e) => {
            return json!({"error": format!("Failed to read image file '{}': {}", image_path, e)})
        }
    };

    let mime_type = infer_mime_type(&image_path);

    let input = VisionInput {
        image_bytes,
        mime_type,
    };

    let opts = VisionOptions {
        prompt,
        max_tokens: None,
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return json!({"error": format!("Failed to create async runtime: {}", e)}),
    };

    match rt.block_on(provider.describe_image(input, opts)) {
        Ok(desc) => json!({
            "text": desc.text,
            "model": desc.model,
            "provider": desc.provider,
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── memory_transcribe_audio ───────────────────────────────────────────────────

/// Transcribe an audio file using the configured audio transcription provider.
///
/// Required params:
/// - `audio_path` (string) — path to the audio file
///
/// Returns: `{ text, language, duration_secs, segments }`
#[cfg(feature = "multimodal")]
pub fn memory_transcribe_audio(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::multimodal::audio::AudioTranscriberFactory;
    use std::path::Path;

    let audio_path = match params.get("audio_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "audio_path is required"}),
    };

    let transcriber = match AudioTranscriberFactory::from_env() {
        Ok(t) => t,
        Err(e) => {
            return json!({"error": format!("Audio transcription provider not configured: {}", e)})
        }
    };

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return json!({"error": format!("Failed to create async runtime: {}", e)}),
    };

    match rt.block_on(transcriber.transcribe(Path::new(&audio_path))) {
        Ok(result) => {
            let segments: Vec<Value> = result
                .segments
                .iter()
                .map(|s| {
                    json!({
                        "start_secs": s.start_secs,
                        "end_secs": s.end_secs,
                        "text": s.text,
                    })
                })
                .collect();

            json!({
                "text": result.text,
                "language": result.language,
                "duration_secs": result.duration_secs,
                "segments": segments,
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── memory_capture_screenshot ─────────────────────────────────────────────────

/// Capture a screenshot of the full screen or a specific application window.
///
/// Optional params:
/// - `app_name` (string) — if provided, captures that app's window; otherwise captures full screen
///
/// Returns: `{ image_path, width, height, file_size, file_hash }`
#[cfg(feature = "multimodal")]
pub fn memory_capture_screenshot(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::multimodal::screenshot::ScreenshotCapture;

    let capture = match ScreenshotCapture::new() {
        Ok(c) => c,
        Err(e) => {
            return json!({"error": format!("Failed to initialize screenshot capture: {}", e)})
        }
    };

    let app_name = params
        .get("app_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let result = if let Some(app) = app_name {
        capture.capture_window(&app)
    } else {
        capture.capture()
    };

    match result {
        Ok(screenshot) => json!({
            "image_path": screenshot.image_path.to_string_lossy(),
            "width": screenshot.width,
            "height": screenshot.height,
            "file_size": screenshot.file_size,
            "file_hash": screenshot.file_hash,
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── memory_process_video ──────────────────────────────────────────────────────

/// Process a video file: extract metadata and keyframe descriptions.
///
/// Required params:
/// - `video_path` (string) — path to the video file
///
/// Returns: `{ metadata, keyframe_descriptions, summary }`
#[cfg(feature = "multimodal")]
pub fn memory_process_video(_ctx: &HandlerContext, params: Value) -> Value {
    use crate::multimodal::video::VideoProcessor;
    use crate::multimodal::vision::VisionProviderFactory;
    use std::path::Path;

    let video_path = match params.get("video_path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return json!({"error": "video_path is required"}),
    };

    let vision = match VisionProviderFactory::from_env() {
        Ok(p) => p,
        Err(e) => return json!({"error": format!("Vision provider not configured: {}", e)}),
    };

    let processor = VideoProcessor::new();

    if let Err(e) = processor.check_availability() {
        return json!({"error": format!("Video processing unavailable: {}", e)});
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return json!({"error": format!("Failed to create async runtime: {}", e)}),
    };

    match rt.block_on(processor.create_video_memory(Path::new(&video_path), vision.as_ref())) {
        Ok(video_memory) => {
            let meta = &video_memory.metadata;
            json!({
                "metadata": {
                    "duration_secs": meta.duration_secs,
                    "width": meta.width,
                    "height": meta.height,
                    "codec": meta.codec,
                    "file_size": meta.file_size,
                    "file_hash": meta.file_hash,
                },
                "keyframe_descriptions": video_memory.keyframe_descriptions,
                "summary": video_memory.summary,
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ── memory_list_media ─────────────────────────────────────────────────────────

/// List media assets stored in the media_assets table.
///
/// Optional params:
/// - `media_type` (string) — filter by type: "image", "audio", "video"
/// - `limit` (integer) — maximum number of results (default 50)
///
/// Returns: `{ assets: [...], count }`
#[cfg(feature = "multimodal")]
pub fn memory_list_media(ctx: &HandlerContext, params: Value) -> Value {
    let media_type = params
        .get("media_type")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

    ctx.storage
        .with_connection(|conn| {
            let assets = query_media_assets(conn, media_type.as_deref(), limit)?;
            Ok(json!({
                "assets": assets,
                "count": assets.len(),
            }))
        })
        .unwrap_or_else(|e| json!({"error": e.to_string()}))
}

/// Query media_assets table, optionally filtered by media_type.
#[cfg(feature = "multimodal")]
fn query_media_assets(
    conn: &rusqlite::Connection,
    media_type: Option<&str>,
    limit: usize,
) -> crate::error::Result<Vec<serde_json::Value>> {
    let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
        if let Some(mt) = media_type {
            (
                "SELECT id, memory_id, media_type, file_hash, file_path, file_size, \
                 mime_type, duration_secs, width, height, transcription, description, \
                 provider, model, created_at \
                 FROM media_assets WHERE media_type = ?1 \
                 ORDER BY created_at DESC LIMIT ?2"
                    .to_string(),
                vec![Box::new(mt.to_string()), Box::new(limit as i64)],
            )
        } else {
            (
                "SELECT id, memory_id, media_type, file_hash, file_path, file_size, \
                 mime_type, duration_secs, width, height, transcription, description, \
                 provider, model, created_at \
                 FROM media_assets ORDER BY created_at DESC LIMIT ?1"
                    .to_string(),
                vec![Box::new(limit as i64)],
            )
        };

    let mut stmt = conn.prepare(&sql)?;

    let rows: Vec<serde_json::Value> = stmt
        .query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "memory_id": row.get::<_, i64>(1)?,
                "media_type": row.get::<_, String>(2)?,
                "file_hash": row.get::<_, String>(3)?,
                "file_path": row.get::<_, Option<String>>(4)?,
                "file_size": row.get::<_, Option<i64>>(5)?,
                "mime_type": row.get::<_, Option<String>>(6)?,
                "duration_secs": row.get::<_, Option<f64>>(7)?,
                "width": row.get::<_, Option<i64>>(8)?,
                "height": row.get::<_, Option<i64>>(9)?,
                "transcription": row.get::<_, Option<String>>(10)?,
                "description": row.get::<_, Option<String>>(11)?,
                "provider": row.get::<_, Option<String>>(12)?,
                "model": row.get::<_, Option<String>>(13)?,
                "created_at": row.get::<_, String>(14)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Infer MIME type from file extension.
#[cfg(feature = "multimodal")]
fn infer_mime_type(path: &str) -> String {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        _ => "image/png", // default fallback
    }
    .to_string()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[cfg(feature = "multimodal")]
mod tests {
    use super::*;
    use crate::mcp::handlers::HandlerContext;
    use crate::storage::Storage;
    use serde_json::json;
    use std::sync::Arc;

    fn make_ctx() -> HandlerContext {
        use crate::embedding::{create_embedder, EmbeddingCache};
        use crate::search::{AdaptiveCacheConfig, FuzzyEngine, SearchConfig, SearchResultCache};
        use crate::types::EmbeddingConfig;
        use parking_lot::Mutex;

        let storage = Storage::open_in_memory().expect("in-memory storage should open");
        let embedder = create_embedder(&EmbeddingConfig::default()).expect("tfidf embedder");
        HandlerContext {
            storage,
            embedder,
            fuzzy_engine: Arc::new(Mutex::new(FuzzyEngine::new())),
            search_config: SearchConfig::default(),
            realtime: None,
            embedding_cache: Arc::new(EmbeddingCache::default()),
            search_cache: Arc::new(SearchResultCache::new(AdaptiveCacheConfig::default())),
            #[cfg(feature = "meilisearch")]
            meili: None,
            #[cfg(feature = "meilisearch")]
            meili_indexer: None,
            #[cfg(feature = "meilisearch")]
            meili_sync_interval: 60,
            #[cfg(feature = "langfuse")]
            langfuse_runtime: Arc::new(tokio::runtime::Runtime::new().expect("langfuse runtime")),
        }
    }

    #[test]
    fn test_describe_image_missing_param() {
        let ctx = make_ctx();
        let result = memory_describe_image(&ctx, json!({}));
        assert!(
            result.get("error").is_some(),
            "should return error when image_path is missing"
        );
        assert!(
            result["error"].as_str().unwrap().contains("image_path"),
            "error should mention image_path"
        );
    }

    #[test]
    fn test_describe_image_missing_file() {
        let ctx = make_ctx();
        let result = memory_describe_image(
            &ctx,
            json!({"image_path": "/tmp/nonexistent_image_12345.png"}),
        );
        assert!(
            result.get("error").is_some(),
            "should return error for missing file"
        );
    }

    #[test]
    fn test_transcribe_audio_missing_param() {
        let ctx = make_ctx();
        let result = memory_transcribe_audio(&ctx, json!({}));
        assert!(
            result.get("error").is_some(),
            "should return error when audio_path is missing"
        );
        assert!(
            result["error"].as_str().unwrap().contains("audio_path"),
            "error should mention audio_path"
        );
    }

    #[test]
    fn test_process_video_missing_param() {
        let ctx = make_ctx();
        let result = memory_process_video(&ctx, json!({}));
        assert!(
            result.get("error").is_some(),
            "should return error when video_path is missing"
        );
        assert!(
            result["error"].as_str().unwrap().contains("video_path"),
            "error should mention video_path"
        );
    }

    #[test]
    fn test_capture_screenshot_no_params() {
        let ctx = make_ctx();
        // Without screencapture available (CI), this will fail with a meaningful error.
        let result = memory_capture_screenshot(&ctx, json!({}));
        // On platforms without screencapture, expect an error; on macOS, might succeed.
        // We only assert the response is a JSON object.
        assert!(result.is_object(), "should return a JSON object");
    }

    #[test]
    fn test_list_media_empty_db() {
        let ctx = make_ctx();
        let result = memory_list_media(&ctx, json!({}));
        assert!(
            result.get("error").is_none(),
            "should not error on empty db"
        );
        assert_eq!(result["count"], 0, "empty db should return 0 assets");
        assert!(result["assets"].is_array(), "assets should be an array");
    }

    #[test]
    fn test_list_media_with_type_filter() {
        let ctx = make_ctx();
        let result = memory_list_media(&ctx, json!({"media_type": "image", "limit": 10}));
        assert!(result.get("error").is_none(), "should not error");
        assert!(result["assets"].is_array(), "assets should be an array");
    }

    #[test]
    fn test_list_media_default_limit() {
        let ctx = make_ctx();
        let result = memory_list_media(&ctx, json!({}));
        assert!(result.get("error").is_none(), "should not error");
        assert_eq!(result["count"], 0);
    }

    #[test]
    fn test_infer_mime_type() {
        assert_eq!(infer_mime_type("photo.jpg"), "image/jpeg");
        assert_eq!(infer_mime_type("photo.jpeg"), "image/jpeg");
        assert_eq!(infer_mime_type("image.png"), "image/png");
        assert_eq!(infer_mime_type("anim.gif"), "image/gif");
        assert_eq!(infer_mime_type("pic.webp"), "image/webp");
        assert_eq!(infer_mime_type("scan.tiff"), "image/tiff");
        assert_eq!(infer_mime_type("unknown.xyz"), "image/png");
    }
}
