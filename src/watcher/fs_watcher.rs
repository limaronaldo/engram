//! File system watcher module
//!
//! Monitors configured directories for file changes and invokes a callback with
//! [`FileEvent`] values that callers use to create Engram memories.
//!
//! # Usage
//!
//! ```rust,ignore
//! use engram::watcher::{FileWatcherConfig, FsWatcher};
//! use std::path::PathBuf;
//!
//! let config = FileWatcherConfig {
//!     enabled: true,
//!     paths: vec![PathBuf::from("/tmp/notes")],
//!     extensions: vec!["md".to_string(), "txt".to_string()],
//!     debounce_ms: 500,
//!     ignore_patterns: vec![".git".to_string()],
//! };
//!
//! let (watcher, stop_tx) = FsWatcher::new(config, |event| {
//!     println!("File event: {:?}", event);
//! }).expect("failed to create watcher");
//!
//! // Run in a dedicated thread
//! let handle = std::thread::spawn(move || watcher.run());
//!
//! // Signal shutdown
//! stop_tx.send(()).ok();
//! handle.join().ok();
//! ```

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    time::{Duration, Instant},
};

use notify::{Event, EventKind, RecursiveMode, Watcher};
use tracing::{debug, error, warn};

use super::config::FileWatcherConfig;
use crate::error::{EngramError, Result};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// The kind of change that was detected on a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// A file was created.
    Created,
    /// A file was modified.
    Modified,
    /// A file was deleted.
    Deleted,
}

impl std::fmt::Display for ChangeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChangeKind::Created => write!(f, "created"),
            ChangeKind::Modified => write!(f, "modified"),
            ChangeKind::Deleted => write!(f, "deleted"),
        }
    }
}

/// A filesystem change event delivered to the callback.
#[derive(Debug, Clone)]
pub struct FileEvent {
    /// Absolute path to the changed file.
    pub path: PathBuf,
    /// Nature of the change.
    pub kind: ChangeKind,
    /// RFC3339 UTC timestamp when the event was detected.
    pub timestamp: String,
}

impl FileEvent {
    /// Build the memory content string for this event.
    ///
    /// Format: `"File {kind}: {path} at {timestamp}"`
    pub fn to_memory_content(&self) -> String {
        format!(
            "File {}: {} at {}",
            self.kind,
            self.path.display(),
            self.timestamp
        )
    }
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// A pending (debounced) event waiting to fire.
#[derive(Debug)]
struct PendingEvent {
    kind: ChangeKind,
    earliest_fire_at: Instant,
}

// ---------------------------------------------------------------------------
// FsWatcher
// ---------------------------------------------------------------------------

/// File system watcher that monitors directories for changes.
///
/// Create with [`FsWatcher::new`], then call [`FsWatcher::run`] in a dedicated
/// thread.  Send `()` on the stop channel to shut down gracefully.
pub struct FsWatcher<F>
where
    F: Fn(FileEvent) + Send + 'static,
{
    config: FileWatcherConfig,
    callback: F,
    stop_rx: mpsc::Receiver<()>,
    event_rx: mpsc::Receiver<notify::Result<Event>>,
    /// Keep the underlying watcher alive for its full lifetime.
    _watcher: Box<dyn Watcher + Send>,
}

impl<F> FsWatcher<F>
where
    F: Fn(FileEvent) + Send + 'static,
{
    /// Create a new `FsWatcher`.
    ///
    /// Returns `(watcher, stop_tx)`.  Call [`FsWatcher::run`] (usually in a
    /// dedicated thread) and drop `stop_tx` or send `()` to initiate graceful
    /// shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying `notify` watcher cannot be created or
    /// if any of the configured paths cannot be watched.
    pub fn new(config: FileWatcherConfig, callback: F) -> Result<(Self, mpsc::SyncSender<()>)> {
        let (event_tx, event_rx) = mpsc::channel::<notify::Result<Event>>();

        let mut watcher = notify::recommended_watcher(move |res| {
            // Ignore send errors — they only occur after the receiver has been
            // dropped (i.e. after FsWatcher is shut down).
            let _ = event_tx.send(res);
        })
        .map_err(|e| EngramError::Config(format!("Cannot create filesystem watcher: {e}")))?;

        for path in &config.paths {
            if !path.exists() {
                warn!(path = ?path, "Watched path does not exist; skipping");
                continue;
            }
            watcher
                .watch(path, RecursiveMode::Recursive)
                .map_err(|e| EngramError::Config(format!("Cannot watch path {:?}: {e}", path)))?;

            debug!(path = ?path, "Watching path");
        }

        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);

        let fs_watcher = Self {
            config,
            callback,
            stop_rx,
            event_rx,
            _watcher: Box::new(watcher),
        };

        Ok((fs_watcher, stop_tx))
    }

    /// Run the watcher event loop until a stop signal is received.
    ///
    /// This method blocks the calling thread.  Run it in a dedicated
    /// `std::thread::spawn` call.
    pub fn run(self) {
        if !self.config.enabled {
            debug!("File watcher is disabled; exiting immediately");
            return;
        }

        let debounce = Duration::from_millis(self.config.debounce_ms);
        // path → pending change
        let mut pending: HashMap<PathBuf, PendingEvent> = HashMap::new();

        loop {
            // Wait for the next event (or until the next pending event is due).
            let recv_timeout = Self::next_fire_delay(&pending, debounce)
                .unwrap_or_else(|| Duration::from_millis(50));

            match self.event_rx.recv_timeout(recv_timeout) {
                Ok(Ok(event)) => {
                    self.handle_raw_event(event, debounce, &mut pending);
                }
                Ok(Err(e)) => {
                    error!(error = %e, "Notify watcher error");
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    // Fall through to flush pending events.
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    debug!("Event channel disconnected; shutting down");
                    break;
                }
            }

            // Deliver any debounced events whose deadline has passed.
            self.flush_pending(&mut pending);

            // Check for stop signal (non-blocking).
            match self.stop_rx.try_recv() {
                Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {
                    debug!("Stop signal received; shutting down file watcher");
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    /// Convert a raw `notify` event into a pending debounced entry.
    fn handle_raw_event(
        &self,
        event: Event,
        debounce: Duration,
        pending: &mut HashMap<PathBuf, PendingEvent>,
    ) {
        let kind = match classify_event_kind(&event.kind) {
            Some(k) => k,
            None => return,
        };

        for path in &event.paths {
            if !self.should_watch(path) {
                continue;
            }

            let fire_at = Instant::now() + debounce;

            pending
                .entry(path.clone())
                .and_modify(|p| {
                    // Keep the highest-priority change kind.
                    if kind_priority(&kind) > kind_priority(&p.kind) {
                        p.kind = kind.clone();
                    }
                    // Always push the deadline forward so rapid events stay debounced.
                    p.earliest_fire_at = fire_at;
                })
                .or_insert(PendingEvent {
                    kind: kind.clone(),
                    earliest_fire_at: fire_at,
                });
        }
    }

    /// Fire all pending events whose deadline has passed.
    fn flush_pending(&self, pending: &mut HashMap<PathBuf, PendingEvent>) {
        let now = Instant::now();
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, p)| now >= p.earliest_fire_at)
            .map(|(path, _)| path.clone())
            .collect();

        for path in ready {
            if let Some(p) = pending.remove(&path) {
                let event = FileEvent {
                    path,
                    kind: p.kind,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                debug!(path = ?event.path, kind = ?event.kind, "Firing debounced file event");
                (self.callback)(event);
            }
        }
    }

    /// Duration until the next pending event fires, capped at one debounce interval.
    fn next_fire_delay(
        pending: &HashMap<PathBuf, PendingEvent>,
        debounce: Duration,
    ) -> Option<Duration> {
        pending
            .values()
            .map(|p| p.earliest_fire_at)
            .min()
            .map(|earliest| {
                let now = Instant::now();
                if earliest > now {
                    (earliest - now).min(debounce)
                } else {
                    Duration::ZERO
                }
            })
    }

    /// Returns `true` if this path should generate an event.
    ///
    /// Checks the extension filter (empty = watch all) and then the ignore
    /// patterns (simple substring match against the full path string).
    pub(crate) fn should_watch(&self, path: &Path) -> bool {
        // Extension filter
        if !self.config.extensions.is_empty() {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !self.config.extensions.iter().any(|e| e == ext) {
                return false;
            }
        }

        // Ignore patterns
        let path_str = path.to_string_lossy();
        for pattern in &self.config.ignore_patterns {
            if path_str.contains(pattern.as_str()) {
                return false;
            }
        }

        true
    }
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

/// Map `notify::EventKind` to our simpler [`ChangeKind`].
pub(crate) fn classify_event_kind(kind: &EventKind) -> Option<ChangeKind> {
    match kind {
        EventKind::Create(_) => Some(ChangeKind::Created),
        EventKind::Modify(_) => Some(ChangeKind::Modified),
        EventKind::Remove(_) => Some(ChangeKind::Deleted),
        _ => None,
    }
}

/// Higher number = higher priority (kept in the pending map).
fn kind_priority(kind: &ChangeKind) -> u8 {
    match kind {
        ChangeKind::Deleted => 3,
        ChangeKind::Created => 2,
        ChangeKind::Modified => 1,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{
        event::{CreateKind, ModifyKind, RemoveKind},
        Config as NotifyConfig, NullWatcher,
    };

    // ---- Helpers -----------------------------------------------------------

    /// Build an FsWatcher with a NullWatcher backend (no real FS watching).
    ///
    /// The stop channel is dropped immediately so `run()` exits on the first
    /// check.  The event channel is also closed so the loop exits fast.
    fn make_test_watcher(
        config: FileWatcherConfig,
    ) -> FsWatcher<impl Fn(FileEvent) + Send + 'static> {
        let (_event_tx, event_rx) = mpsc::channel::<notify::Result<Event>>();
        let (_stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);

        let null_watcher = NullWatcher::new(|_: notify::Result<Event>| {}, NotifyConfig::default())
            .expect("NullWatcher should always succeed");

        FsWatcher {
            config,
            callback: |_: FileEvent| {},
            stop_rx,
            event_rx,
            _watcher: Box::new(null_watcher),
        }
    }

    fn config_with(extensions: Vec<&str>, ignore: Vec<&str>) -> FileWatcherConfig {
        FileWatcherConfig {
            enabled: true,
            paths: Vec::new(),
            extensions: extensions.into_iter().map(String::from).collect(),
            debounce_ms: 50,
            ignore_patterns: ignore.into_iter().map(String::from).collect(),
        }
    }

    // ---- classify_event_kind -----------------------------------------------

    #[test]
    fn test_classify_create_event() {
        let kind = EventKind::Create(CreateKind::File);
        assert_eq!(classify_event_kind(&kind), Some(ChangeKind::Created));
    }

    #[test]
    fn test_classify_modify_event() {
        let kind = EventKind::Modify(ModifyKind::Any);
        assert_eq!(classify_event_kind(&kind), Some(ChangeKind::Modified));
    }

    #[test]
    fn test_classify_remove_event() {
        let kind = EventKind::Remove(RemoveKind::File);
        assert_eq!(classify_event_kind(&kind), Some(ChangeKind::Deleted));
    }

    #[test]
    fn test_classify_access_event_returns_none() {
        let kind = EventKind::Access(notify::event::AccessKind::Any);
        assert!(classify_event_kind(&kind).is_none());
    }

    #[test]
    fn test_classify_other_event_returns_none() {
        assert!(classify_event_kind(&EventKind::Other).is_none());
    }

    // ---- Extension filter --------------------------------------------------

    #[test]
    fn test_extension_filter_passes_matching_extension() {
        let w = make_test_watcher(config_with(vec!["rs", "md"], vec![]));
        assert!(w.should_watch(Path::new("/home/user/notes/README.md")));
        assert!(w.should_watch(Path::new("/project/src/main.rs")));
    }

    #[test]
    fn test_extension_filter_rejects_non_matching_extension() {
        let w = make_test_watcher(config_with(vec!["rs", "md"], vec![]));
        assert!(!w.should_watch(Path::new("/project/image.png")));
        assert!(!w.should_watch(Path::new("/project/data.json")));
    }

    #[test]
    fn test_empty_extension_list_passes_all() {
        let w = make_test_watcher(config_with(vec![], vec![]));
        assert!(w.should_watch(Path::new("/anything/file.xyz")));
        assert!(w.should_watch(Path::new("/no-extension")));
    }

    // ---- Ignore patterns ---------------------------------------------------

    #[test]
    fn test_ignore_pattern_rejects_matching_path() {
        let w = make_test_watcher(config_with(vec![], vec![".git", "node_modules"]));
        assert!(!w.should_watch(Path::new("/project/.git/config")));
        assert!(!w.should_watch(Path::new("/project/node_modules/lodash/index.js")));
    }

    #[test]
    fn test_ignore_pattern_passes_non_matching_path() {
        let w = make_test_watcher(config_with(vec![], vec![".git"]));
        assert!(w.should_watch(Path::new("/project/src/main.rs")));
    }

    #[test]
    fn test_extension_and_ignore_combined() {
        let w = make_test_watcher(config_with(vec!["rs"], vec!["target"]));
        // Good: right extension, not in ignored dir
        assert!(w.should_watch(Path::new("/project/src/lib.rs")));
        // Bad: right extension but under ignored dir
        assert!(!w.should_watch(Path::new("/project/target/debug/build/foo.rs")));
        // Bad: wrong extension
        assert!(!w.should_watch(Path::new("/project/src/style.css")));
    }

    // ---- FileEvent --------------------------------------------------------

    #[test]
    fn test_file_event_to_memory_content() {
        let event = FileEvent {
            path: PathBuf::from("/home/user/notes/README.md"),
            kind: ChangeKind::Modified,
            timestamp: "2026-03-09T00:00:00Z".to_string(),
        };
        let content = event.to_memory_content();
        assert!(content.contains("modified"), "content: {content}");
        assert!(content.contains("README.md"), "content: {content}");
        assert!(
            content.contains("2026-03-09T00:00:00Z"),
            "content: {content}"
        );
    }

    #[test]
    fn test_change_kind_display() {
        assert_eq!(ChangeKind::Created.to_string(), "created");
        assert_eq!(ChangeKind::Modified.to_string(), "modified");
        assert_eq!(ChangeKind::Deleted.to_string(), "deleted");
    }

    // ---- Debounce helpers -------------------------------------------------

    #[test]
    fn test_next_fire_delay_empty_returns_none() {
        let pending: HashMap<PathBuf, PendingEvent> = HashMap::new();
        assert!(
            FsWatcher::<fn(FileEvent)>::next_fire_delay(&pending, Duration::from_millis(500))
                .is_none()
        );
    }

    #[test]
    fn test_next_fire_delay_with_entry_returns_some_bounded_by_debounce() {
        let debounce = Duration::from_millis(500);
        let mut pending: HashMap<PathBuf, PendingEvent> = HashMap::new();
        pending.insert(
            PathBuf::from("/tmp/file.txt"),
            PendingEvent {
                kind: ChangeKind::Modified,
                earliest_fire_at: Instant::now() + Duration::from_millis(200),
            },
        );
        let delay = FsWatcher::<fn(FileEvent)>::next_fire_delay(&pending, debounce)
            .expect("should be Some");
        assert!(
            delay <= debounce,
            "delay {delay:?} should be <= debounce {debounce:?}"
        );
    }

    // ---- Disabled watcher -------------------------------------------------

    #[test]
    fn test_disabled_watcher_run_returns_immediately() {
        let (_event_tx, event_rx) = mpsc::channel::<notify::Result<Event>>();
        let (_stop_tx, stop_rx) = mpsc::sync_channel::<()>(1);

        let null_watcher = NullWatcher::new(|_: notify::Result<Event>| {}, NotifyConfig::default())
            .expect("NullWatcher should always succeed");

        let watcher = FsWatcher {
            config: FileWatcherConfig {
                enabled: false,
                ..FileWatcherConfig::default()
            },
            callback: |_: FileEvent| {},
            stop_rx,
            event_rx,
            _watcher: Box::new(null_watcher),
        };

        let handle = std::thread::spawn(move || watcher.run());
        handle
            .join()
            .expect("disabled watcher thread should not panic");
    }
}
