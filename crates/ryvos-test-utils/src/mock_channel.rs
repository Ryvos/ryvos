use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use tokio::sync::mpsc;

use ryvos_core::error::Result;
use ryvos_core::traits::ChannelAdapter;
use ryvos_core::types::{MessageContent, MessageEnvelope, SessionId};

/// A mock channel adapter that records all sent messages for assertion.
pub struct MockChannelAdapter {
    adapter_name: String,
    sent: Arc<Mutex<Vec<(String, MessageContent)>>>,
    broadcasts: Arc<Mutex<Vec<MessageContent>>>,
}

impl MockChannelAdapter {
    pub fn new(name: &str) -> Self {
        Self {
            adapter_name: name.to_string(),
            sent: Arc::new(Mutex::new(Vec::new())),
            broadcasts: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// All messages sent via `send()`.
    pub fn sent_messages(&self) -> Vec<(String, MessageContent)> {
        self.sent.lock().unwrap().clone()
    }

    /// All messages sent via `broadcast()`.
    pub fn broadcast_messages(&self) -> Vec<MessageContent> {
        self.broadcasts.lock().unwrap().clone()
    }

    /// Number of messages sent.
    pub fn send_count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

impl ChannelAdapter for MockChannelAdapter {
    fn name(&self) -> &str {
        &self.adapter_name
    }

    fn start(&self, _tx: mpsc::Sender<MessageEnvelope>) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn send(&self, session: &SessionId, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        self.sent
            .lock()
            .unwrap()
            .push((session.0.clone(), content.clone()));
        Box::pin(async { Ok(()) })
    }

    fn broadcast(&self, content: &MessageContent) -> BoxFuture<'_, Result<()>> {
        self.broadcasts.lock().unwrap().push(content.clone());
        Box::pin(async { Ok(()) })
    }

    fn stop(&self) -> BoxFuture<'_, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_channel_send() {
        let ch = MockChannelAdapter::new("test");
        let sid = SessionId::from_string("session1");
        let content = MessageContent::Text("hello".into());
        ch.send(&sid, &content).await.unwrap();
        assert_eq!(ch.send_count(), 1);
        assert_eq!(ch.sent_messages()[0].0, "session1");
    }

    #[tokio::test]
    async fn test_mock_channel_broadcast() {
        let ch = MockChannelAdapter::new("test");
        let content = MessageContent::Text("alert".into());
        ch.broadcast(&content).await.unwrap();
        assert_eq!(ch.broadcast_messages().len(), 1);
    }
}
