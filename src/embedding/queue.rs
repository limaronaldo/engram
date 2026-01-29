//! Async embedding queue with batch processing (RML-873)
//!
//! Embeddings are computed in the background to avoid blocking writes.
//! The queue supports batching for efficient API usage.

use async_channel::{bounded, Receiver, Sender};
use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

use super::{create_embedder, Embedder};
use crate::error::{EngramError, Result};
use crate::types::{EmbeddingConfig, EmbeddingState, EmbeddingStatus, MemoryId};

/// Message for the embedding queue
#[derive(Debug)]
pub struct EmbeddingRequest {
    pub memory_id: MemoryId,
    pub content: String,
}

/// Embedding queue for async processing
pub struct EmbeddingQueue {
    sender: Sender<EmbeddingRequest>,
    receiver: Receiver<EmbeddingRequest>,
    batch_size: usize,
}

impl EmbeddingQueue {
    /// Create a new embedding queue
    pub fn new(batch_size: usize) -> Self {
        let (sender, receiver) = bounded(10000); // Buffer up to 10k requests
        Self {
            sender,
            receiver,
            batch_size,
        }
    }

    /// Queue a memory for embedding
    pub async fn queue(&self, memory_id: MemoryId, content: String) -> Result<()> {
        self.sender
            .send(EmbeddingRequest { memory_id, content })
            .await
            .map_err(|e| EngramError::Embedding(format!("Queue send error: {}", e)))?;
        Ok(())
    }

    /// Queue a memory (blocking version for sync contexts)
    pub fn queue_blocking(&self, memory_id: MemoryId, content: String) -> Result<()> {
        self.sender
            .send_blocking(EmbeddingRequest { memory_id, content })
            .map_err(|e| EngramError::Embedding(format!("Queue send error: {}", e)))?;
        Ok(())
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.receiver.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.receiver.is_empty()
    }

    /// Get receiver for worker
    pub fn receiver(&self) -> Receiver<EmbeddingRequest> {
        self.receiver.clone()
    }
}

impl Clone for EmbeddingQueue {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            batch_size: self.batch_size,
        }
    }
}

/// Background worker for processing embeddings
pub struct EmbeddingWorker {
    embedder: Arc<dyn Embedder>,
    queue: EmbeddingQueue,
    conn: Arc<Mutex<Connection>>,
    batch_size: usize,
    batch_timeout: Duration,
}

impl EmbeddingWorker {
    /// Create a new embedding worker
    pub fn new(
        config: EmbeddingConfig,
        queue: EmbeddingQueue,
        conn: Arc<Mutex<Connection>>,
    ) -> Result<Self> {
        let embedder = create_embedder(&config)?;
        let batch_size = config.batch_size;

        Ok(Self {
            embedder,
            queue,
            conn,
            batch_size,
            batch_timeout: Duration::from_secs(5),
        })
    }

    /// Run the worker (call in a spawned task)
    pub async fn run(&self) {
        let receiver = self.queue.receiver();
        let mut batch: Vec<EmbeddingRequest> = Vec::with_capacity(self.batch_size);
        let mut batch_timer = interval(self.batch_timeout);

        loop {
            tokio::select! {
                // Receive new request
                Ok(request) = receiver.recv() => {
                    batch.push(request);

                    // Process if batch is full
                    if batch.len() >= self.batch_size {
                        self.process_batch(&mut batch).await;
                    }
                }

                // Process on timeout even if batch isn't full
                _ = batch_timer.tick() => {
                    if !batch.is_empty() {
                        self.process_batch(&mut batch).await;
                    }
                }
            }
        }
    }

    /// Process a batch of embedding requests
    async fn process_batch(&self, batch: &mut Vec<EmbeddingRequest>) {
        if batch.is_empty() {
            return;
        }

        let memory_ids: Vec<MemoryId> = batch.iter().map(|r| r.memory_id).collect();
        let contents: Vec<&str> = batch.iter().map(|r| r.content.as_str()).collect();

        // Mark as processing
        {
            let conn = self.conn.lock();
            let now = Utc::now().to_rfc3339();
            for &id in &memory_ids {
                let _ = conn.execute(
                    "UPDATE embedding_queue SET status = 'processing', started_at = ? WHERE memory_id = ?",
                    params![now, id],
                );
            }
        }

        // Generate embeddings
        match self.embedder.embed_batch(&contents) {
            Ok(embeddings) => {
                let conn = self.conn.lock();
                let now = Utc::now().to_rfc3339();
                let model = self.embedder.model_name();
                let dimensions = self.embedder.dimensions();

                for (id, embedding) in memory_ids.iter().zip(embeddings.iter()) {
                    // Serialize embedding to bytes
                    let embedding_bytes: Vec<u8> =
                        embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

                    // Store embedding
                    let _ = conn.execute(
                        "INSERT OR REPLACE INTO embeddings (memory_id, embedding, model, dimensions, created_at)
                         VALUES (?, ?, ?, ?, ?)",
                        params![id, embedding_bytes, model, dimensions, now],
                    );

                    // Update memory
                    let _ = conn.execute(
                        "UPDATE memories SET has_embedding = 1 WHERE id = ?",
                        params![id],
                    );

                    // Mark as complete
                    let _ = conn.execute(
                        "UPDATE embedding_queue SET status = 'complete', completed_at = ? WHERE memory_id = ?",
                        params![now, id],
                    );
                }

                tracing::info!("Processed {} embeddings", memory_ids.len());
            }
            Err(e) => {
                let conn = self.conn.lock();
                let error_time = Utc::now().to_rfc3339();
                let error_msg = e.to_string();
                let _ = error_time; // suppress unused warning

                for &id in &memory_ids {
                    let _ = conn.execute(
                        "UPDATE embedding_queue SET status = 'failed', error = ?, retry_count = retry_count + 1 WHERE memory_id = ?",
                        params![error_msg, id],
                    );
                }

                tracing::error!("Embedding batch failed: {}", e);
            }
        }

        batch.clear();
    }
}

/// Get embedding status for a memory
pub fn get_embedding_status(conn: &Connection, memory_id: MemoryId) -> Result<EmbeddingStatus> {
    let row = conn.query_row(
        "SELECT status, queued_at, completed_at, error FROM embedding_queue WHERE memory_id = ?",
        params![memory_id],
        |row| {
            let status_str: String = row.get(0)?;
            let queued_at: Option<String> = row.get(1)?;
            let completed_at: Option<String> = row.get(2)?;
            let error: Option<String> = row.get(3)?;

            let status = match status_str.as_str() {
                "pending" => EmbeddingState::Pending,
                "processing" => EmbeddingState::Processing,
                "complete" => EmbeddingState::Complete,
                "failed" => EmbeddingState::Failed,
                _ => EmbeddingState::Pending,
            };

            Ok(EmbeddingStatus {
                memory_id,
                status,
                queued_at: queued_at.and_then(|s| {
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                completed_at: completed_at.and_then(|s| {
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                error,
            })
        },
    );

    match row {
        Ok(status) => Ok(status),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Check if memory has embedding
            let has_embedding: bool = conn
                .query_row(
                    "SELECT has_embedding FROM memories WHERE id = ?",
                    params![memory_id],
                    |row| row.get(0),
                )
                .unwrap_or(false);

            Ok(EmbeddingStatus {
                memory_id,
                status: if has_embedding {
                    EmbeddingState::Complete
                } else {
                    EmbeddingState::Pending
                },
                queued_at: None,
                completed_at: None,
                error: None,
            })
        }
        Err(e) => Err(EngramError::Database(e)),
    }
}

/// Get embedding for a memory
pub fn get_embedding(conn: &Connection, memory_id: MemoryId) -> Result<Option<Vec<f32>>> {
    let row = conn.query_row(
        "SELECT embedding, dimensions FROM embeddings WHERE memory_id = ?",
        params![memory_id],
        |row| {
            let bytes: Vec<u8> = row.get(0)?;
            let dimensions: usize = row.get(1)?;
            Ok((bytes, dimensions))
        },
    );

    match row {
        Ok((bytes, dimensions)) => {
            let expected_len = dimensions.checked_mul(4).ok_or_else(|| {
                EngramError::InvalidInput("Embedding dimensions too large".to_string())
            })?;
            if bytes.len() != expected_len {
                return Err(EngramError::InvalidInput(format!(
                    "Embedding byte length {} does not match dimensions {}",
                    bytes.len(),
                    dimensions
                )));
            }

            // Deserialize from bytes
            let mut embedding = Vec::with_capacity(dimensions);
            for chunk in bytes.chunks_exact(4) {
                let arr: [u8; 4] = chunk.try_into().unwrap();
                embedding.push(f32::from_le_bytes(arr));
            }
            Ok(Some(embedding))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(EngramError::Database(e)),
    }
}

/// Retry failed embeddings
#[allow(dead_code)]
pub fn retry_failed_embeddings(conn: &Connection, max_retries: i32) -> Result<Vec<MemoryId>> {
    let mut stmt = conn.prepare(
        "SELECT eq.memory_id, m.content FROM embedding_queue eq
         JOIN memories m ON eq.memory_id = m.id
         WHERE eq.status = 'failed' AND eq.retry_count < ?",
    )?;

    let failed: Vec<(MemoryId, String)> = stmt
        .query_map([max_retries], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|r| r.ok())
        .collect();

    let ids: Vec<MemoryId> = failed.iter().map(|(id, _)| *id).collect();

    // Reset status to pending
    for &id in &ids {
        conn.execute(
            "UPDATE embedding_queue SET status = 'pending', error = NULL WHERE memory_id = ?",
            params![id],
        )?;
    }

    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::queries::create_memory;
    use crate::storage::Storage;
    use crate::types::{CreateMemoryInput, MemoryType};

    #[tokio::test]
    async fn test_embedding_queue() {
        let queue = EmbeddingQueue::new(10);

        queue.queue(1, "Hello world".to_string()).await.unwrap();
        queue.queue(2, "Test content".to_string()).await.unwrap();

        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn test_get_embedding_length_mismatch() {
        let storage = Storage::open_in_memory().unwrap();

        storage
            .with_connection(|conn| {
                let memory = create_memory(
                    conn,
                    &CreateMemoryInput {
                        content: "Test embedding".to_string(),
                        memory_type: MemoryType::Note,
                        tags: vec![],
                        metadata: std::collections::HashMap::new(),
                        importance: None,
                        scope: Default::default(),
                        workspace: None,
                        tier: Default::default(),
                        defer_embedding: true,
                        ttl_seconds: None,
                        dedup_mode: Default::default(),
                        dedup_threshold: None,
                    },
                )?;

                // Insert embedding with incorrect byte length (dimensions=2 => expected 8 bytes)
                conn.execute(
                    "INSERT INTO embeddings (memory_id, embedding, model, dimensions, created_at)
                     VALUES (?, ?, ?, ?, datetime('now'))",
                    params![memory.id, vec![0u8; 4], "test", 2],
                )?;

                match get_embedding(conn, memory.id) {
                    Err(EngramError::InvalidInput(_)) => Ok(()),
                    Err(e) => Err(e),
                    Ok(_) => Err(EngramError::Internal(
                        "Expected embedding length mismatch error".to_string(),
                    )),
                }
            })
            .unwrap();
    }
}
