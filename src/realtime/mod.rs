//! Real-time updates via WebSocket (RML-881)
//!
//! Provides push notifications for memory changes to connected clients.

pub(crate) mod events;
mod server;

pub use events::{EventType, RealtimeEvent, SubscriptionFilter};
pub use server::{RealtimeManager, RealtimeServer};
