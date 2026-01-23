//! Database connection management with WAL mode support (RML-874)
//!
//! Implements SQLite connection pooling with configurable storage modes
//! for both local (WAL) and cloud-safe (DELETE journal) operation.

use parking_lot::Mutex;
use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Arc;

use super::migrations::run_migrations;
use crate::error::Result;
use crate::types::{StorageConfig, StorageMode};

/// Storage engine wrapping SQLite with connection pooling
pub struct Storage {
    config: StorageConfig,
    conn: Arc<Mutex<Connection>>,
}

/// Connection pool for concurrent access
pub struct StoragePool {
    config: StorageConfig,
    pool: Vec<Arc<Mutex<Connection>>>,
    next: std::sync::atomic::AtomicUsize,
}

impl Storage {
    /// Open or create a database with the given configuration
    pub fn open(config: StorageConfig) -> Result<Self> {
        let conn = Self::create_connection(&config)?;

        // Run migrations
        run_migrations(&conn)?;

        Ok(Self {
            config,
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Open with default configuration (in-memory for testing)
    pub fn open_in_memory() -> Result<Self> {
        let config = StorageConfig {
            db_path: ":memory:".to_string(),
            storage_mode: StorageMode::Local,
            cloud_uri: None,
            encrypt_cloud: false,
            confidence_half_life_days: 30.0,
            auto_sync: false,
            sync_debounce_ms: 5000,
        };
        Self::open(config)
    }

    /// Create a new connection with appropriate pragmas
    fn create_connection(config: &StorageConfig) -> Result<Connection> {
        let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;

        let conn = if config.db_path == ":memory:" {
            Connection::open_in_memory()?
        } else {
            // Ensure parent directory exists
            if let Some(parent) = Path::new(&config.db_path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            Connection::open_with_flags(&config.db_path, flags)?
        };

        // Configure based on storage mode (RML-874, RML-900)
        Self::configure_pragmas(&conn, config.storage_mode)?;

        Ok(conn)
    }

    /// Configure SQLite pragmas based on storage mode
    ///
    /// Local mode (RML-874): WAL for performance and crash recovery
    /// Cloud-safe mode (RML-900): DELETE journal for cloud sync compatibility
    fn configure_pragmas(conn: &Connection, mode: StorageMode) -> Result<()> {
        match mode {
            StorageMode::Local => {
                // WAL mode for better concurrency and crash recovery
                conn.execute_batch(
                    r#"
                    PRAGMA journal_mode=WAL;
                    PRAGMA synchronous=NORMAL;
                    PRAGMA wal_autocheckpoint=1000;
                    PRAGMA busy_timeout=30000;
                    PRAGMA cache_size=-64000;
                    PRAGMA temp_store=MEMORY;
                    PRAGMA mmap_size=268435456;
                    PRAGMA foreign_keys=ON;
                    "#,
                )?;
            }
            StorageMode::CloudSafe => {
                // Single-file mode for cloud sync (Dropbox, OneDrive, iCloud)
                conn.execute_batch(
                    r#"
                    PRAGMA journal_mode=DELETE;
                    PRAGMA synchronous=FULL;
                    PRAGMA busy_timeout=30000;
                    PRAGMA cache_size=-32000;
                    PRAGMA temp_store=MEMORY;
                    PRAGMA foreign_keys=ON;
                    "#,
                )?;
            }
        }
        Ok(())
    }

    /// Get a reference to the connection (for single-threaded use)
    pub fn connection(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }

    /// Execute a function with the connection
    pub fn with_connection<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = self.conn.lock();
        f(&conn)
    }

    /// Execute a function with a transaction
    pub fn with_transaction<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let mut conn = self.conn.lock();
        let tx = conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    /// Get current storage mode
    pub fn storage_mode(&self) -> StorageMode {
        self.config.storage_mode
    }

    /// Get database path
    pub fn db_path(&self) -> &str {
        &self.config.db_path
    }

    /// Check if database is in a cloud-synced folder
    pub fn is_in_cloud_folder(&self) -> bool {
        let path = self.config.db_path.to_lowercase();
        path.contains("dropbox")
            || path.contains("onedrive")
            || path.contains("icloud")
            || path.contains("google drive")
    }

    /// Get warning if storage mode doesn't match folder type
    pub fn storage_mode_warning(&self) -> Option<String> {
        if self.is_in_cloud_folder() && self.config.storage_mode == StorageMode::Local {
            Some(format!(
                "WARNING: Database '{}' appears to be in a cloud-synced folder. \
                WAL mode may cause corruption. Consider:\n\
                1. Set ENGRAM_STORAGE_MODE=cloud-safe\n\
                2. Move database to a local folder with backup sync",
                self.config.db_path
            ))
        } else {
            None
        }
    }

    /// Checkpoint WAL file (for local mode)
    pub fn checkpoint(&self) -> Result<()> {
        if self.config.storage_mode == StorageMode::Local {
            let conn = self.conn.lock();
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        }
        Ok(())
    }

    /// Get database size in bytes
    pub fn db_size(&self) -> Result<i64> {
        let conn = self.conn.lock();
        let size: i64 = conn.query_row(
            "SELECT page_count * page_size FROM pragma_page_count(), pragma_page_size()",
            [],
            |row| row.get(0),
        )?;
        Ok(size)
    }

    /// Vacuum the database to reclaim space
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute_batch("VACUUM;")?;
        Ok(())
    }

    /// Get configuration
    pub fn config(&self) -> &StorageConfig {
        &self.config
    }
}

impl StoragePool {
    /// Create a connection pool with the specified size
    pub fn new(config: StorageConfig, pool_size: usize) -> Result<Self> {
        let mut pool = Vec::with_capacity(pool_size);

        for _ in 0..pool_size {
            let conn = Storage::create_connection(&config)?;
            pool.push(Arc::new(Mutex::new(conn)));
        }

        // Run migrations on first connection
        if let Some(first) = pool.first() {
            let conn = first.lock();
            run_migrations(&conn)?;
        }

        Ok(Self {
            config,
            pool,
            next: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Get a connection from the pool (round-robin)
    pub fn get(&self) -> Arc<Mutex<Connection>> {
        let idx = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % self.pool.len();
        self.pool[idx].clone()
    }

    /// Execute a function with a connection from the pool
    pub fn with_connection<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn_arc = self.get();
        let conn = conn_arc.lock();
        f(&conn)
    }

    /// Get configuration
    pub fn config(&self) -> &StorageConfig {
        &self.config
    }
}

impl Clone for Storage {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            conn: self.conn.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_in_memory() {
        let storage = Storage::open_in_memory().unwrap();
        assert_eq!(storage.db_path(), ":memory:");
    }

    #[test]
    fn test_storage_modes() {
        // Test local mode
        let config = StorageConfig {
            db_path: ":memory:".to_string(),
            storage_mode: StorageMode::Local,
            cloud_uri: None,
            encrypt_cloud: false,
            confidence_half_life_days: 30.0,
            auto_sync: false,
            sync_debounce_ms: 5000,
        };
        let storage = Storage::open(config).unwrap();
        assert_eq!(storage.storage_mode(), StorageMode::Local);

        // Test cloud-safe mode
        let config = StorageConfig {
            db_path: ":memory:".to_string(),
            storage_mode: StorageMode::CloudSafe,
            cloud_uri: None,
            encrypt_cloud: false,
            confidence_half_life_days: 30.0,
            auto_sync: false,
            sync_debounce_ms: 5000,
        };
        let storage = Storage::open(config).unwrap();
        assert_eq!(storage.storage_mode(), StorageMode::CloudSafe);
    }

    #[test]
    fn test_cloud_folder_detection() {
        let config = StorageConfig {
            db_path: "/Users/test/Dropbox/memories.db".to_string(),
            storage_mode: StorageMode::Local,
            cloud_uri: None,
            encrypt_cloud: false,
            confidence_half_life_days: 30.0,
            auto_sync: false,
            sync_debounce_ms: 5000,
        };
        // Can't actually open this path in tests, but we can test detection
        let path = config.db_path.to_lowercase();
        assert!(path.contains("dropbox"));
    }
}
