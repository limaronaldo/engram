## Task: Advanced SSE (resumable streams)

### Approach

Three targeted changes to add resumable SSE support without breaking existing functionality:

1. Add `seq_id: Option<u64>` to `RealtimeEvent` — optional so existing callers don't need changes.
2. Add `AtomicU64` counter + `VecDeque` ring buffer to `RealtimeManager` — stamped on every `broadcast()` call.
3. Update `handle_events` in the HTTP transport to: parse `Last-Event-Id`, replay buffered events, emit `.id()` on each SSE event, and send a `retry: 3000` hint at connection start.

### Files Changed

- `src/realtime/events.rs` — Added `seq_id: Option<u64>` field to `RealtimeEvent` with `skip_serializing_if = "Option::is_none"`. Updated all constructor methods to initialize `seq_id: None`.

- `src/realtime/server.rs` — Added `next_seq_id: Arc<AtomicU64>`, `buffer: Arc<RwLock<VecDeque<RealtimeEvent>>>`, and `max_buffered_events: usize` to `RealtimeManager`. Added `with_buffer_size()` constructor. Updated `broadcast()` to stamp `seq_id` and push to ring buffer (evicting oldest when full). Added `get_events_after(last_seq_id)` public method for replay. Updated `Clone` impl to share the `Arc`-wrapped fields. Added 10 new unit tests.

- `src/mcp/http_transport.rs` — Added `RealtimeEvent` import. Added `SSE_RETRY_MS = 3000` constant. Added `realtime_event_to_sse()` helper that stamps `.id(seq_id)` when present. Rewrote `handle_events` to: parse `Last-Event-Id` header, subscribe to live channel first, drain replay events, chain replay burst → live stream, prepend `retry:` directive. Added 8 new tests for `Last-Event-Id` parsing, replay logic, and SSE formatting.

### Decisions Made

- `seq_id: Option<u64>` rather than required `u64` — preserves all existing construction sites; the manager stamps it.
- Subscribe to broadcast channel *before* draining the buffer — avoids a race where events arrive between the two operations.
- `Arc<AtomicU64>` and `Arc<RwLock<VecDeque>>` — shared across clones of `RealtimeManager` so all HTTP handler instances see the same counter and buffer.
- `retry:` sent as a synthetic SSE event at stream start — the SSE spec allows sending `retry:` lines at any point; sending it once at the beginning is idiomatic.
- Default buffer size 500 (with `with_buffer_size()` for custom sizes in tests).

### Verification

- Tests pass: yes — 689 unit tests pass (13 new realtime/SSE tests added)
- Lint clean: yes — `cargo clippy` clean
- Type check: yes — compiles without errors
