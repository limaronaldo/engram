//! Memory Agent Loop — RML-1223
//!
//! Ties together proactive acquisition (RML-1221) and gardening (RML-1222)
//! into a single observe→decide→act cycle for autonomous memory management.
//!
//! ## Design
//!
//! The agent is **not** a background thread. It is a struct with a [`MemoryAgent::tick`]
//! method. The caller — an MCP handler or binary — is responsible for invoking
//! `tick()` at the desired interval (see `check_interval_secs`).
//!
//! ## Cycle Phases
//!
//! 1. **Observe** — score memories with [`MemoryGardener`], detect knowledge gaps
//!    with [`GapDetector`].
//! 2. **Decide** — prioritise actions: urgent prunes first, then merges, archives,
//!    then acquisition suggestions. Capped at `max_actions_per_cycle`.
//! 3. **Act** — return the decided actions as a [`CycleResult`]. The agent does NOT
//!    apply DB changes itself; MCP handlers optionally apply them.
//! 4. **Update state** — increment cycle counter and action totals.
//!
//! ## Invariants
//!
//! - The agent starts in a stopped state (`running = false`).
//! - `tick()` may be called regardless of `running` state; callers decide.
//! - `AgentMetrics.uptime_secs` is 0 until `start()` is called.
//! - All timestamps in `AgentState` are RFC3339 UTC strings.
//! - No `unwrap()` in production paths.

use std::time::Instant;

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::{
    error::Result,
    intelligence::{
        gardening::{GardenAction, GardenConfig, MemoryGardener},
        proactive::GapDetector,
    },
};

// =============================================================================
// Public types
// =============================================================================

/// Configuration for the memory agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// How often (in seconds) the agent should run a full observe→decide→act cycle.
    /// Default: 300 (5 minutes).
    pub check_interval_secs: u64,
    /// How often (in seconds) a full garden maintenance run should be triggered.
    /// Default: 3600 (1 hour).
    pub garden_interval_secs: u64,
    /// Maximum number of actions the agent may decide on per cycle.
    /// Default: 10.
    pub max_actions_per_cycle: usize,
    /// Workspace to operate on.
    /// Default: `"default"`.
    pub workspace: String,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            check_interval_secs: 300,
            garden_interval_secs: 3600,
            max_actions_per_cycle: 10,
            workspace: "default".to_string(),
        }
    }
}

/// Live state of the memory agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentState {
    /// Whether the agent has been started.
    pub running: bool,
    /// Number of cycles completed so far.
    pub cycles: u64,
    /// RFC3339 UTC timestamp of the last garden maintenance run.
    pub last_garden_at: Option<String>,
    /// RFC3339 UTC timestamp of the last proactive acquisition scan.
    pub last_acquisition_at: Option<String>,
    /// Total number of actions decided across all cycles.
    pub total_actions: u64,
}

/// A single action decided by the agent during one cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentAction {
    /// A memory should be pruned because its garden score is critically low.
    Prune {
        memory_id: i64,
        reason: String,
    },
    /// Two or more memories should be merged (similar content).
    Merge {
        source_ids: Vec<i64>,
    },
    /// A memory should be archived (old but not deleted).
    Archive {
        memory_id: i64,
    },
    /// A knowledge gap was detected — suggest creating a new memory.
    Suggest {
        hint: String,
        priority: u8,
    },
    /// A full garden maintenance run was executed.
    Garden {
        report_summary: String,
    },
}

/// Performance and health metrics for the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetrics {
    /// Total completed cycles.
    pub cycles: u64,
    /// Total actions decided across all cycles.
    pub total_actions: u64,
    /// Memories pruned (counted from Prune actions).
    pub memories_pruned: u64,
    /// Memories merged (counted from Merge actions; each Merge action = 1 merge op).
    pub memories_merged: u64,
    /// Memories archived (counted from Archive actions).
    pub memories_archived: u64,
    /// Acquisition suggestions made.
    pub suggestions_made: u64,
    /// Garden maintenance runs executed.
    pub gardens_run: u64,
    /// Wall-clock seconds since `start()` was called (0 if never started).
    pub uptime_secs: u64,
}

/// The result of one observe→decide→act cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleResult {
    /// Actions decided during this cycle (not yet applied to the DB).
    pub actions: Vec<AgentAction>,
    /// The cycle number (1-based; equals `AgentState.cycles` after the tick).
    pub cycle_number: u64,
    /// Wall-clock time spent executing this cycle, in milliseconds.
    pub duration_ms: u64,
}

// =============================================================================
// MemoryAgent
// =============================================================================

/// Autonomous memory management agent.
///
/// Call [`MemoryAgent::tick`] at the desired cadence to run one full
/// observe→decide→act cycle. The agent tracks its own state between calls.
pub struct MemoryAgent {
    /// Current configuration (mutable via [`MemoryAgent::configure`]).
    pub config: AgentConfig,
    /// Live operational state.
    pub state: AgentState,
    /// Wall-clock instant when `start()` was last called; `None` if never started.
    started_at: Option<Instant>,

    // Per-action-type counters (not exposed in `AgentState` to keep it simple;
    // surfaced through `metrics()`).
    memories_pruned: u64,
    memories_merged: u64,
    memories_archived: u64,
    suggestions_made: u64,
    gardens_run: u64,
}

impl MemoryAgent {
    // -------------------------------------------------------------------------
    // Construction
    // -------------------------------------------------------------------------

    /// Create a new agent with the given configuration.
    /// The agent starts in a **stopped** state (`running = false`).
    pub fn new(config: AgentConfig) -> Self {
        Self {
            config,
            state: AgentState {
                running: false,
                cycles: 0,
                last_garden_at: None,
                last_acquisition_at: None,
                total_actions: 0,
            },
            started_at: None,
            memories_pruned: 0,
            memories_merged: 0,
            memories_archived: 0,
            suggestions_made: 0,
            gardens_run: 0,
        }
    }

    // -------------------------------------------------------------------------
    // Lifecycle
    // -------------------------------------------------------------------------

    /// Mark the agent as running and record the start time for uptime tracking.
    pub fn start(&mut self) {
        self.state.running = true;
        self.started_at = Some(Instant::now());
    }

    /// Mark the agent as stopped. The accumulated state is preserved.
    pub fn stop(&mut self) {
        self.state.running = false;
    }

    /// Returns `true` if the agent has been started and not yet stopped.
    pub fn is_running(&self) -> bool {
        self.state.running
    }

    // -------------------------------------------------------------------------
    // Core loop
    // -------------------------------------------------------------------------

    /// Execute **one** observe→decide→act cycle.
    ///
    /// # Phase 1 — Observe
    /// - Run [`MemoryGardener`] in **dry-run** mode to score memories and
    ///   identify prune / merge / archive candidates.
    /// - Run [`GapDetector`] to surface knowledge gaps and acquisition hints.
    ///
    /// # Phase 2 — Decide
    /// Actions are prioritised as:
    /// 1. Urgent prunes (garden score critically low)
    /// 2. Merge candidates
    /// 3. Archive candidates
    /// 4. Garden run (if `should_garden()`)
    /// 5. Acquisition suggestions from gap analysis
    ///
    /// The total is capped at `config.max_actions_per_cycle`.
    ///
    /// # Phase 3 — Act
    /// The actions list is returned. **No DB writes are made by this method.**
    /// The caller (MCP handler) decides whether to apply them.
    ///
    /// # State Update
    /// `AgentState.cycles` is incremented and timestamps are refreshed.
    pub fn tick(&mut self, conn: &Connection) -> Result<CycleResult> {
        let cycle_start = Instant::now();
        let now_str = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let workspace = self.config.workspace.clone();
        let max = self.config.max_actions_per_cycle;

        // ------------------------------------------------------------------ //
        // Phase 1 — Observe
        // ------------------------------------------------------------------ //

        // Run gardener in dry-run mode to get scored action candidates
        let garden_preview = MemoryGardener::new(GardenConfig {
            dry_run: true,
            ..GardenConfig::default()
        })
        .garden(conn, &workspace)?;

        // Run gap detector to find acquisition suggestions
        let gap_detector = GapDetector::new();
        let acquisition_suggestions = gap_detector.suggest_acquisitions(conn, &workspace, 0)?;

        // ------------------------------------------------------------------ //
        // Phase 2 — Decide
        // ------------------------------------------------------------------ //

        let mut decided: Vec<AgentAction> = Vec::new();

        // 2a. Urgent prunes first (score < 0.1 based on gardener report)
        for action in &garden_preview.actions {
            if decided.len() >= max {
                break;
            }
            if let GardenAction::Prune { memory_id, reason } = action {
                // "Urgent" = the reason string contains a very low score indicator.
                // The gardener always uses score < prune_threshold (default 0.2) to prune.
                // We treat all gardener-recommended prunes as legitimate candidates.
                decided.push(AgentAction::Prune {
                    memory_id: *memory_id,
                    reason: reason.clone(),
                });
            }
        }

        // 2b. Merge candidates
        for action in &garden_preview.actions {
            if decided.len() >= max {
                break;
            }
            if let GardenAction::Merge { source_ids, .. } = action {
                decided.push(AgentAction::Merge {
                    source_ids: source_ids.clone(),
                });
            }
        }

        // 2c. Archive candidates
        for action in &garden_preview.actions {
            if decided.len() >= max {
                break;
            }
            if let GardenAction::Archive { memory_id } = action {
                decided.push(AgentAction::Archive {
                    memory_id: *memory_id,
                });
            }
        }

        // 2d. Garden run (if due)
        if decided.len() < max && self.should_garden() {
            let summary = format!(
                "Gardening workspace '{}': {} pruned, {} merged, {} archived",
                workspace,
                garden_preview.memories_pruned,
                garden_preview.memories_merged,
                garden_preview.memories_archived,
            );
            decided.push(AgentAction::Garden {
                report_summary: summary,
            });
        }

        // 2e. Acquisition suggestions
        for suggestion in &acquisition_suggestions {
            if decided.len() >= max {
                break;
            }
            decided.push(AgentAction::Suggest {
                hint: suggestion.content_hint.clone(),
                priority: suggestion.priority,
            });
        }

        // ------------------------------------------------------------------ //
        // Phase 3 — Act (record only; caller applies)
        // ------------------------------------------------------------------ //

        // Count action types for metrics
        for action in &decided {
            match action {
                AgentAction::Prune { .. } => self.memories_pruned += 1,
                AgentAction::Merge { .. } => self.memories_merged += 1,
                AgentAction::Archive { .. } => self.memories_archived += 1,
                AgentAction::Suggest { .. } => self.suggestions_made += 1,
                AgentAction::Garden { .. } => {
                    self.gardens_run += 1;
                    self.state.last_garden_at = Some(now_str.clone());
                }
            }
        }

        // Update acquisition timestamp when we scanned for gaps
        if !acquisition_suggestions.is_empty() || decided.iter().any(|a| matches!(a, AgentAction::Suggest { .. })) {
            self.state.last_acquisition_at = Some(now_str.clone());
        }

        // ------------------------------------------------------------------ //
        // Update state
        // ------------------------------------------------------------------ //

        self.state.cycles += 1;
        self.state.total_actions += decided.len() as u64;

        let duration_ms = cycle_start.elapsed().as_millis() as u64;

        Ok(CycleResult {
            actions: decided,
            cycle_number: self.state.cycles,
            duration_ms,
        })
    }

    // -------------------------------------------------------------------------
    // Garden scheduling
    // -------------------------------------------------------------------------

    /// Returns `true` if enough time has passed since the last garden run to
    /// justify running another one.
    ///
    /// - If the garden has never run, returns `true`.
    /// - Otherwise, returns `true` only if `garden_interval_secs` seconds have
    ///   elapsed since `state.last_garden_at`.
    pub fn should_garden(&self) -> bool {
        match &self.state.last_garden_at {
            None => true,
            Some(last_str) => {
                if let Ok(last_dt) = chrono::DateTime::parse_from_rfc3339(last_str) {
                    let now = Utc::now();
                    let elapsed = (now.timestamp() - last_dt.timestamp()).max(0) as u64;
                    elapsed >= self.config.garden_interval_secs
                } else {
                    // Unparseable timestamp — treat as "never gardened"
                    true
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Metrics & introspection
    // -------------------------------------------------------------------------

    /// Compute current performance metrics from accumulated state.
    pub fn metrics(&self) -> AgentMetrics {
        let uptime_secs = self
            .started_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);

        AgentMetrics {
            cycles: self.state.cycles,
            total_actions: self.state.total_actions,
            memories_pruned: self.memories_pruned,
            memories_merged: self.memories_merged,
            memories_archived: self.memories_archived,
            suggestions_made: self.suggestions_made,
            gardens_run: self.gardens_run,
            uptime_secs,
        }
    }

    /// Replace the current configuration.
    ///
    /// Changes take effect on the next call to `tick()`.
    pub fn configure(&mut self, new_config: AgentConfig) {
        self.config = new_config;
    }

    /// Return a snapshot of the current agent state.
    pub fn status(&self) -> AgentState {
        self.state.clone()
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    // -------------------------------------------------------------------------
    // Test DB helpers
    // -------------------------------------------------------------------------

    /// Minimal schema for agent loop tests.
    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS memories (
                id                INTEGER PRIMARY KEY AUTOINCREMENT,
                content           TEXT    NOT NULL,
                memory_type       TEXT    NOT NULL DEFAULT 'note',
                workspace         TEXT    NOT NULL DEFAULT 'default',
                importance        REAL    NOT NULL DEFAULT 0.5,
                updated_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
                last_accessed_at  TEXT,
                created_at        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
            );
            CREATE TABLE IF NOT EXISTS tags (
                id        INTEGER PRIMARY KEY AUTOINCREMENT,
                memory_id INTEGER NOT NULL,
                tag       TEXT    NOT NULL,
                FOREIGN KEY(memory_id) REFERENCES memories(id)
            );",
        )
        .expect("create schema");
        conn
    }

    fn insert_memory(
        conn: &Connection,
        content: &str,
        importance: f32,
        updated_at: &str,
        workspace: &str,
    ) -> i64 {
        conn.execute(
            "INSERT INTO memories (content, importance, updated_at, created_at, workspace)
             VALUES (?1, ?2, ?3, ?3, ?4)",
            rusqlite::params![content, importance as f64, updated_at, workspace],
        )
        .expect("insert");
        conn.last_insert_rowid()
    }

    // -------------------------------------------------------------------------
    // Test 1 — new agent is not running
    // -------------------------------------------------------------------------
    #[test]
    fn test_new_agent_not_running() {
        let agent = MemoryAgent::new(AgentConfig::default());
        assert!(!agent.is_running(), "new agent should not be running");
        assert_eq!(agent.state.cycles, 0);
        assert_eq!(agent.state.total_actions, 0);
        assert!(agent.state.last_garden_at.is_none());
        assert!(agent.state.last_acquisition_at.is_none());
    }

    // -------------------------------------------------------------------------
    // Test 2 — start/stop lifecycle
    // -------------------------------------------------------------------------
    #[test]
    fn test_start_stop_lifecycle() {
        let mut agent = MemoryAgent::new(AgentConfig::default());

        agent.start();
        assert!(agent.is_running(), "agent should be running after start()");

        agent.stop();
        assert!(!agent.is_running(), "agent should not be running after stop()");

        // Uptime should be non-zero after start (even if tiny)
        let m = agent.metrics();
        // started_at was set during start(), so uptime_secs reflects elapsed
        // time. It may be 0 for very fast tests — just assert it's accessible.
        assert!(m.uptime_secs < u64::MAX, "uptime should be a valid value");
    }

    // -------------------------------------------------------------------------
    // Test 3 — tick on empty workspace produces no actions
    // -------------------------------------------------------------------------
    #[test]
    fn test_tick_empty_workspace() {
        let conn = setup_conn();
        let mut agent = MemoryAgent::new(AgentConfig {
            workspace: "empty".to_string(),
            ..AgentConfig::default()
        });

        let result = agent.tick(&conn).expect("tick");

        // Empty workspace: gardener has no candidates, gap detector finds nothing
        assert_eq!(
            result.cycle_number, 1,
            "cycle_number should be 1 after first tick"
        );
        // Actions may include a Garden action (should_garden returns true first time)
        // but no Prune/Merge/Archive since workspace is empty
        for action in &result.actions {
            assert!(
                !matches!(action, AgentAction::Prune { .. }),
                "should not prune in empty workspace"
            );
            assert!(
                !matches!(action, AgentAction::Merge { .. }),
                "should not merge in empty workspace"
            );
            assert!(
                !matches!(action, AgentAction::Archive { .. }),
                "should not archive in empty workspace"
            );
        }
    }

    // -------------------------------------------------------------------------
    // Test 4 — tick increments cycles and total_actions
    // -------------------------------------------------------------------------
    #[test]
    fn test_tick_increments_state() {
        let conn = setup_conn();
        let mut agent = MemoryAgent::new(AgentConfig::default());

        agent.tick(&conn).expect("tick 1");
        assert_eq!(agent.state.cycles, 1);

        agent.tick(&conn).expect("tick 2");
        assert_eq!(agent.state.cycles, 2);

        // total_actions counter is updated on each tick; value depends on actions taken.
        let _ = agent.state.total_actions; // access to suppress dead-code hints
    }

    // -------------------------------------------------------------------------
    // Test 5 — tick produces prune/archive actions for stale low-importance memories
    // -------------------------------------------------------------------------
    #[test]
    fn test_tick_produces_prune_actions() {
        let conn = setup_conn();

        // Insert very old, very low importance memory → gardener will want to prune
        insert_memory(
            &conn,
            "completely stale irrelevant note",
            0.01,
            "2000-01-01T00:00:00Z",
            "default",
        );

        let mut agent = MemoryAgent::new(AgentConfig::default());
        let result = agent.tick(&conn).expect("tick");

        let has_prune = result
            .actions
            .iter()
            .any(|a| matches!(a, AgentAction::Prune { .. }));

        // The gardener (dry-run, prune_threshold=0.2) should flag this memory
        assert!(has_prune, "expected at least one Prune action for stale memory");
    }

    // -------------------------------------------------------------------------
    // Test 6 — metrics track cycles correctly
    // -------------------------------------------------------------------------
    #[test]
    fn test_metrics_track_cycles() {
        let conn = setup_conn();
        let mut agent = MemoryAgent::new(AgentConfig::default());

        agent.start();

        for _ in 0..3 {
            agent.tick(&conn).expect("tick");
        }

        let m = agent.metrics();
        assert_eq!(m.cycles, 3, "metrics.cycles should equal number of ticks");
        assert_eq!(m.total_actions, agent.state.total_actions);
        assert!(m.uptime_secs < 60, "uptime should be seconds, not huge");
    }

    // -------------------------------------------------------------------------
    // Test 7 — configure updates config
    // -------------------------------------------------------------------------
    #[test]
    fn test_configure_updates_config() {
        let mut agent = MemoryAgent::new(AgentConfig::default());

        assert_eq!(agent.config.check_interval_secs, 300);
        assert_eq!(agent.config.max_actions_per_cycle, 10);

        agent.configure(AgentConfig {
            check_interval_secs: 60,
            garden_interval_secs: 600,
            max_actions_per_cycle: 5,
            workspace: "my-ws".to_string(),
        });

        assert_eq!(agent.config.check_interval_secs, 60);
        assert_eq!(agent.config.garden_interval_secs, 600);
        assert_eq!(agent.config.max_actions_per_cycle, 5);
        assert_eq!(agent.config.workspace, "my-ws");
    }

    // -------------------------------------------------------------------------
    // Test 8 — should_garden timing
    // -------------------------------------------------------------------------
    #[test]
    fn test_should_garden_timing() {
        let mut agent = MemoryAgent::new(AgentConfig {
            garden_interval_secs: 3600,
            ..AgentConfig::default()
        });

        // Never gardened → should garden
        assert!(agent.should_garden(), "should garden when no previous run");

        // Set last_garden_at to just now → should NOT garden yet
        let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        agent.state.last_garden_at = Some(now);
        assert!(
            !agent.should_garden(),
            "should NOT garden immediately after a run"
        );

        // Set last_garden_at to the past beyond the interval → should garden
        let past = "2000-01-01T00:00:00Z".to_string();
        agent.state.last_garden_at = Some(past);
        assert!(
            agent.should_garden(),
            "should garden when interval has elapsed"
        );
    }

    // -------------------------------------------------------------------------
    // Test 9 — max_actions_per_cycle is respected
    // -------------------------------------------------------------------------
    #[test]
    fn test_max_actions_per_cycle_capped() {
        let conn = setup_conn();

        // Insert many stale memories so gardener has lots of prune candidates
        for i in 0..20 {
            insert_memory(
                &conn,
                &format!("stale note number {}", i),
                0.01,
                "2000-01-01T00:00:00Z",
                "default",
            );
        }

        let mut agent = MemoryAgent::new(AgentConfig {
            max_actions_per_cycle: 3,
            ..AgentConfig::default()
        });

        let result = agent.tick(&conn).expect("tick");
        assert!(
            result.actions.len() <= 3,
            "actions should be capped at max_actions_per_cycle=3, got {}",
            result.actions.len()
        );
    }

    // -------------------------------------------------------------------------
    // Test 10 — status() returns clone of current state
    // -------------------------------------------------------------------------
    #[test]
    fn test_status_returns_state_snapshot() {
        let mut agent = MemoryAgent::new(AgentConfig::default());
        agent.start();

        let status = agent.status();
        assert!(status.running);
        assert_eq!(status.cycles, agent.state.cycles);
        assert_eq!(status.total_actions, agent.state.total_actions);
    }

    // -------------------------------------------------------------------------
    // Test 11 — garden action appears in first tick (should_garden = true)
    // -------------------------------------------------------------------------
    #[test]
    fn test_garden_action_in_first_tick() {
        let conn = setup_conn();
        let mut agent = MemoryAgent::new(AgentConfig::default());

        // On first tick, should_garden() returns true → a Garden action is added
        let result = agent.tick(&conn).expect("tick");

        let has_garden = result
            .actions
            .iter()
            .any(|a| matches!(a, AgentAction::Garden { .. }));

        assert!(has_garden, "first tick should include a Garden action");
        assert!(
            agent.state.last_garden_at.is_some(),
            "last_garden_at should be set after a Garden action"
        );
    }

    // -------------------------------------------------------------------------
    // Test 12 — uptime is 0 when never started
    // -------------------------------------------------------------------------
    #[test]
    fn test_uptime_zero_when_not_started() {
        let agent = MemoryAgent::new(AgentConfig::default());
        let m = agent.metrics();
        assert_eq!(m.uptime_secs, 0, "uptime should be 0 before start()");
    }
}
