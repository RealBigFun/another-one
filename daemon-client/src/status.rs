//! Process-wide status queue used by the iroh dial flow to surface
//! progress to the UI without holding the live `Session`. The GPUI
//! render thread isn't async and shouldn't block on a receiver, so we
//! use a `Mutex<Vec<_>>` rather than a channel.

use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone)]
pub enum DialStatus {
    Started { endpoint_id: String },
    Bound,
    Connected,
    HelloSent,
    Error(String),
}

static STATUS_QUEUE: OnceLock<Mutex<Vec<DialStatus>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<DialStatus>> {
    STATUS_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Push a status event. Called by `Session::connect` as it progresses.
pub fn push_status(s: DialStatus) {
    if let Ok(mut q) = queue().lock() {
        q.push(s);
    }
}

/// Take all pending status events. Called by the UI render tick.
pub fn drain_status() -> Vec<DialStatus> {
    queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}
