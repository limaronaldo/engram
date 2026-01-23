//! Real-time updates via WebSocket (RML-881)
//!
//! Provides push notifications for memory changes to connected clients.

mod events;
mod server;

pub use events::{EventType, RealtimeEvent};
pub use server::{RealtimeManager, RealtimeServer};
