//! Watcher daemon — monitors directories, browser history, and app focus,
//! then stores observations as Engram memories.
//!
//! Enable with the `watcher` feature flag:
//! ```toml
//! [features]
//! watcher = ["dep:toml"]
//! ```

pub mod app_focus;
pub mod browser;
pub mod config;
pub mod fs_watcher;

pub use config::WatcherConfig;
