//! Application Focus Watcher
//!
//! Tracks which applications are in the foreground and for how long, then
//! records focus sessions as Engram memories once the focus changes.
//!
//! ## How it works
//!
//! The watcher polls the foreground application at a configurable interval
//! (default: every 5 s).  When the foreground app changes it records the
//! *previous* focus session only if its duration meets the
//! `min_focus_secs` threshold (default: 1 s).  This filters out rapid
//! alt-tab switches and transient system popups.
//!
//! ### Platform support
//!
//! | Platform | Detection method |
//! |----------|-----------------|
//! | macOS    | `osascript -e 'tell application "System Events" to get name of first process whose frontmost is true'` |
//! | Other    | Not supported — `poll_foreground_app` always returns `None` |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use engram::watcher::{app_focus::AppFocusWatcher, config::AppFocusConfig};
//!
//! let config = AppFocusConfig {
//!     enabled: true,
//!     poll_interval_secs: 5,
//!     min_focus_secs: 2,
//!     exclude_apps: vec!["Finder".to_string()],
//! };
//!
//! let mut watcher = AppFocusWatcher::new(config);
//! watcher.tick(); // call periodically
//! let events = watcher.drain_completed_events();
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::time::Duration;

use crate::watcher::config::AppFocusConfig;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A completed application focus session — the app was in the foreground
/// from `started_at` until `ended_at`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FocusEvent {
    /// Name of the application (as reported by the OS).
    pub app_name: String,

    /// Window title, if available (macOS: not yet populated, always `None`).
    pub window_title: Option<String>,

    /// When focus on this app began.
    pub started_at: DateTime<Utc>,

    /// When focus on this app ended (i.e. another app took focus).
    pub ended_at: DateTime<Utc>,

    /// Duration of the focus session in seconds.
    pub duration_secs: u64,
}

impl FocusEvent {
    /// Render the event as a human-readable memory content string.
    pub fn to_memory_content(&self) -> String {
        let window_part = self
            .window_title
            .as_deref()
            .map(|t| format!(" ({})", t))
            .unwrap_or_default();

        format!(
            "App focus: {}{} — {} seconds (from {} to {})",
            self.app_name,
            window_part,
            self.duration_secs,
            self.started_at.format("%Y-%m-%dT%H:%M:%SZ"),
            self.ended_at.format("%Y-%m-%dT%H:%M:%SZ"),
        )
    }
}

// ---------------------------------------------------------------------------
// Internal state
// ---------------------------------------------------------------------------

/// Tracks the currently active focus session (app that is currently in
/// the foreground) and accumulates completed events.
#[derive(Debug)]
struct ActiveFocus {
    app_name: String,
    started_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// AppFocusWatcher
// ---------------------------------------------------------------------------

/// Watches which application is in the foreground and records focus sessions.
///
/// Call [`AppFocusWatcher::tick`] at regular intervals (driven by your own
/// event loop or timer).  Completed events are buffered internally; call
/// [`AppFocusWatcher::drain_completed_events`] to retrieve and clear them.
pub struct AppFocusWatcher {
    config: AppFocusConfig,
    active: Option<ActiveFocus>,
    completed: Vec<FocusEvent>,
    /// Injected command runner — replaced in tests to avoid real `osascript`.
    command_runner: Box<dyn AppNameProvider + Send + Sync>,
}

impl std::fmt::Debug for AppFocusWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppFocusWatcher")
            .field("config", &self.config)
            .field("active", &self.active)
            .field("completed_count", &self.completed.len())
            .finish()
    }
}

impl AppFocusWatcher {
    /// Create a new watcher with the given configuration.
    ///
    /// Uses the real OS command runner by default.
    pub fn new(config: AppFocusConfig) -> Self {
        Self {
            config,
            active: None,
            completed: Vec::new(),
            command_runner: Box::new(OsCommandRunner),
        }
    }

    /// Create a watcher with a custom [`AppNameProvider`] — primarily useful
    /// for testing without spawning real subprocesses.
    pub fn with_runner(
        config: AppFocusConfig,
        runner: Box<dyn AppNameProvider + Send + Sync>,
    ) -> Self {
        Self {
            config,
            active: None,
            completed: Vec::new(),
            command_runner: runner,
        }
    }

    /// Poll the foreground application once and update internal state.
    ///
    /// If the foreground app has changed and the previous session meets the
    /// `min_focus_secs` threshold it is moved to the completed events buffer.
    ///
    /// Returns `true` when a new completed event was buffered.
    pub fn tick(&mut self) -> bool {
        if !self.config.enabled {
            return false;
        }

        let now = Utc::now();
        let current_app = match self.command_runner.current_app() {
            Some(name) => name,
            None => return false,
        };

        // Skip excluded apps.
        if self.is_excluded(&current_app) {
            // If we had an active session on an excluded app, just clear it.
            self.active = None;
            return false;
        }

        let mut new_event = false;

        match self.active.take() {
            None => {
                // No previous session — start tracking this app.
                self.active = Some(ActiveFocus {
                    app_name: current_app,
                    started_at: now,
                });
            }
            Some(prev) if prev.app_name == current_app => {
                // Same app still in focus — restore the active session.
                self.active = Some(prev);
            }
            Some(prev) => {
                // App changed — finalise the previous session if it's long enough.
                let duration = now
                    .signed_duration_since(prev.started_at)
                    .num_seconds()
                    .max(0) as u64;

                if duration >= self.config.min_focus_secs {
                    self.completed.push(FocusEvent {
                        app_name: prev.app_name,
                        window_title: None,
                        started_at: prev.started_at,
                        ended_at: now,
                        duration_secs: duration,
                    });
                    new_event = true;
                }

                // Start tracking the new app.
                self.active = Some(ActiveFocus {
                    app_name: current_app,
                    started_at: now,
                });
            }
        }

        new_event
    }

    /// Drain and return all completed focus events, clearing the internal
    /// buffer.
    pub fn drain_completed_events(&mut self) -> Vec<FocusEvent> {
        std::mem::take(&mut self.completed)
    }

    /// Return a reference to the current (in-progress) focus session, if any.
    pub fn current_app(&self) -> Option<&str> {
        self.active.as_ref().map(|a| a.app_name.as_str())
    }

    /// Return the configured poll interval as a [`Duration`].
    pub fn poll_interval(&self) -> Duration {
        Duration::from_secs(self.config.poll_interval_secs)
    }

    /// Check whether an app name is in the exclusion list (case-insensitive).
    fn is_excluded(&self, app_name: &str) -> bool {
        let lower = app_name.to_lowercase();
        self.config
            .exclude_apps
            .iter()
            .any(|ex| ex.to_lowercase() == lower)
    }
}

// ---------------------------------------------------------------------------
// Platform abstraction: AppNameProvider
// ---------------------------------------------------------------------------

/// Abstraction over the OS mechanism for detecting the foreground application.
///
/// The default implementation shells out to `osascript` on macOS.
/// Tests supply a mock that returns a predetermined sequence.
pub trait AppNameProvider {
    /// Return the name of the currently active (frontmost) application, or
    /// `None` if the name cannot be determined.
    fn current_app(&self) -> Option<String>;
}

// ---------------------------------------------------------------------------
// Real macOS implementation
// ---------------------------------------------------------------------------

struct OsCommandRunner;

impl AppNameProvider for OsCommandRunner {
    fn current_app(&self) -> Option<String> {
        poll_foreground_app()
    }
}

/// Query macOS for the name of the frontmost application.
///
/// Returns `None` on non-macOS platforms or when the `osascript` invocation
/// fails (e.g. user has not granted Accessibility/Automation permissions).
pub fn poll_foreground_app() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("osascript")
            .args([
                "-e",
                r#"tell application "System Events" to get name of first process whose frontmost is true"#,
            ])
            .output()
            .ok()?;

        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        } else {
            tracing::debug!(
                stderr = %String::from_utf8_lossy(&output.stderr),
                "osascript returned non-zero exit code"
            );
            None
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Test helper: a mock AppNameProvider backed by a queue of names.
    // ------------------------------------------------------------------

    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    struct MockRunner {
        /// Queue of app names to return in order.  `None` entries simulate
        /// failures (e.g. osascript not available).
        queue: Arc<Mutex<VecDeque<Option<String>>>>,
    }

    impl MockRunner {
        fn from_names(names: Vec<Option<&str>>) -> Self {
            let deque = names
                .into_iter()
                .map(|n| n.map(|s| s.to_string()))
                .collect();
            Self {
                queue: Arc::new(Mutex::new(deque)),
            }
        }
    }

    impl AppNameProvider for MockRunner {
        fn current_app(&self) -> Option<String> {
            self.queue.lock().unwrap().pop_front().flatten()
        }
    }

    fn default_config() -> AppFocusConfig {
        AppFocusConfig {
            enabled: true,
            poll_interval_secs: 5,
            min_focus_secs: 1,
            exclude_apps: Vec::new(),
        }
    }

    // ------------------------------------------------------------------
    // Test 1: First tick starts tracking without emitting an event.
    // ------------------------------------------------------------------

    #[test]
    fn test_first_tick_starts_tracking_no_event() {
        let runner = MockRunner::from_names(vec![Some("Safari")]);
        let mut watcher = AppFocusWatcher::with_runner(default_config(), Box::new(runner));

        let emitted = watcher.tick();

        assert!(!emitted, "first tick should not emit a completed event");
        assert_eq!(watcher.current_app(), Some("Safari"));
        assert!(watcher.drain_completed_events().is_empty());
    }

    // ------------------------------------------------------------------
    // Test 2: App switch emits a completed event for the previous session.
    // ------------------------------------------------------------------

    #[test]
    fn test_app_switch_emits_event() {
        // tick 1 → Safari, tick 2 → Terminal
        // The time gap between ticks is at least 1s due to the real clock;
        // we set min_focus_secs to 0 so we always record regardless of gap.
        let mut config = default_config();
        config.min_focus_secs = 0;

        let runner = MockRunner::from_names(vec![Some("Safari"), Some("Terminal")]);
        let mut watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

        watcher.tick(); // start Safari session
        let emitted = watcher.tick(); // switch to Terminal → finalise Safari

        assert!(emitted, "app switch should emit a completed event");
        let events = watcher.drain_completed_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].app_name, "Safari");
        assert_eq!(watcher.current_app(), Some("Terminal"));
    }

    // ------------------------------------------------------------------
    // Test 3: Same app across multiple ticks produces no completed events.
    // ------------------------------------------------------------------

    #[test]
    fn test_same_app_no_event() {
        let runner = MockRunner::from_names(vec![Some("VSCode"), Some("VSCode"), Some("VSCode")]);
        let mut watcher = AppFocusWatcher::with_runner(default_config(), Box::new(runner));

        watcher.tick();
        watcher.tick();
        watcher.tick();

        assert!(watcher.drain_completed_events().is_empty());
        assert_eq!(watcher.current_app(), Some("VSCode"));
    }

    // ------------------------------------------------------------------
    // Test 4: Events shorter than min_focus_secs are dropped.
    // ------------------------------------------------------------------

    #[test]
    fn test_sub_threshold_events_are_dropped() {
        // We cannot control wall-clock time, so we set min_focus_secs to a
        // very large value to guarantee the (near-instant) tick gap is below
        // the threshold.
        let mut config = default_config();
        config.min_focus_secs = 3600; // 1 hour — no real test will run that long

        let runner = MockRunner::from_names(vec![Some("App A"), Some("App B")]);
        let mut watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

        watcher.tick(); // start App A
        let emitted = watcher.tick(); // switch to App B, but duration < threshold

        assert!(!emitted, "short focus sessions should be dropped");
        assert!(watcher.drain_completed_events().is_empty());
    }

    // ------------------------------------------------------------------
    // Test 5: Excluded apps are skipped and do not appear in events.
    // ------------------------------------------------------------------

    #[test]
    fn test_excluded_apps_are_skipped() {
        let mut config = default_config();
        config.exclude_apps = vec!["Finder".to_string(), "loginwindow".to_string()];
        config.min_focus_secs = 0;

        // Sequence: Finder (excluded) → Safari → Finder (excluded) → Terminal
        let runner = MockRunner::from_names(vec![
            Some("Finder"),
            Some("Safari"),
            Some("Finder"),
            Some("Terminal"),
        ]);
        let mut watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

        watcher.tick(); // Finder — excluded, active cleared
        watcher.tick(); // Safari — starts tracking
        watcher.tick(); // Finder — excluded, Safari session finalised without an event
                        //          (because active is cleared, not emitted)
        watcher.tick(); // Terminal — starts tracking (no previous to finalise)

        // No completed events because the only app switch that could emit
        // something went from Safari → Finder (excluded destination means we
        // clear active without recording Finder as new app).
        // Actually Safari → Finder: Finder is excluded so we clear active.
        // Then Terminal: no active → starts tracking.
        // Net: 0 events expected (Safari session was lost, not recorded, because
        // the new "app" was excluded).
        let events = watcher.drain_completed_events();
        for ev in &events {
            assert_ne!(
                ev.app_name.to_lowercase(),
                "finder",
                "excluded apps must not appear in events"
            );
            assert_ne!(
                ev.app_name.to_lowercase(),
                "loginwindow",
                "excluded apps must not appear in events"
            );
        }
    }

    // ------------------------------------------------------------------
    // Test 6: Disabled watcher never emits events regardless of app changes.
    // ------------------------------------------------------------------

    #[test]
    fn test_disabled_watcher_never_emits() {
        let mut config = default_config();
        config.enabled = false;

        let runner = MockRunner::from_names(vec![Some("Safari"), Some("Terminal")]);
        let mut watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

        let e1 = watcher.tick();
        let e2 = watcher.tick();

        assert!(!e1);
        assert!(!e2);
        assert!(
            watcher.current_app().is_none(),
            "disabled watcher should not track"
        );
        assert!(watcher.drain_completed_events().is_empty());
    }

    // ------------------------------------------------------------------
    // Test 7: FocusEvent::to_memory_content formats correctly.
    // ------------------------------------------------------------------

    #[test]
    fn test_focus_event_to_memory_content() {
        let started = "2026-03-09T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
        let ended = "2026-03-09T10:05:30Z".parse::<DateTime<Utc>>().unwrap();
        let duration = ended.signed_duration_since(started).num_seconds() as u64;

        let event = FocusEvent {
            app_name: "Xcode".to_string(),
            window_title: Some("MyProject".to_string()),
            started_at: started,
            ended_at: ended,
            duration_secs: duration,
        };

        let content = event.to_memory_content();
        assert!(content.contains("Xcode"), "content should include app name");
        assert!(
            content.contains("MyProject"),
            "content should include window title"
        );
        assert!(
            content.contains("330"),
            "content should include duration in seconds"
        );
    }

    // ------------------------------------------------------------------
    // Test 8: Runner returning None does not crash and is a no-op.
    // ------------------------------------------------------------------

    #[test]
    fn test_none_app_name_is_no_op() {
        let runner = MockRunner::from_names(vec![None, None]);
        let mut watcher = AppFocusWatcher::with_runner(default_config(), Box::new(runner));

        let e1 = watcher.tick();
        let e2 = watcher.tick();

        assert!(!e1);
        assert!(!e2);
        assert!(watcher.current_app().is_none());
        assert!(watcher.drain_completed_events().is_empty());
    }

    // ------------------------------------------------------------------
    // Test 9: poll_interval returns the configured value.
    // ------------------------------------------------------------------

    #[test]
    fn test_poll_interval() {
        let mut config = default_config();
        config.poll_interval_secs = 10;
        let runner = MockRunner::from_names(vec![]);
        let watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

        assert_eq!(watcher.poll_interval(), Duration::from_secs(10));
    }

    // ------------------------------------------------------------------
    // Test 10: Multiple completed events accumulate before drain.
    // ------------------------------------------------------------------

    #[test]
    fn test_multiple_events_accumulate() {
        let mut config = default_config();
        config.min_focus_secs = 0;

        let runner = MockRunner::from_names(vec![Some("App A"), Some("App B"), Some("App C")]);
        let mut watcher = AppFocusWatcher::with_runner(config, Box::new(runner));

        watcher.tick(); // start App A
        watcher.tick(); // switch to App B → emit App A event
        watcher.tick(); // switch to App C → emit App B event

        let events = watcher.drain_completed_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].app_name, "App A");
        assert_eq!(events[1].app_name, "App B");

        // After draining the buffer is empty.
        assert!(watcher.drain_completed_events().is_empty());
    }
}
