//! Document ingestion handler.

use serde_json::{json, Value};

use super::HandlerContext;

pub fn ingest_document(ctx: &HandlerContext, params: Value) -> Value {
    use crate::intelligence::{DocumentFormat, DocumentIngestor, IngestConfig};
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct IngestParams {
        path: String,
        format: Option<String>,
        chunk_size: Option<usize>,
        overlap: Option<usize>,
        max_file_size: Option<u64>,
        tags: Option<Vec<String>>,
    }

    let input: IngestParams = match serde_json::from_value(params) {
        Ok(i) => i,
        Err(e) => return json!({"error": e.to_string()}),
    };

    let format = match input.format.as_deref() {
        None | Some("auto") => None,
        Some("md") | Some("markdown") => Some(DocumentFormat::Markdown),
        Some("pdf") => Some(DocumentFormat::Pdf),
        Some(other) => {
            return json!({"error": format!("Invalid format: {}", other)});
        }
    };

    let default_config = IngestConfig::default();
    let config = IngestConfig {
        format,
        chunk_size: input.chunk_size.unwrap_or(default_config.chunk_size),
        overlap: input.overlap.unwrap_or(default_config.overlap),
        max_file_size: input.max_file_size.unwrap_or(default_config.max_file_size),
        extra_tags: input.tags.unwrap_or_default(),
    };

    let ingestor = DocumentIngestor::new(&ctx.storage);
    match ingestor.ingest_file(&input.path, config) {
        Ok(result) => {
            // Phase L: best-effort attestation for the ingested document.
            #[cfg(feature = "agent-portability")]
            {
                use crate::attestation::AttestationChain;
                let chain = AttestationChain::new(ctx.storage.clone());
                let doc_name = std::path::Path::new(&input.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| input.path.clone());
                // Read file bytes for attestation (best-effort — skip on I/O error).
                if let Ok(bytes) = std::fs::read(&input.path) {
                    if let Err(e) =
                        chain.log_document(&bytes, &doc_name, None, &[], None)
                    {
                        tracing::warn!(
                            "Attestation hook (ingest_document): failed to log '{}': {}",
                            doc_name,
                            e
                        );
                    }
                }
            }
            json!(result)
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}
