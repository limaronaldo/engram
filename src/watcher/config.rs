//! Watcher daemon configuration
//!
//! Loads and validates configuration for the Engram watcher daemon from a TOML file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{EngramError, Result};

// ---------------------------------------------------------------------------
// Default value helpers (required by serde)
// ---------------------------------------------------------------------------

fn default_true() -> bool {
    true
}

fn default_poll_interval() -> u64 {
    300 // 5 minutes
}

fn default_debounce_ms() -> u64 {
    500 // 500 ms debounce
}

fn default_browser_poll_interval() -> u64 {
    60 // 1 minute
}

fn default_app_focus_poll_interval() -> u64 {
    5 // 5 seconds
}

fn default_min_focus_secs() -> u64 {
    1 // ignore sub-second switches
}

fn default_engram_url() -> String {
    "http://localhost:3000".to_string()
}

fn default_workspace() -> String {
    "watcher".to_string()
}

// ---------------------------------------------------------------------------
// FileWatcherConfig
// ---------------------------------------------------------------------------

/// Configuration for the file system watcher component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileWatcherConfig {
    /// Whether the file watcher is enabled (default: true).
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Directories to watch for file changes.
    #[serde(default)]
    pub paths: Vec<PathBuf>,

    /// File extensions to watch (e.g. `["rs", "md", "txt"]`).
    /// An empty list means all extensions are watched.
    #[serde(default)]
    pub extensions: Vec<String>,

    /// Debounce interval in milliseconds (default: 500).
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,

    /// Glob patterns for files/directories to ignore.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,
}

impl Default for FileWatcherConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            paths: Vec::new(),
            extensions: Vec::new(),
            debounce_ms: default_debounce_ms(),
            ignore_patterns: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// BrowserWatcherConfig
// ---------------------------------------------------------------------------

/// Configuration for the browser history watcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserWatcherConfig {
    /// Whether the browser watcher is enabled.
    #[serde(default)]
    pub enabled: bool,
    /// Browsers to watch. Supported values: "chrome", "firefox", "safari".
    #[serde(default = "default_browsers")]
    pub browsers: Vec<String>,
    /// How often to poll browser history databases (in seconds).
    #[serde(default = "default_browser_poll_interval")]
    pub poll_interval_secs: u64,
    /// URL patterns to exclude from memory creation (substring match).
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,
}

fn default_browsers() -> Vec<String> {
    vec!["chrome".to_string(), "firefox".to_string()]
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "about:".to_string(),
        "chrome://".to_string(),
        "chrome-extension://".to_string(),
    ]
}

impl Default for BrowserWatcherConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            browsers: default_browsers(),
            poll_interval_secs: default_browser_poll_interval(),
            exclude_patterns: default_exclude_patterns(),
        }
    }
}

// ---------------------------------------------------------------------------
// AppFocusConfig
// ---------------------------------------------------------------------------

/// Configuration for the application focus watcher.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppFocusConfig {
    /// Whether to track active application focus events (default: false).
    #[serde(default)]
    pub enabled: bool,

    /// How often to poll for the foreground app, in seconds (default: 5).
    #[serde(default = "default_app_focus_poll_interval")]
    pub poll_interval_secs: u64,

    /// Minimum number of seconds an app must be in focus before recording
    /// the event (default: 1).
    #[serde(default = "default_min_focus_secs")]
    pub min_focus_secs: u64,

    /// Application names to exclude from tracking.
    #[serde(default)]
    pub exclude_apps: Vec<String>,
}

impl Default for AppFocusConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            poll_interval_secs: default_app_focus_poll_interval(),
            min_focus_secs: default_min_focus_secs(),
            exclude_apps: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// WatcherConfig
// ---------------------------------------------------------------------------

/// Configuration for the Engram watcher daemon.
///
/// Loaded from `~/.config/engram/watcher.toml` (or a custom path).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Directories to watch for file changes.
    #[serde(default)]
    pub watched_directories: Vec<PathBuf>,

    /// Whether to track browser history (default: true).
    #[serde(default = "default_true")]
    pub browser_history_enabled: bool,

    /// Whether to track active application focus events (default: false).
    #[serde(default)]
    pub app_focus_enabled: bool,

    /// How often to poll watched directories, in seconds (default: 300).
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u64,

    /// URL of the Engram HTTP server (default: "http://localhost:3000").
    #[serde(default = "default_engram_url")]
    pub engram_url: String,

    /// Optional API key for authenticated Engram endpoints.
    pub api_key: Option<String>,

    /// Engram workspace to store watcher memories in (default: "watcher").
    #[serde(default = "default_workspace")]
    pub workspace: String,

    /// Glob patterns for files/directories to ignore.
    #[serde(default)]
    pub ignore_patterns: Vec<String>,

    /// Fine-grained file watcher configuration.
    #[serde(default)]
    pub file_watcher: FileWatcherConfig,

    /// Fine-grained browser watcher configuration.
    #[serde(default)]
    pub browser: BrowserWatcherConfig,

    /// Fine-grained application focus watcher configuration.
    #[serde(default)]
    pub app_focus: AppFocusConfig,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            watched_directories: Vec::new(),
            browser_history_enabled: default_true(),
            app_focus_enabled: false,
            poll_interval_secs: default_poll_interval(),
            engram_url: default_engram_url(),
            api_key: None,
            workspace: default_workspace(),
            ignore_patterns: Vec::new(),
            file_watcher: FileWatcherConfig::default(),
            browser: BrowserWatcherConfig::default(),
            app_focus: AppFocusConfig::default(),
        }
    }
}

impl WatcherConfig {
    /// Load configuration from the given TOML file path.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            EngramError::Config(format!("Cannot read watcher config {:?}: {}", path, e))
        })?;

        toml::from_str(&contents).map_err(|e| {
            EngramError::Config(format!(
                "Invalid TOML in watcher config {:?}: {}",
                path, e
            ))
        })
    }

    /// Try to load from the default path (`~/.config/engram/watcher.toml`).
    ///
    /// Falls back to [`WatcherConfig::default`] if the file does not exist.
    pub fn load_or_default() -> Self {
        let default_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("engram")
            .join("watcher.toml");

        if default_path.exists() {
            match Self::load(&default_path) {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!(
                        path = ?default_path,
                        error = %e,
                        "Failed to parse watcher config, using defaults"
                    );
                    Self::default()
                }
            }
        } else {
            Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = WatcherConfig::default();
        assert!(cfg.watched_directories.is_empty());
        assert!(cfg.browser_history_enabled);
        assert!(!cfg.app_focus_enabled);
        assert_eq!(cfg.poll_interval_secs, 300);
        assert_eq!(cfg.engram_url, "http://localhost:3000");
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.workspace, "watcher");
    }

    #[test]
    fn test_full_config_from_toml_string() {
        let toml_str = r#"
            watched_directories = ["/home/user/Documents", "/tmp/notes"]
            browser_history_enabled = false
            app_focus_enabled = true
            poll_interval_secs = 60
            engram_url = "http://engram.example.com:4000"
            api_key = "sk_test_abc123"
            workspace = "my-watcher"
            ignore_patterns = ["*.tmp", ".git", "node_modules"]
        "#;
        let cfg: WatcherConfig = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.watched_directories.len(), 2);
        assert!(!cfg.browser_history_enabled);
        assert!(cfg.app_focus_enabled);
        assert_eq!(cfg.poll_interval_secs, 60);
    }

    #[test]
    fn test_partial_config_uses_defaults() {
        let toml_str = r#"
            watched_directories = ["/data"]
            poll_interval_secs = 120
        "#;
        let cfg: WatcherConfig = toml::from_str(toml_str).expect("should parse");
        assert_eq!(cfg.watched_directories, vec![PathBuf::from("/data")]);
        assert_eq!(cfg.poll_interval_secs, 120);
        assert!(cfg.browser_history_enabled);
    }

    #[test]
    fn test_file_watcher_config_default() {
        let cfg = FileWatcherConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.paths.is_empty());
        assert!(cfg.extensions.is_empty());
        assert_eq!(cfg.debounce_ms, 500);
    }

    #[test]
    fn test_browser_watcher_config_default() {
        let cfg = BrowserWatcherConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.browsers, vec!["chrome", "firefox"]);
        assert_eq!(cfg.poll_interval_secs, 60);
        assert!(!cfg.exclude_patterns.is_empty());
    }

    #[test]
    fn test_app_focus_config_default() {
        let cfg = AppFocusConfig::default();
        assert!(!cfg.enabled);
        assert_eq!(cfg.poll_interval_secs, 5);
        assert_eq!(cfg.min_focus_secs, 1);
        assert!(cfg.exclude_apps.is_empty());
    }

    #[test]
    fn test_load_nonexistent_file_returns_error() {
        let result = WatcherConfig::load(Path::new("/nonexistent/path/watcher.toml"));
        assert!(result.is_err());
    }

    #[test]
    fn test_load_or_default_returns_default_when_no_file() {
        let cfg = WatcherConfig::load_or_default();
        assert_eq!(cfg.poll_interval_secs, 300);
    }
}
