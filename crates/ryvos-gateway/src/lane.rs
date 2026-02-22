use tokio::sync::{mpsc, oneshot};

/// A request queued in a session lane.
pub struct LaneItem {
    pub method: String,
    pub params: serde_json::Value,
    pub respond: oneshot::Sender<serde_json::Value>,
}

/// Per-session FIFO queue ensuring serial execution of requests.
pub struct LaneQueue {
    tx: mpsc::Sender<LaneItem>,
}

impl LaneQueue {
    /// Create a new lane and return (queue_handle, receiver).
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<LaneItem>) {
        let (tx, rx) = mpsc::channel(buffer);
        (Self { tx }, rx)
    }

    /// Enqueue a request and wait for the result.
    pub async fn send(&self, method: String, params: serde_json::Value) -> Option<serde_json::Value> {
        let (respond, rx) = oneshot::channel();
        let item = LaneItem {
            method,
            params,
            respond,
        };
        self.tx.send(item).await.ok()?;
        rx.await.ok()
    }
}
