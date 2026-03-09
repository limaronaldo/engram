//! Integration tests for the Engram watcher modules.
//!
//! Verifies that the file system watcher, app focus watcher, and configuration
//! modules work correctly together including real filesystem interaction via
//! temporary directories.
//!
//! Run with: cargo test --features watcher --test watcher_integration

#![cfg(feature = "watcher")]

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use engram::watcher::{
    app_focus::{AppFocusWatcher, AppNameProvider, FocusEvent},
    config::{AppFocusConfig, BrowserWatcherConfig, FileWatcherConfig, WatcherConfig},
    fs_watcher::{ChangeKind, FileEvent, FsWatcher},
};

// ============================================================================
// Helpers
// ============================================================================

/// A mock [`AppNameProvider`] backed by a queue of names to return in order.
/// `None` entries simulate OS-level failures.
struct MockAppRunner {
    queue: Arc<Mutex<VecDeque<Option<String>>>>,
}

impl MockAppRunner {
    fn from_names(names: &[Option<&str>]) -> Self {
        let deque = names
            .iter()
            .map(|n| n.map(|s| s.to_string()))
            .collect();
        Self {
            queue: Arc::new(Mutex::new(deque)),
        }
    }
}

impl AppNameProvider for MockAppRunner {
    fn current_app(&self) -> Option<String> {
        self.queue.lock().unwrap().pop_front().flatten()
    }
}

fn app_focus_config_enabled() -> AppFocusConfig {
    AppFocusConfig {
        enabled: true,
        poll_interval_secs: 5,
        min_focus_secs: 0,
        exclude_apps: Vec::new(),
    }
}

// ============================================================================
// Test 1: FsWatcher detects file creation in a temp directory
// ============================================================================

#[test]
fn test_fs_watcher_detects_file_creation() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let watched_path = dir.path().to_path_buf();

    let config = FileWatcherConfig {
        enabled: true,
        paths: vec![watched_path.clone()],
        extensions: Vec::new(), // watch everything
        debounce_ms: 50,
        ignore_patterns: Vec::new(),
    };

    let received: Arc<Mutex<Vec<FileEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    let (watcher, stop_tx) =
        FsWatcher::new(config, move |event| {
            received_clone.lock().unwrap().push(event);
        })
        .expect("failed to create FsWatcher");

    let handle = std::thread::spawn(move || watcher.run());

    // Give the watcher a moment to start watching before creating a file.
    std::thread::sleep(Duration::from_millis(100));

    let new_file = watched_path.join("hello.txt");
    std::fs::write(&new_file, "engram watcher test").expect("failed to write test file");

    // Wait long enough for the debounce interval to fire (50 ms) plus margin.
    std::thread::sleep(Duration::from_millis(300));

    // Signal shutdown.
    stop_tx.send(()).ok();
    handle.join().expect("watcher thread should not panic");

    let events = received.lock().unwrap();
    assert!(
        !events.is_empty(),
        "expected at least one FileEvent for newly created file"
    );

    let found = events.iter().any(|e| {
        e.path.file_name().and_then(|n| n.to_str()) == Some("hello.txt")
            && e.kind == ChangeKind::Created
    });
    assert!(
        found,
        "expected a Created event for hello.txt, got: {:?}",
        events
            .iter()
            .map(|e| (&e.path, &e.kind))
            .collect::<Vec<_>>()
    );
}

// ============================================================================
// Test 2: FsWatcher respects extension filtering
// ============================================================================

#[test]
fn test_fs_watcher_extension_filtering() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let watched_path = dir.path().to_path_buf();

    let config = FileWatcherConfig {
        enabled: true,
        paths: vec![watched_path.clone()],
        extensions: vec!["md".to_string(), "txt".to_string()],
        debounce_ms: 50,
        ignore_patterns: Vec::new(),
    };

    let received: Arc<Mutex<Vec<FileEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    let (watcher, stop_tx) =
        FsWatcher::new(config, move |event| {
            received_clone.lock().unwrap().push(event);
        })
        .expect("failed to create FsWatcher");

    let handle = std::thread::spawn(move || watcher.run());

    std::thread::sleep(Duration::from_millis(100));

    // Create files with different extensions.
    std::fs::write(watched_path.join("notes.md"), "markdown").unwrap();
    std::fs::write(watched_path.join("readme.txt"), "text").unwrap();
    std::fs::write(watched_path.join("data.json"), "{}").unwrap(); // should be ignored
    std::fs::write(watched_path.join("image.png"), "binary").unwrap(); // should be ignored

    std::thread::sleep(Duration::from_millis(300));

    stop_tx.send(()).ok();
    handle.join().expect("watcher thread should not panic");

    let events = received.lock().unwrap();

    // All received events must be for .md or .txt files.
    for event in events.iter() {
        let ext = event
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        assert!(
            ext == "md" || ext == "txt",
            "received event for unexpected extension '{}': {:?}",
            ext,
            event.path
        );
    }

    // We must have received at least one event for a matching extension file.
    let has_md_or_txt = events
        .iter()
        .any(|e| matches!(e.path.extension().and_then(|x| x.to_str()), Some("md") | Some("txt")));
    assert!(
        has_md_or_txt,
        "expected at least one event for .md or .txt files"
    );
}

// ============================================================================
// Test 3: FsWatcher respects ignore patterns
// ============================================================================

#[test]
fn test_fs_watcher_ignore_patterns() {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    let watched_path = dir.path().to_path_buf();

    // Create a subdirectory that should be ignored.
    let ignored_dir = watched_path.join(".git");
    std::fs::create_dir_all(&ignored_dir).unwrap();

    let config = FileWatcherConfig {
        enabled: true,
        paths: vec![watched_path.clone()],
        extensions: Vec::new(),
        debounce_ms: 50,
        ignore_patterns: vec![".git".to_string()],
    };

    let received: Arc<Mutex<Vec<FileEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = Arc::clone(&received);

    let (watcher, stop_tx) =
        FsWatcher::new(config, move |event| {
            received_clone.lock().unwrap().push(event);
        })
        .expect("failed to create FsWatcher");

    let handle = std::thread::spawn(move || watcher.run());

    std::thread::sleep(Duration::from_millis(100));

    // This file should be ignored.
    std::fs::write(ignored_dir.join("COMMIT_EDITMSG"), "initial commit").unwrap();

    // This file should be watched.
    std::fs::write(watched_path.join("main.rs"), "fn main() {}").unwrap();

    std::thread::sleep(Duration::from_millis(300));

    stop_tx.send(()).ok();
    handle.join().expect("watcher thread should not panic");

    let events = received.lock().unwrap();

    // No event should be for a path containing ".git".
    for event in events.iter() {
        let path_str = event.path.to_string_lossy();
        assert!(
            !path_str.contains(".git"),
            "received event for ignored path: {}",
            path_str
        );
    }
}

// ============================================================================
// Test 4: AppFocusWatcher with mock runner tracks focus changes
// ============================================================================

#[test]
fn test_app_focus_watcher_tracks_focus_changes() {
    let config = app_focus_config_enabled();
    let runner = MockAppRunner::from_names(&[
        Some("Safari"),
        Some("Safari"),
        Some("Terminal"),
        Some("Terminal"),
        Some("VSCode"),
    ]);

    let mut watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

    // Tick 1: Safari starts.
    let e1 = watcher.tick();
    assert!(!e1, "first tick should not emit an event");
    assert_eq!(watcher.current_app(), Some("Safari"));

    // Tick 2: Still Safari.
    let e2 = watcher.tick();
    assert!(!e2, "same app should not emit an event");

    // Tick 3: Switch to Terminal — Safari session completes.
    let e3 = watcher.tick();
    assert!(e3, "app switch should emit a completed event");
    assert_eq!(watcher.current_app(), Some("Terminal"));

    let events = watcher.drain_completed_events();
    assert_eq!(events.len(), 1, "expected exactly one completed event");
    assert_eq!(events[0].app_name, "Safari");

    // Tick 4: Still Terminal.
    let e4 = watcher.tick();
    assert!(!e4);

    // Tick 5: Switch to VSCode — Terminal session completes.
    let e5 = watcher.tick();
    assert!(e5);

    let events2 = watcher.drain_completed_events();
    assert_eq!(events2.len(), 1);
    assert_eq!(events2[0].app_name, "Terminal");
    assert_eq!(watcher.current_app(), Some("VSCode"));
}

// ============================================================================
// Test 5: WatcherConfig round-trips through TOML serialization
// ============================================================================

#[test]
fn test_watcher_config_toml_round_trip() {
    let original = WatcherConfig {
        watched_directories: vec![PathBuf::from("/home/user/documents")],
        browser_history_enabled: false,
        app_focus_enabled: true,
        poll_interval_secs: 120,
        engram_url: "http://engram.example.com:4000".to_string(),
        api_key: Some("sk_test_integration".to_string()),
        workspace: "integration-tests".to_string(),
        ignore_patterns: vec!["*.tmp".to_string(), "node_modules".to_string()],
        file_watcher: FileWatcherConfig {
            enabled: true,
            paths: vec![PathBuf::from("/tmp/watch")],
            extensions: vec!["rs".to_string(), "toml".to_string()],
            debounce_ms: 250,
            ignore_patterns: vec!["target".to_string()],
        },
        browser: BrowserWatcherConfig {
            enabled: true,
            browsers: vec!["chrome".to_string()],
            poll_interval_secs: 30,
            exclude_patterns: vec!["localhost".to_string()],
        },
        app_focus: AppFocusConfig {
            enabled: true,
            poll_interval_secs: 10,
            min_focus_secs: 2,
            exclude_apps: vec!["Finder".to_string()],
        },
    };

    let serialized = toml::to_string(&original).expect("serialization to TOML should succeed");
    let deserialized: WatcherConfig =
        toml::from_str(&serialized).expect("deserialization from TOML should succeed");

    assert_eq!(deserialized.watched_directories, original.watched_directories);
    assert_eq!(deserialized.browser_history_enabled, original.browser_history_enabled);
    assert_eq!(deserialized.app_focus_enabled, original.app_focus_enabled);
    assert_eq!(deserialized.poll_interval_secs, original.poll_interval_secs);
    assert_eq!(deserialized.engram_url, original.engram_url);
    assert_eq!(deserialized.api_key, original.api_key);
    assert_eq!(deserialized.workspace, original.workspace);
    assert_eq!(deserialized.ignore_patterns, original.ignore_patterns);

    // file_watcher sub-section
    assert_eq!(deserialized.file_watcher.enabled, original.file_watcher.enabled);
    assert_eq!(deserialized.file_watcher.paths, original.file_watcher.paths);
    assert_eq!(deserialized.file_watcher.extensions, original.file_watcher.extensions);
    assert_eq!(deserialized.file_watcher.debounce_ms, original.file_watcher.debounce_ms);

    // browser sub-section
    assert_eq!(deserialized.browser.enabled, original.browser.enabled);
    assert_eq!(deserialized.browser.browsers, original.browser.browsers);
    assert_eq!(deserialized.browser.poll_interval_secs, original.browser.poll_interval_secs);

    // app_focus sub-section
    assert_eq!(deserialized.app_focus.enabled, original.app_focus.enabled);
    assert_eq!(deserialized.app_focus.poll_interval_secs, original.app_focus.poll_interval_secs);
    assert_eq!(deserialized.app_focus.min_focus_secs, original.app_focus.min_focus_secs);
    assert_eq!(deserialized.app_focus.exclude_apps, original.app_focus.exclude_apps);
}

// ============================================================================
// Test 6: Complete config file with all sections loads correctly
// ============================================================================

#[test]
fn test_full_config_file_loads_correctly() {
    let toml_content = r#"
watched_directories = ["/home/user/docs", "/home/user/code"]
browser_history_enabled = true
app_focus_enabled = true
poll_interval_secs = 60
engram_url = "http://localhost:4000"
api_key = "sk_live_abc123"
workspace = "my-workspace"
ignore_patterns = ["*.log", ".DS_Store"]

[file_watcher]
enabled = true
paths = ["/home/user/docs", "/home/user/code"]
extensions = ["md", "rs", "toml"]
debounce_ms = 200
ignore_patterns = [".git", "target", "node_modules"]

[browser]
enabled = true
browsers = ["chrome", "firefox", "safari"]
poll_interval_secs = 30
exclude_patterns = ["localhost", "127.0.0.1", "about:"]

[app_focus]
enabled = true
poll_interval_secs = 3
min_focus_secs = 2
exclude_apps = ["Finder", "loginwindow", "SystemUIServer"]
"#;

    // Write to a temp file and load it.
    let temp_file = tempfile::NamedTempFile::new().expect("failed to create temp file");
    std::fs::write(temp_file.path(), toml_content).expect("failed to write config file");

    let config = WatcherConfig::load(temp_file.path()).expect("config should load without error");

    // Top-level fields.
    assert_eq!(config.watched_directories.len(), 2);
    assert!(config.browser_history_enabled);
    assert!(config.app_focus_enabled);
    assert_eq!(config.poll_interval_secs, 60);
    assert_eq!(config.engram_url, "http://localhost:4000");
    assert_eq!(config.api_key.as_deref(), Some("sk_live_abc123"));
    assert_eq!(config.workspace, "my-workspace");
    assert_eq!(config.ignore_patterns, vec!["*.log", ".DS_Store"]);

    // [file_watcher] section.
    assert!(config.file_watcher.enabled);
    assert_eq!(config.file_watcher.paths.len(), 2);
    assert_eq!(
        config.file_watcher.extensions,
        vec!["md", "rs", "toml"]
    );
    assert_eq!(config.file_watcher.debounce_ms, 200);
    assert_eq!(config.file_watcher.ignore_patterns.len(), 3);

    // [browser] section.
    assert!(config.browser.enabled);
    assert_eq!(
        config.browser.browsers,
        vec!["chrome", "firefox", "safari"]
    );
    assert_eq!(config.browser.poll_interval_secs, 30);
    assert!(config.browser.exclude_patterns.contains(&"localhost".to_string()));

    // [app_focus] section.
    assert!(config.app_focus.enabled);
    assert_eq!(config.app_focus.poll_interval_secs, 3);
    assert_eq!(config.app_focus.min_focus_secs, 2);
    assert_eq!(
        config.app_focus.exclude_apps,
        vec!["Finder", "loginwindow", "SystemUIServer"]
    );
}

// ============================================================================
// Test 7: FileEvent.to_memory_content() produces well-formed content strings
// ============================================================================

#[test]
fn test_file_event_to_memory_content_format() {
    // Created event
    let created = FileEvent {
        path: PathBuf::from("/home/user/docs/notes.md"),
        kind: ChangeKind::Created,
        timestamp: "2026-03-09T12:00:00Z".to_string(),
    };
    let content = created.to_memory_content();
    assert!(
        content.starts_with("File created:"),
        "created event content should start with 'File created:': {content}"
    );
    assert!(
        content.contains("notes.md"),
        "content should include the filename: {content}"
    );
    assert!(
        content.contains("2026-03-09T12:00:00Z"),
        "content should include the timestamp: {content}"
    );

    // Modified event
    let modified = FileEvent {
        path: PathBuf::from("/project/src/main.rs"),
        kind: ChangeKind::Modified,
        timestamp: "2026-03-09T13:30:45Z".to_string(),
    };
    let content = modified.to_memory_content();
    assert!(
        content.starts_with("File modified:"),
        "modified event content should start with 'File modified:': {content}"
    );
    assert!(content.contains("main.rs"));
    assert!(content.contains("2026-03-09T13:30:45Z"));

    // Deleted event
    let deleted = FileEvent {
        path: PathBuf::from("/tmp/old_file.txt"),
        kind: ChangeKind::Deleted,
        timestamp: "2026-03-09T14:00:00Z".to_string(),
    };
    let content = deleted.to_memory_content();
    assert!(
        content.starts_with("File deleted:"),
        "deleted event content should start with 'File deleted:': {content}"
    );
    assert!(content.contains("old_file.txt"));
    assert!(content.contains("2026-03-09T14:00:00Z"));

    // Verify the format template: "File {kind}: {path} at {timestamp}"
    let expected_format = format!(
        "File deleted: {} at 2026-03-09T14:00:00Z",
        PathBuf::from("/tmp/old_file.txt").display()
    );
    assert_eq!(
        deleted.to_memory_content(),
        expected_format,
        "to_memory_content should follow the format 'File {{kind}}: {{path}} at {{timestamp}}'"
    );
}

// ============================================================================
// Additional integration: FocusEvent.to_memory_content() format
// ============================================================================

#[test]
fn test_focus_event_to_memory_content_format() {
    use chrono::DateTime;

    let started: chrono::DateTime<chrono::Utc> = "2026-03-09T09:00:00Z"
        .parse::<DateTime<chrono::Utc>>()
        .unwrap();
    let ended: chrono::DateTime<chrono::Utc> = "2026-03-09T09:02:30Z"
        .parse::<DateTime<chrono::Utc>>()
        .unwrap();

    let event = FocusEvent {
        app_name: "Cursor".to_string(),
        window_title: Some("engram — watcher_integration.rs".to_string()),
        started_at: started,
        ended_at: ended,
        duration_secs: 150,
    };

    let content = event.to_memory_content();

    assert!(
        content.contains("Cursor"),
        "content should include the app name: {content}"
    );
    assert!(
        content.contains("engram"),
        "content should include the window title: {content}"
    );
    assert!(
        content.contains("150"),
        "content should include the duration in seconds: {content}"
    );
    assert!(
        content.contains("2026-03-09T09:00:00Z"),
        "content should include start timestamp: {content}"
    );
    assert!(
        content.contains("2026-03-09T09:02:30Z"),
        "content should include end timestamp: {content}"
    );

    // Without window title
    let event_no_title = FocusEvent {
        app_name: "iTerm2".to_string(),
        window_title: None,
        started_at: started,
        ended_at: ended,
        duration_secs: 150,
    };
    let content_no_title = event_no_title.to_memory_content();
    assert!(
        content_no_title.contains("iTerm2"),
        "content should include the app name even without window title: {content_no_title}"
    );
    assert!(
        !content_no_title.contains("None"),
        "content should not contain the string 'None' when window_title is None: {content_no_title}"
    );
}
