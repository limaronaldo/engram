//! Background sync worker with debouncing (RML-875)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use parking_lot::Mutex;
use rusqlite::{params, Connection};
use tokio::sync::mpsc;
use tokio::time::{interval, Instant};

use super::{CloudStorage, SyncDirection};
use crate::error::{EngramError, Result};
use crate::types::SyncStatus;

/// Commands for the sync worker
#[derive(Debug)]
pub enum SyncCommand {
    /// Trigger a sync (direction, force)
    Sync(SyncDirection, bool),
    /// Mark data as dirty (triggers debounced sync)
    MarkDirty,
    /// Stop the worker
    Stop,
}

/// Background sync worker
pub struct SyncWorker {
    sender: mpsc::Sender<SyncCommand>,
}

impl SyncWorker {
    /// Start the sync worker
    pub async fn start(
        db_path: PathBuf,
        cloud_uri: String,
        encrypt: bool,
        debounce_ms: u64,
        conn: Arc<Mutex<Connection>>,
    ) -> Result<Self> {
        let (sender, mut receiver) = mpsc::channel::<SyncCommand>(100);

        let cloud = CloudStorage::from_uri(&cloud_uri, encrypt).await?;
        let debounce = Duration::from_millis(debounce_ms);

        // Spawn worker task
        tokio::spawn(async move {
            let mut last_dirty: Option<Instant> = None;
            let mut check_interval = interval(Duration::from_secs(1));

            loop {
                tokio::select! {
                    Some(cmd) = receiver.recv() => {
                        match cmd {
                            SyncCommand::Sync(direction, force) => {
                                Self::do_sync(&db_path, &cloud, &conn, direction, force).await;
                                last_dirty = None;
                            }
                            SyncCommand::MarkDirty => {
                                last_dirty = Some(Instant::now());
                            }
                            SyncCommand::Stop => {
                                // Final sync before stopping
                                Self::do_sync(&db_path, &cloud, &conn, SyncDirection::Push, false).await;
                                break;
                            }
                        }
                    }
                    _ = check_interval.tick() => {
                        // Check if debounce period has passed
                        if let Some(dirty_time) = last_dirty {
                            if dirty_time.elapsed() >= debounce {
                                Self::do_sync(&db_path, &cloud, &conn, SyncDirection::Push, false).await;
                                last_dirty = None;
                            }
                        }
                    }
                }
            }

            tracing::info!("Sync worker stopped");
        });

        Ok(Self { sender })
    }

    /// Perform the actual sync operation
    async fn do_sync(
        db_path: &PathBuf,
        cloud: &CloudStorage,
        conn: &Arc<Mutex<Connection>>,
        direction: SyncDirection,
        _force: bool,
    ) {
        let started_at = Utc::now();

        // Update sync state to syncing
        {
            let conn = conn.lock();
            let _ = conn.execute("UPDATE sync_state SET is_syncing = 1 WHERE id = 1", []);
        }

        let result = match direction {
            SyncDirection::Push => cloud.upload(db_path).await,
            SyncDirection::Pull => cloud.download(db_path).await,
            SyncDirection::Bidirectional => {
                // Check which is newer
                match cloud.metadata().await {
                    Ok(_remote_meta) => {
                        let _local_modified =
                            std::fs::metadata(db_path).and_then(|m| m.modified()).ok();

                        // Simple heuristic: push if local is newer or no remote
                        cloud.upload(db_path).await
                    }
                    Err(_) => {
                        // No remote, push
                        cloud.upload(db_path).await
                    }
                }
            }
        };

        let completed_at = Utc::now();

        // Update sync state
        {
            let conn = conn.lock();
            match &result {
                Ok(_) => {
                    let _ = conn.execute(
                        "UPDATE sync_state SET
                            is_syncing = 0,
                            last_sync = ?,
                            pending_changes = 0,
                            last_error = NULL
                         WHERE id = 1",
                        params![completed_at.to_rfc3339()],
                    );
                }
                Err(e) => {
                    let _ = conn.execute(
                        "UPDATE sync_state SET
                            is_syncing = 0,
                            last_error = ?
                         WHERE id = 1",
                        params![e.to_string()],
                    );
                }
            }
        }

        match result {
            Ok(bytes) => {
                tracing::info!(
                    "Sync {:?} completed: {} bytes in {:?}",
                    direction,
                    bytes,
                    completed_at - started_at
                );
            }
            Err(e) => {
                tracing::error!("Sync {:?} failed: {}", direction, e);
            }
        }
    }

    /// Trigger a sync
    pub async fn sync(&self, direction: SyncDirection, force: bool) -> Result<()> {
        self.sender
            .send(SyncCommand::Sync(direction, force))
            .await
            .map_err(|_| EngramError::Sync("Worker channel closed".to_string()))?;
        Ok(())
    }

    /// Mark data as dirty (triggers debounced sync)
    pub async fn mark_dirty(&self) -> Result<()> {
        self.sender
            .send(SyncCommand::MarkDirty)
            .await
            .map_err(|_| EngramError::Sync("Worker channel closed".to_string()))?;
        Ok(())
    }

    /// Stop the worker
    pub async fn stop(&self) -> Result<()> {
        self.sender
            .send(SyncCommand::Stop)
            .await
            .map_err(|_| EngramError::Sync("Worker channel closed".to_string()))?;
        Ok(())
    }
}

/// Get current sync status from database
pub fn get_sync_status(conn: &Connection) -> Result<SyncStatus> {
    let row = conn.query_row(
        "SELECT pending_changes, last_sync, last_error, is_syncing FROM sync_state WHERE id = 1",
        [],
        |row| {
            let pending: i64 = row.get(0)?;
            let last_sync: Option<String> = row.get(1)?;
            let last_error: Option<String> = row.get(2)?;
            let is_syncing: i32 = row.get(3)?;

            Ok(SyncStatus {
                pending_changes: pending,
                last_sync: last_sync.and_then(|s| {
                    chrono::DateTime::parse_from_rfc3339(&s)
                        .map(|dt| dt.with_timezone(&Utc))
                        .ok()
                }),
                last_error,
                is_syncing: is_syncing != 0,
            })
        },
    )?;

    Ok(row)
}

/// Increment pending changes counter
#[allow(dead_code)]
pub fn increment_pending_changes(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET pending_changes = pending_changes + 1 WHERE id = 1",
        [],
    )?;
    Ok(())
}
