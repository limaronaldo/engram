use super::{MeilisearchBackend, SqliteBackend, StorageBackend};
use crate::error::EngramError;
use crate::types::{ListOptions, SortField, SortOrder};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use tokio::time::{sleep, Duration};
use tracing::{error, info};

pub struct MeilisearchIndexer {
    sqlite: Arc<SqliteBackend>,
    meilisearch: Arc<MeilisearchBackend>,
    sync_interval: Duration,
    last_sync_timestamp: AtomicI64,
}

impl MeilisearchIndexer {
    pub fn new(
        sqlite: Arc<SqliteBackend>,
        meilisearch: Arc<MeilisearchBackend>,
        sync_interval_secs: u64,
    ) -> Self {
        Self {
            sqlite,
            meilisearch,
            sync_interval: Duration::from_secs(sync_interval_secs),
            last_sync_timestamp: AtomicI64::new(0),
        }
    }

    pub async fn start(self: Arc<Self>) {
        info!("Starting Meilisearch indexer Service");

        // Initial sync on startup (could be optimized to check state)
        if let Err(e) = self.run_full_sync().await {
            error!("Initial full sync failed: {}", e);
        }

        loop {
            sleep(self.sync_interval).await;
            if let Err(e) = self.run_incremental_sync().await {
                error!("Incremental sync failed: {}", e);
            }
        }
    }

    pub async fn run_full_sync(&self) -> Result<(), EngramError> {
        info!("Running full sync from SQLite to Meilisearch...");

        // 1. Get all memories from SQLite
        // Using pagination to avoid OOM
        let mut offset = 0;
        let limit = 100;
        let run_started_at = chrono::Utc::now().timestamp();

        loop {
            // Need to spawn blocking because SqliteBackend is synchronous
            let sqlite = self.sqlite.clone();
            let memories = tokio::task::spawn_blocking(move || {
                sqlite.list_memories(ListOptions {
                    limit: Some(limit),
                    offset: Some(offset),
                    include_archived: true,
                    sort_by: Some(SortField::UpdatedAt),
                    sort_order: Some(SortOrder::Desc),
                    ..Default::default()
                })
            })
            .await
            .map_err(|e| EngramError::Internal(e.to_string()))??;

            if memories.is_empty() {
                break;
            }

            let count = memories.len();

            // 2. Batched index to Meilisearch
            self.meilisearch.index_memories(&memories)?;

            offset += limit;
            info!("Synced {} memories...", offset);

            if count < limit as usize {
                break;
            }
        }

        self.last_sync_timestamp
            .store(run_started_at, Ordering::Relaxed);
        info!("Full sync complete.");
        Ok(())
    }

    async fn run_incremental_sync(&self) -> Result<(), EngramError> {
        let last_sync = self.last_sync_timestamp.load(Ordering::Relaxed);
        if last_sync == 0 {
            return self.run_full_sync().await;
        }

        let run_started_at = chrono::Utc::now().timestamp();
        let mut offset = 0;
        let limit = 100;
        let mut should_stop = false;

        loop {
            let sqlite = self.sqlite.clone();
            let memories = tokio::task::spawn_blocking(move || {
                sqlite.list_memories(ListOptions {
                    limit: Some(limit),
                    offset: Some(offset),
                    include_archived: true,
                    sort_by: Some(SortField::UpdatedAt),
                    sort_order: Some(SortOrder::Desc),
                    ..Default::default()
                })
            })
            .await
            .map_err(|e| EngramError::Internal(e.to_string()))??;

            if memories.is_empty() {
                break;
            }

            let mut batch = Vec::new();
            for memory in memories {
                if memory.updated_at.timestamp() <= last_sync {
                    should_stop = true;
                    break;
                }
                batch.push(memory);
            }

            if !batch.is_empty() {
                self.meilisearch.index_memories(&batch)?;
            }

            if should_stop {
                break;
            }

            offset += limit;
        }

        self.last_sync_timestamp
            .store(run_started_at, Ordering::Relaxed);
        Ok(())
    }
}
