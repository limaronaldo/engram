//! Engram Watcher Daemon
//!
//! Monitors file system changes, browser history, and application focus events,
//! then stores observations as Engram memories via the HTTP API.
//!
//! # Usage
//!
//! ```bash
//! # Run with default config (~/.config/engram/watcher.toml)
//! engram-watcher
//!
//! # Run with custom config path
//! engram-watcher --config /path/to/watcher.toml
//!
//! # Dry-run: print events to stdout without sending to Engram
//! engram-watcher --dry-run
//!
//! # Enable verbose tracing output
//! engram-watcher --verbose
//! ```
//!
//! # Building
//!
//! ```bash
//! cargo build --bin engram-watcher --features watcher
//! ```

use std::path::PathBuf;
use std::sync::mpsc as stdmpsc;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use engram::watcher::{
    app_focus::AppFocusWatcher,
    browser::BrowserWatcher,
    config::WatcherConfig,
    fs_watcher::{FileEvent, FsWatcher},
};

// ---------------------------------------------------------------------------
// CLI arguments
// ---------------------------------------------------------------------------

/// Engram Watcher Daemon — monitors files, browser history, and app focus,
/// then stores observations as Engram memories.
#[derive(Parser, Debug)]
#[command(name = "engram-watcher")]
#[command(about = "Engram watcher daemon: file, browser, and app-focus monitoring")]
#[command(version)]
struct Args {
    /// Path to the watcher TOML configuration file.
    ///
    /// Defaults to `~/.config/engram/watcher.toml` when not provided.
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Print events to stdout instead of sending them to the Engram server.
    ///
    /// Useful for testing the configuration without side effects.
    #[arg(long, default_value = "false")]
    dry_run: bool,

    /// Enable verbose tracing output (sets RUST_LOG=debug if not already set).
    #[arg(short, long, default_value = "false")]
    verbose: bool,
}

// ---------------------------------------------------------------------------
// HTTP client
// ---------------------------------------------------------------------------

/// Lightweight JSON-RPC 2.0 payload for `memory_create`.
#[derive(serde::Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: serde_json::Value,
}

/// Context needed to post a single memory event to the Engram HTTP API.
struct MemorySender<'a> {
    client: &'a reqwest::Client,
    engram_url: &'a str,
    api_key: Option<&'a str>,
    workspace: &'a str,
    dry_run: bool,
}

impl<'a> MemorySender<'a> {
    /// Send a single memory to the Engram HTTP API via `memory_create`.
    ///
    /// On failure (connection refused, API error, etc.) the error is logged
    /// and swallowed — watcher events are best-effort.
    async fn send(&self, content: String, memory_type: &str, tags: Vec<String>) {
        if self.dry_run {
            info!(
                workspace = self.workspace,
                r#type = memory_type,
                "[dry-run] Would create memory: {}",
                content
            );
            return;
        }

        let request_id: u64 = Utc::now().timestamp_micros() as u64;

        let payload = JsonRpcRequest {
            jsonrpc: "2.0",
            id: request_id,
            method: "memory_create",
            params: serde_json::json!({
                "content": content,
                "memory_type": memory_type,
                "workspace": self.workspace,
                "tags": tags,
            }),
        };

        let url = format!("{}/v1/mcp", self.engram_url.trim_end_matches('/'));

        let mut req = self.client.post(&url).json(&payload);

        if let Some(key) = self.api_key {
            req = req.bearer_auth(key);
        }

        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                debug!(workspace = self.workspace, "Memory created successfully");
            }
            Ok(resp) => {
                warn!(
                    status = %resp.status(),
                    "Engram API returned non-2xx status"
                );
            }
            Err(e) => {
                warn!(error = %e, "Failed to send memory to Engram");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Watcher orchestration
// ---------------------------------------------------------------------------

/// Spawn the file system watcher in a dedicated thread.
///
/// Returns the stop sender so the caller can shut the watcher down.
fn start_fs_watcher(
    config: &WatcherConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(String, &'static str, Vec<String>)>,
) -> Option<stdmpsc::SyncSender<()>> {
    if !config.file_watcher.enabled || config.file_watcher.paths.is_empty() {
        info!("File watcher disabled or no paths configured — skipping");
        return None;
    }

    let fs_config = config.file_watcher.clone();

    let callback = move |event: FileEvent| {
        let content = event.to_memory_content();
        let tags = vec![
            "watcher".to_string(),
            "file-system".to_string(),
            event.kind.to_string(),
        ];
        // Log and discard send errors (receiver has gone away during shutdown).
        let _ = tx.send((content, "note", tags));
    };

    match FsWatcher::new(fs_config, callback) {
        Ok((watcher, stop_tx)) => {
            std::thread::Builder::new()
                .name("engram-fs-watcher".to_string())
                .spawn(move || {
                    info!("File system watcher thread started");
                    watcher.run();
                    info!("File system watcher thread exiting");
                })
                .expect("Failed to spawn fs-watcher thread");
            Some(stop_tx)
        }
        Err(e) => {
            error!(error = %e, "Failed to create file system watcher");
            None
        }
    }
}

/// Run the browser history poller in a Tokio task.
///
/// Polls at `config.browser.poll_interval_secs` and sends any new visits to
/// the event channel.
fn start_browser_watcher(
    config: &WatcherConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(String, &'static str, Vec<String>)>,
    shutdown_rx: Arc<Mutex<tokio::sync::watch::Receiver<bool>>>,
) {
    if !config.browser.enabled {
        info!("Browser watcher disabled — skipping");
        return;
    }

    let browser_config = config.browser.clone();
    let poll_interval = Duration::from_secs(browser_config.poll_interval_secs);

    tokio::spawn(async move {
        let mut watcher = BrowserWatcher::new(browser_config);
        let initial_since = Utc::now();

        info!(?poll_interval, "Browser watcher started");

        loop {
            // Wait one interval before the first poll so we don't race startup.
            let shutdown = {
                let mut rx = shutdown_rx.lock().await;
                tokio::time::timeout(poll_interval, rx.changed()).await
            };

            if shutdown.is_ok() {
                info!("Browser watcher received shutdown signal");
                break;
            }

            let visits = watcher.poll(initial_since);
            for visit in visits {
                let content = BrowserWatcher::visit_to_memory_content(&visit);
                let tags = vec![
                    "watcher".to_string(),
                    "browser-history".to_string(),
                    visit.browser.clone(),
                ];
                if tx.send((content, "note", tags)).is_err() {
                    break;
                }
            }
        }

        info!("Browser watcher task exiting");
    });
}

/// Run the app focus poller in a Tokio task.
fn start_app_focus_watcher(
    config: &WatcherConfig,
    tx: tokio::sync::mpsc::UnboundedSender<(String, &'static str, Vec<String>)>,
    shutdown_rx: Arc<Mutex<tokio::sync::watch::Receiver<bool>>>,
) {
    if !config.app_focus.enabled {
        info!("App focus watcher disabled — skipping");
        return;
    }

    let app_focus_config = config.app_focus.clone();
    let poll_interval = Duration::from_secs(app_focus_config.poll_interval_secs);

    tokio::spawn(async move {
        let mut watcher = AppFocusWatcher::new(app_focus_config);

        info!(?poll_interval, "App focus watcher started");

        loop {
            let shutdown = {
                let mut rx = shutdown_rx.lock().await;
                tokio::time::timeout(poll_interval, rx.changed()).await
            };

            if shutdown.is_ok() {
                info!("App focus watcher received shutdown signal");
                break;
            }

            // Tick on this thread (sync call, but fast).
            watcher.tick();

            for event in watcher.drain_completed_events() {
                let content = event.to_memory_content();
                let tags = vec!["watcher".to_string(), "app-focus".to_string()];
                if tx.send((content, "episodic", tags)).is_err() {
                    break;
                }
            }
        }

        info!("App focus watcher task exiting");
    });
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Set up tracing.
    let filter = if args.verbose {
        "engram_watcher=debug,engram=debug"
    } else {
        "engram_watcher=info,engram=info"
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter)),
        )
        .init();

    // Load configuration.
    let config = match args.config {
        Some(ref path) => match WatcherConfig::load(path) {
            Ok(cfg) => {
                info!(path = ?path, "Loaded watcher config");
                cfg
            }
            Err(e) => {
                error!(error = %e, "Failed to load config; using defaults");
                WatcherConfig::default()
            }
        },
        None => {
            let cfg = WatcherConfig::load_or_default();
            info!("Loaded watcher config (default path)");
            cfg
        }
    };

    info!(
        engram_url = %config.engram_url,
        workspace = %config.workspace,
        dry_run = args.dry_run,
        "Engram watcher daemon starting"
    );

    // Build the reqwest HTTP client (shared across all senders).
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client");

    // Unbounded channel: watchers → event consumer task.
    // Events are tuples of (content, memory_type, tags).
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::unbounded_channel::<(String, &'static str, Vec<String>)>();

    // Shutdown watch channel: broadcast `true` to all tasks when Ctrl-C fires.
    let (shutdown_tx, shutdown_rx_base) = tokio::sync::watch::channel(false);
    let shutdown_rx = Arc::new(Mutex::new(shutdown_rx_base));

    // Start individual watchers.
    let fs_stop_tx = start_fs_watcher(&config, event_tx.clone());
    start_browser_watcher(
        &config,
        event_tx.clone(),
        Arc::clone(&shutdown_rx),
    );
    start_app_focus_watcher(
        &config,
        event_tx.clone(),
        Arc::clone(&shutdown_rx),
    );

    // Drop the last producer reference held by main so the channel closes
    // naturally when all watcher tasks finish.
    drop(event_tx);

    // Clone config fields for the consumer task.
    let engram_url = config.engram_url.clone();
    let api_key = config.api_key.clone();
    let workspace = config.workspace.clone();
    let dry_run = args.dry_run;

    // Event consumer: drain events and POST them to Engram.
    let consumer = tokio::spawn(async move {
        let sender = MemorySender {
            client: &http_client,
            engram_url: &engram_url,
            api_key: api_key.as_deref(),
            workspace: &workspace,
            dry_run,
        };
        while let Some((content, memory_type, tags)) = event_rx.recv().await {
            sender.send(content, memory_type, tags).await;
        }
        info!("Event consumer task exiting");
    });

    // Wait for Ctrl-C / SIGTERM.
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl-C; initiating graceful shutdown");
        }
    }

    // Signal async tasks to shut down.
    let _ = shutdown_tx.send(true);

    // Signal the synchronous FS watcher thread to stop.
    if let Some(stop_tx) = fs_stop_tx {
        let _ = stop_tx.send(());
    }

    // Give tasks a moment to flush pending events, then abort.
    tokio::time::sleep(Duration::from_millis(500)).await;
    consumer.abort();

    info!("Engram watcher daemon stopped");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::NamedTempFile;

    // ---- T1: Config loading from file ----------------------------------------

    #[test]
    fn test_load_valid_toml_config() {
        let toml = r#"
            engram_url = "http://localhost:9999"
            api_key = "sk_test_abc"
            workspace = "test-workspace"
            poll_interval_secs = 120

            [file_watcher]
            enabled = true
            paths = ["/tmp"]
            extensions = ["rs", "md"]
            debounce_ms = 200

            [browser]
            enabled = false

            [app_focus]
            enabled = false
        "#;

        let tmp = NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), toml).unwrap();

        let cfg = WatcherConfig::load(tmp.path()).expect("should parse");
        assert_eq!(cfg.engram_url, "http://localhost:9999");
        assert_eq!(cfg.api_key.as_deref(), Some("sk_test_abc"));
        assert_eq!(cfg.workspace, "test-workspace");
        assert_eq!(cfg.poll_interval_secs, 120);
        assert!(cfg.file_watcher.enabled);
        assert_eq!(cfg.file_watcher.extensions, vec!["rs", "md"]);
        assert!(!cfg.browser.enabled);
        assert!(!cfg.app_focus.enabled);
    }

    // ---- T2: Nonexistent config file returns an error ------------------------

    #[test]
    fn test_load_missing_config_returns_error() {
        let result = WatcherConfig::load(Path::new("/nonexistent/watcher.toml"));
        assert!(result.is_err(), "missing file should return Err");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("watcher.toml") || err_msg.contains("Cannot read"),
            "error should mention the path or problem: {err_msg}"
        );
    }

    // ---- T3: Default config uses expected defaults ----------------------------

    #[test]
    fn test_default_config_values() {
        let cfg = WatcherConfig::default();
        assert_eq!(cfg.engram_url, "http://localhost:3000");
        assert_eq!(cfg.workspace, "watcher");
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.poll_interval_secs, 300);
        assert!(cfg.file_watcher.enabled);
        assert!(!cfg.browser.enabled);
        assert!(!cfg.app_focus.enabled);
    }

    // ---- T4: CLI accepts --dry-run flag --------------------------------------

    #[test]
    fn test_cli_dry_run_flag() {
        let args = Args::try_parse_from(["engram-watcher", "--dry-run"]).unwrap();
        assert!(args.dry_run);
        assert!(!args.verbose);
        assert!(args.config.is_none());
    }

    // ---- T5: CLI accepts --verbose flag --------------------------------------

    #[test]
    fn test_cli_verbose_flag() {
        let args = Args::try_parse_from(["engram-watcher", "--verbose"]).unwrap();
        assert!(args.verbose);
        assert!(!args.dry_run);
    }

    // ---- T6: CLI accepts --config with a path --------------------------------

    #[test]
    fn test_cli_config_path() {
        let args =
            Args::try_parse_from(["engram-watcher", "--config", "/tmp/my-watcher.toml"]).unwrap();
        assert_eq!(args.config, Some(PathBuf::from("/tmp/my-watcher.toml")));
    }

    // ---- T7: CLI accepts combined flags -------------------------------------

    #[test]
    fn test_cli_combined_flags() {
        let args = Args::try_parse_from([
            "engram-watcher",
            "--config",
            "/tmp/cfg.toml",
            "--dry-run",
            "--verbose",
        ])
        .unwrap();
        assert!(args.dry_run);
        assert!(args.verbose);
        assert_eq!(args.config, Some(PathBuf::from("/tmp/cfg.toml")));
    }

    // ---- T8: JsonRpcRequest serialises correctly ----------------------------

    #[test]
    fn test_json_rpc_request_serialisation() {
        let req = JsonRpcRequest {
            jsonrpc: "2.0",
            id: 42,
            method: "memory_create",
            params: serde_json::json!({
                "content": "hello",
                "memory_type": "note",
                "workspace": "watcher",
                "tags": ["watcher", "file-system"],
            }),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"memory_create\""));
        assert!(json.contains("\"id\":42"));
        assert!(json.contains("\"content\":\"hello\""));
    }

    // ---- T9: Partial TOML config uses defaults for missing fields -----------

    #[test]
    fn test_partial_toml_uses_defaults() {
        let toml = r#"
            engram_url = "http://engram.local:4000"
        "#;
        let tmp = NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), toml).unwrap();

        let cfg = WatcherConfig::load(tmp.path()).unwrap();
        assert_eq!(cfg.engram_url, "http://engram.local:4000");
        // defaults
        assert_eq!(cfg.workspace, "watcher");
        assert!(cfg.api_key.is_none());
        assert_eq!(cfg.poll_interval_secs, 300);
    }
}
