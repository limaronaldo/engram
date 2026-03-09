//! Browser history watcher module.
//!
//! Monitors browser history databases (Chrome, Firefox, Safari) for new visits
//! and converts them into memory events. Handles locked SQLite databases by
//! copying to a temporary file before reading.

use chrono::{DateTime, TimeZone, Utc};
use rusqlite::{Connection, OpenFlags};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::watcher::config::BrowserWatcherConfig;

/// A single browser history visit that can be turned into a memory.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowserVisit {
    /// Page URL.
    pub url: String,
    /// Page title (may be empty).
    pub title: String,
    /// When the page was visited.
    pub visited_at: DateTime<Utc>,
    /// Which browser produced this visit.
    pub browser: String,
}

/// Error type for browser watcher operations.
#[derive(Debug)]
pub enum BrowserWatcherError {
    /// The history database could not be opened or read.
    DatabaseError(String),
    /// The database file does not exist (browser not installed / never launched).
    DatabaseNotFound(PathBuf),
    /// An IO error occurred (e.g. while copying a locked file).
    Io(std::io::Error),
}

impl std::fmt::Display for BrowserWatcherError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserWatcherError::DatabaseError(msg) => write!(f, "Database error: {msg}"),
            BrowserWatcherError::DatabaseNotFound(path) => {
                write!(f, "Database not found: {}", path.display())
            }
            BrowserWatcherError::Io(e) => write!(f, "IO error: {e}"),
        }
    }
}

impl From<std::io::Error> for BrowserWatcherError {
    fn from(e: std::io::Error) -> Self {
        BrowserWatcherError::Io(e)
    }
}

impl From<rusqlite::Error> for BrowserWatcherError {
    fn from(e: rusqlite::Error) -> Self {
        BrowserWatcherError::DatabaseError(e.to_string())
    }
}

// ────────────────────────────────────────────────────────────────────────────
// DB path resolution
// ────────────────────────────────────────────────────────────────────────────

/// Return the macOS path to the Chrome history database.
pub fn chrome_history_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(
        home.join("Library")
            .join("Application Support")
            .join("Google")
            .join("Chrome")
            .join("Default")
            .join("History"),
    )
}

/// Return the macOS path to the first matching Firefox profile's `places.sqlite`.
pub fn firefox_history_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let profiles_dir = home
        .join("Library")
        .join("Application Support")
        .join("Firefox")
        .join("Profiles");

    // Walk the profiles directory looking for `*.default-release` or `*.default`.
    let read_dir = std::fs::read_dir(&profiles_dir).ok()?;
    let mut candidates: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.ends_with(".default-release") || name.ends_with(".default")
        })
        .collect();

    // Prefer `.default-release` over `.default`.
    candidates.sort_by_key(|p| {
        if p.to_string_lossy().ends_with(".default-release") {
            0u8
        } else {
            1u8
        }
    });

    candidates.into_iter().next().map(|p| p.join("places.sqlite"))
}

/// Return the macOS path to the Safari history database.
pub fn safari_history_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join("Library").join("Safari").join("History.db"))
}

/// Resolve the history database path for the named browser.
pub fn history_db_path(browser: &str) -> Option<PathBuf> {
    match browser.to_lowercase().as_str() {
        "chrome" => chrome_history_path(),
        "firefox" => firefox_history_path(),
        "safari" => safari_history_path(),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Locked-DB handling
// ────────────────────────────────────────────────────────────────────────────

/// Try to open a SQLite database read-only.  If the file appears locked (or
/// returns a `SQLITE_BUSY` / `SQLITE_LOCKED` error), copy the file to a
/// temporary location and open the copy instead.
///
/// Returns `(Connection, Option<tempfile::NamedTempFile>)` — the caller must
/// keep the `NamedTempFile` alive for the duration of use; dropping it deletes
/// the copy.
fn open_readonly(
    path: &Path,
) -> Result<(Connection, Option<tempfile::TempPath>), BrowserWatcherError> {
    if !path.exists() {
        return Err(BrowserWatcherError::DatabaseNotFound(path.to_owned()));
    }

    // First, try opening directly (works when the browser is not running).
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    match Connection::open_with_flags(path, flags) {
        Ok(conn) => {
            // Use WAL mode hint — this is a no-op on read-only connections but
            // surfaces compatibility issues early.
            let _ = conn.pragma_update(None, "journal_mode", "WAL");
            return Ok((conn, None));
        }
        Err(e) => {
            debug!(
                "Direct open of {} failed ({}), trying copy approach",
                path.display(),
                e
            );
        }
    }

    // Fallback: copy the database to a temp file and open the copy.
    let tmp = tempfile::NamedTempFile::new()?;
    std::fs::copy(path, tmp.path())?;
    let tmp_path = tmp.into_temp_path();

    let conn = Connection::open_with_flags(&tmp_path, flags)
        .map_err(|e| BrowserWatcherError::DatabaseError(e.to_string()))?;

    Ok((conn, Some(tmp_path)))
}

// ────────────────────────────────────────────────────────────────────────────
// Per-browser query helpers
// ────────────────────────────────────────────────────────────────────────────

/// Chrome stores visit times as microseconds since 1601-01-01 00:00:00 UTC.
fn chrome_timestamp_to_utc(chrome_ts: i64) -> DateTime<Utc> {
    // Chrome epoch: 1601-01-01
    const CHROME_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;
    let unix_micros = chrome_ts - CHROME_EPOCH_OFFSET_MICROS;
    let secs = unix_micros / 1_000_000;
    let nanos = ((unix_micros % 1_000_000) * 1_000) as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .unwrap_or_else(Utc::now)
}

/// Firefox stores visit dates as microseconds since the Unix epoch.
fn firefox_timestamp_to_utc(firefox_ts: i64) -> DateTime<Utc> {
    let secs = firefox_ts / 1_000_000;
    let nanos = ((firefox_ts % 1_000_000) * 1_000) as u32;
    Utc.timestamp_opt(secs, nanos)
        .single()
        .unwrap_or_else(Utc::now)
}

/// Safari stores visit dates as seconds since 2001-01-01 (Core Data epoch).
fn safari_timestamp_to_utc(safari_ts: f64) -> DateTime<Utc> {
    // Core Data epoch offset: 2001-01-01 is 978_307_200 seconds after Unix epoch.
    const CORE_DATA_OFFSET: f64 = 978_307_200.0;
    let unix_secs = safari_ts + CORE_DATA_OFFSET;
    Utc.timestamp_opt(unix_secs as i64, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

fn query_chrome(
    conn: &Connection,
    since: &DateTime<Utc>,
) -> Result<Vec<BrowserVisit>, BrowserWatcherError> {
    const CHROME_EPOCH_OFFSET_MICROS: i64 = 11_644_473_600_000_000;
    let since_chrome = since.timestamp_micros() + CHROME_EPOCH_OFFSET_MICROS;

    let mut stmt = conn.prepare(
        "SELECT urls.url, urls.title, visits.visit_time \
         FROM visits \
         JOIN urls ON visits.url = urls.id \
         WHERE visits.visit_time > ?1 \
         ORDER BY visits.visit_time ASC",
    )?;

    let visits = stmt
        .query_map([since_chrome], |row| {
            let url: String = row.get(0)?;
            let title: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            let ts: i64 = row.get(2)?;
            Ok((url, title, ts))
        })?
        .filter_map(|r| r.ok())
        .map(|(url, title, ts)| BrowserVisit {
            url,
            title,
            visited_at: chrome_timestamp_to_utc(ts),
            browser: "chrome".to_string(),
        })
        .collect();

    Ok(visits)
}

fn query_firefox(
    conn: &Connection,
    since: &DateTime<Utc>,
) -> Result<Vec<BrowserVisit>, BrowserWatcherError> {
    let since_firefox = since.timestamp_micros();

    let mut stmt = conn.prepare(
        "SELECT p.url, p.title, h.visit_date \
         FROM moz_historyvisits h \
         JOIN moz_places p ON h.place_id = p.id \
         WHERE h.visit_date > ?1 \
         ORDER BY h.visit_date ASC",
    )?;

    let visits = stmt
        .query_map([since_firefox], |row| {
            let url: String = row.get(0)?;
            let title: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            let ts: i64 = row.get(2)?;
            Ok((url, title, ts))
        })?
        .filter_map(|r| r.ok())
        .map(|(url, title, ts)| BrowserVisit {
            url,
            title,
            visited_at: firefox_timestamp_to_utc(ts),
            browser: "firefox".to_string(),
        })
        .collect();

    Ok(visits)
}

fn query_safari(
    conn: &Connection,
    since: &DateTime<Utc>,
) -> Result<Vec<BrowserVisit>, BrowserWatcherError> {
    // Core Data epoch offset
    const CORE_DATA_OFFSET: f64 = 978_307_200.0;
    let since_safari = since.timestamp() as f64 - CORE_DATA_OFFSET;

    // Safari stores data across two tables: history_items (urls) and
    // history_visits (timestamps + titles).
    let mut stmt = conn.prepare(
        "SELECT hi.url, hv.title, hv.visit_time \
         FROM history_visits hv \
         JOIN history_items hi ON hv.history_item = hi.id \
         WHERE hv.visit_time > ?1 \
         ORDER BY hv.visit_time ASC",
    )?;

    let visits = stmt
        .query_map([since_safari], |row| {
            let url: String = row.get(0)?;
            let title: String = row.get::<_, Option<String>>(1)?.unwrap_or_default();
            let ts: f64 = row.get(2)?;
            Ok((url, title, ts))
        })?
        .filter_map(|r| r.ok())
        .map(|(url, title, ts)| BrowserVisit {
            url,
            title,
            visited_at: safari_timestamp_to_utc(ts),
            browser: "safari".to_string(),
        })
        .collect();

    Ok(visits)
}

// ────────────────────────────────────────────────────────────────────────────
// BrowserWatcher
// ────────────────────────────────────────────────────────────────────────────

/// Monitors browser history databases for new entries.
///
/// Call [`BrowserWatcher::poll`] on a timer (e.g. with `tokio::time::interval`)
/// to collect new visits since the last poll.
pub struct BrowserWatcher {
    config: BrowserWatcherConfig,
    /// Last-seen timestamp per browser (to avoid re-processing old entries).
    last_seen: HashMap<String, DateTime<Utc>>,
}

impl BrowserWatcher {
    /// Create a new `BrowserWatcher` from the given configuration.
    pub fn new(config: BrowserWatcherConfig) -> Self {
        Self {
            config,
            last_seen: HashMap::new(),
        }
    }

    /// The configured polling interval.
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.config.poll_interval_secs)
    }

    /// Whether this watcher is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Return a list of configured browser names.
    pub fn configured_browsers(&self) -> &[String] {
        &self.config.browsers
    }

    /// Check whether a URL should be excluded based on the configured patterns.
    pub fn is_excluded(&self, url: &str) -> bool {
        self.config
            .exclude_patterns
            .iter()
            .any(|pat| url.contains(pat.as_str()))
    }

    /// Poll all configured browsers and return new visits since the last call.
    ///
    /// On the first call, uses `initial_since` as the lower bound (pass
    /// `Utc::now()` to ignore history that predates the watcher start).
    pub fn poll(&mut self, initial_since: DateTime<Utc>) -> Vec<BrowserVisit> {
        if !self.config.enabled {
            return Vec::new();
        }

        let mut all_visits: Vec<BrowserVisit> = Vec::new();

        for browser in self.config.browsers.clone() {
            let since = *self.last_seen.get(&browser).unwrap_or(&initial_since);

            match self.poll_browser(&browser, &since) {
                Ok(visits) => {
                    if let Some(latest) = visits.iter().map(|v| v.visited_at).max() {
                        self.last_seen.insert(browser.clone(), latest);
                    }
                    let non_excluded: Vec<_> = visits
                        .into_iter()
                        .filter(|v| !self.is_excluded(&v.url))
                        .collect();
                    info!(
                        browser = %browser,
                        count = non_excluded.len(),
                        "Browser poll complete"
                    );
                    all_visits.extend(non_excluded);
                }
                Err(BrowserWatcherError::DatabaseNotFound(path)) => {
                    debug!(
                        browser = %browser,
                        path = %path.display(),
                        "Browser history database not found — skipping"
                    );
                }
                Err(e) => {
                    warn!(browser = %browser, error = %e, "Failed to poll browser history");
                }
            }
        }

        all_visits
    }

    /// Poll a single browser's history database.
    fn poll_browser(
        &self,
        browser: &str,
        since: &DateTime<Utc>,
    ) -> Result<Vec<BrowserVisit>, BrowserWatcherError> {
        let db_path = history_db_path(browser)
            .ok_or_else(|| BrowserWatcherError::DatabaseError(format!("Unknown browser: {browser}")))?;

        let (conn, _tmp) = open_readonly(&db_path)?;

        match browser.to_lowercase().as_str() {
            "chrome" => query_chrome(&conn, since),
            "firefox" => query_firefox(&conn, since),
            "safari" => query_safari(&conn, since),
            other => Err(BrowserWatcherError::DatabaseError(format!(
                "Unsupported browser: {other}"
            ))),
        }
    }

    /// Format a `BrowserVisit` as a human-readable memory content string.
    pub fn visit_to_memory_content(visit: &BrowserVisit) -> String {
        if visit.title.is_empty() {
            format!("Visited: {}", visit.url)
        } else {
            format!("Visited: {} — {}", visit.title, visit.url)
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn default_config() -> BrowserWatcherConfig {
        BrowserWatcherConfig {
            enabled: true,
            browsers: vec!["chrome".to_string(), "firefox".to_string()],
            poll_interval_secs: 30,
            exclude_patterns: vec![
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "about:".to_string(),
                "chrome://".to_string(),
            ],
        }
    }

    // ── T1: watcher is disabled when config says so ──────────────────────────

    #[test]
    fn test_watcher_disabled_returns_empty() {
        let mut config = default_config();
        config.enabled = false;
        let mut watcher = BrowserWatcher::new(config);
        let visits = watcher.poll(Utc::now());
        assert!(visits.is_empty(), "Disabled watcher should return no visits");
    }

    // ── T2: exclusion pattern matching ───────────────────────────────────────

    #[test]
    fn test_is_excluded_matches_localhost() {
        let watcher = BrowserWatcher::new(default_config());
        assert!(watcher.is_excluded("http://localhost:3000/app"));
    }

    #[test]
    fn test_is_excluded_does_not_match_normal_url() {
        let watcher = BrowserWatcher::new(default_config());
        assert!(!watcher.is_excluded("https://www.rust-lang.org/learn"));
    }

    #[test]
    fn test_is_excluded_chrome_internal() {
        let watcher = BrowserWatcher::new(default_config());
        assert!(watcher.is_excluded("chrome://settings/"));
    }

    // ── T3: Chrome timestamp conversion ──────────────────────────────────────

    #[test]
    fn test_chrome_timestamp_conversion() {
        // Known reference: 2024-01-15 12:00:00 UTC
        // Unix timestamp: 1705320000
        // Chrome timestamp: unix_micros + 11_644_473_600_000_000
        let unix_micros: i64 = 1_705_320_000 * 1_000_000;
        let chrome_ts: i64 = unix_micros + 11_644_473_600_000_000;
        let dt = chrome_timestamp_to_utc(chrome_ts);
        assert_eq!(dt, Utc.timestamp_opt(1_705_320_000, 0).unwrap());
    }

    // ── T4: Firefox timestamp conversion ─────────────────────────────────────

    #[test]
    fn test_firefox_timestamp_conversion() {
        // Firefox uses microseconds since Unix epoch.
        let unix_micros: i64 = 1_705_320_000 * 1_000_000;
        let dt = firefox_timestamp_to_utc(unix_micros);
        assert_eq!(dt, Utc.timestamp_opt(1_705_320_000, 0).unwrap());
    }

    // ── T5: Safari timestamp conversion ──────────────────────────────────────

    #[test]
    fn test_safari_timestamp_conversion() {
        // Safari Core Data epoch is 2001-01-01 == Unix 978_307_200
        // A visit at Unix 1_705_320_000 → Safari ts = 1_705_320_000 - 978_307_200 = 727_012_800
        let safari_ts: f64 = 1_705_320_000.0 - 978_307_200.0;
        let dt = safari_timestamp_to_utc(safari_ts);
        assert_eq!(dt, Utc.timestamp_opt(1_705_320_000, 0).unwrap());
    }

    // ── T6: memory content formatting ────────────────────────────────────────

    #[test]
    fn test_visit_to_memory_content_with_title() {
        let visit = BrowserVisit {
            url: "https://example.com".to_string(),
            title: "Example Domain".to_string(),
            visited_at: Utc::now(),
            browser: "chrome".to_string(),
        };
        let content = BrowserWatcher::visit_to_memory_content(&visit);
        assert_eq!(content, "Visited: Example Domain — https://example.com");
    }

    #[test]
    fn test_visit_to_memory_content_without_title() {
        let visit = BrowserVisit {
            url: "https://example.com".to_string(),
            title: String::new(),
            visited_at: Utc::now(),
            browser: "firefox".to_string(),
        };
        let content = BrowserWatcher::visit_to_memory_content(&visit);
        assert_eq!(content, "Visited: https://example.com");
    }

    // ── T7: poll_interval reflects config ────────────────────────────────────

    #[test]
    fn test_poll_interval() {
        let mut config = default_config();
        config.poll_interval_secs = 120;
        let watcher = BrowserWatcher::new(config);
        assert_eq!(watcher.poll_interval(), Duration::from_secs(120));
    }

    // ── T8: history_db_path returns None for unknown browser ─────────────────

    #[test]
    fn test_history_db_path_unknown_browser() {
        assert!(
            history_db_path("opera_neon_ultra").is_none(),
            "Unknown browser should return None"
        );
    }

    // ── T9: open_readonly returns DatabaseNotFound for missing path ───────────

    #[test]
    fn test_open_readonly_missing_path() {
        let missing = PathBuf::from("/tmp/engram_nonexistent_history_db_abc123.sqlite");
        match open_readonly(&missing) {
            Err(BrowserWatcherError::DatabaseNotFound(_)) => {} // expected
            other => panic!("Expected DatabaseNotFound, got: {other:?}"),
        }
    }

    // ── T10: last_seen is updated after a successful poll ────────────────────

    #[test]
    fn test_last_seen_initialized_on_first_call() {
        // Even on a machine without Chrome/Firefox installed, the last_seen map
        // starts empty and poll() runs without panicking.
        let config = default_config();
        let mut watcher = BrowserWatcher::new(config);
        assert!(watcher.last_seen.is_empty());
        let _ = watcher.poll(Utc::now());
        // last_seen only grows if visits were returned; just check no panic.
    }

    // ── T11: configured_browsers returns the right list ──────────────────────

    #[test]
    fn test_configured_browsers() {
        let watcher = BrowserWatcher::new(default_config());
        assert_eq!(watcher.configured_browsers(), &["chrome", "firefox"]);
    }

    // ── T12: BrowserWatcherConfig default ────────────────────────────────────

    #[test]
    fn test_browser_watcher_config_default() {
        let config = BrowserWatcherConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.poll_interval_secs, 60);
        assert!(!config.browsers.is_empty());
        assert!(!config.exclude_patterns.is_empty());
    }
}
