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

fn default_engram_url() -> String {
    "http://localhost:3000".to_string()
}

fn default_workspace() -> String {
    "watcher".to_string()
}

// ---------------------------------------------------------------------------
// WatcherConfig
// ---------------------------------------------------------------------------

/// Configuration for the Engram watcher daemon.
///
/// Loaded from `~/.config/engram/watcher.toml` (or a custom path).
/// All fields have sensible defaults so a minimal config file is sufficient.
///
/// # Example TOML
/// ```toml
/// watched_directories = ["/home/user/Documents"]
/// browser_history_enabled = true
/// poll_interval_secs = 60
/// engram_url = "http://localhost:3000"
/// api_key = "sk_test_abc123"
/// workspace = "watcher"
/// ignore_patterns = ["*.tmp", ".git"]
/// ```
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
        }
    }
}

impl WatcherConfig {
    /// Load configuration from the given TOML file path.
    ///
    /// Returns an error if the file cannot be read or if the TOML is malformed.
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
    /// Returns an error only if the file exists but cannot be parsed.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let cfg = WatcherConfig::default();

        assert!(cfg.watched_directories.is_empty());
        assert!(cfg.browser_history_enabled, "browser_history_enabled should default to true");
        assert!(!cfg.app_focus_enabled, "app_focus_enabled should default to false");
        assert_eq!(cfg.poll_interval_secs, 300);
        assert_eq!(cfg.engram_url, "http://localhost:3000");
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.workspace, "watcher");
        assert!(cfg.ignore_patterns.is_empty());
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

        let cfg: WatcherConfig = toml::from_str(toml_str).expect("should parse full config");

        assert_eq!(cfg.watched_directories.len(), 2);
        assert_eq!(cfg.watched_directories[0], PathBuf::from("/home/user/Documents"));
        assert_eq!(cfg.watched_directories[1], PathBuf::from("/tmp/notes"));
        assert!(!cfg.browser_history_enabled);
        assert!(cfg.app_focus_enabled);
        assert_eq!(cfg.poll_interval_secs, 60);
        assert_eq!(cfg.engram_url, "http://engram.example.com:4000");
        assert_eq!(cfg.api_key.as_deref(), Some("sk_test_abc123"));
        assert_eq!(cfg.workspace, "my-watcher");
        assert_eq!(cfg.ignore_patterns, vec!["*.tmp", ".git", "node_modules"]);
    }

    #[test]
    fn test_partial_config_uses_defaults() {
        // Only override a couple of fields; the rest should fall back to defaults.
        let toml_str = r#"
            watched_directories = ["/data"]
            poll_interval_secs = 120
        "#;

        let cfg: WatcherConfig = toml::from_str(toml_str).expect("should parse partial config");

        assert_eq!(cfg.watched_directories, vec![PathBuf::from("/data")]);
        assert_eq!(cfg.poll_interval_secs, 120);

        // Defaults for fields not specified
        assert!(cfg.browser_history_enabled, "browser_history_enabled should default to true");
        assert!(!cfg.app_focus_enabled);
        assert_eq!(cfg.engram_url, "http://localhost:3000");
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.workspace, "watcher");
        assert!(cfg.ignore_patterns.is_empty());
    }

    #[test]
    fn test_load_from_file() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        let mut file = NamedTempFile::new().expect("create temp file");
        writeln!(
            file,
            r#"
                engram_url = "http://localhost:9999"
                api_key = "test-key"
                workspace = "file-test"
            "#
        )
        .expect("write config");

        let cfg = WatcherConfig::load(file.path()).expect("load should succeed");

        assert_eq!(cfg.engram_url, "http://localhost:9999");
        assert_eq!(cfg.api_key.as_deref(), Some("test-key"));
        assert_eq!(cfg.workspace, "file-test");
        // Unspecified fields use defaults
        assert!(cfg.browser_history_enabled);
        assert_eq!(cfg.poll_interval_secs, 300);
    }

    #[test]
    fn test_load_nonexistent_file_returns_error() {
        let result = WatcherConfig::load(Path::new("/nonexistent/path/watcher.toml"));
        assert!(result.is_err(), "loading a missing file should return an error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("Cannot read watcher config"),
            "error message should describe the problem: {err_msg}"
        );
    }

    #[test]
    fn test_load_or_default_returns_default_when_no_file() {
        // This simply must not panic and must return a valid default config
        // when no config file is present (the common case in CI).
        let cfg = WatcherConfig::load_or_default();
        assert_eq!(cfg.poll_interval_secs, 300);
        assert_eq!(cfg.engram_url, "http://localhost:3000");
    }
}
