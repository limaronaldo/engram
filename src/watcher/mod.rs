//! Watcher daemon — monitors directories, browser history, and app focus,
//! then stores observations as Engram memories.
//!
//! Enable with the `watcher` feature flag:
//! ```toml
//! [features]
//! watcher = ["dep:toml"]
//! ```

pub mod config;

pub use config::WatcherConfig;
