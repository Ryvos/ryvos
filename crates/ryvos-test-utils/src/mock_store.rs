use std::collections::HashMap;
use std::sync::Mutex;

use futures::future::BoxFuture;

use ryvos_core::error::Result;
use ryvos_core::traits::SessionStore;
use ryvos_core::types::{ChatMessage, SearchResult, SessionId};

/// An in-memory session store for testing. Stores messages in a HashMap.
pub struct InMemorySessionStore {
    data: Mutex<HashMap<String, Vec<ChatMessage>>>,
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(HashMap::new()),
        }
    }

    /// Number of sessions stored.
    pub fn session_count(&self) -> usize {
        self.data.lock().unwrap().len()
    }

    /// Number of messages in a session.
    pub fn message_count(&self, session_id: &str) -> usize {
        self.data
            .lock()
            .unwrap()
            .get(session_id)
            .map_or(0, |v| v.len())
    }
}

impl Default for InMemorySessionStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionStore for InMemorySessionStore {
    fn append_messages(&self, sid: &SessionId, msgs: &[ChatMessage]) -> BoxFuture<'_, Result<()>> {
        let sid = sid.0.clone();
        let msgs = msgs.to_vec();
        let mut data = self.data.lock().unwrap();
        data.entry(sid).or_default().extend(msgs);
        Box::pin(async { Ok(()) })
    }

    fn load_history(
        &self,
        sid: &SessionId,
        limit: usize,
    ) -> BoxFuture<'_, Result<Vec<ChatMessage>>> {
        let data = self.data.lock().unwrap();
        let msgs = data
            .get(&sid.0)
            .map(|v| {
                let start = v.len().saturating_sub(limit);
                v[start..].to_vec()
            })
            .unwrap_or_default();
        Box::pin(async move { Ok(msgs) })
    }

    fn search(&self, query: &str, limit: usize) -> BoxFuture<'_, Result<Vec<SearchResult>>> {
        let query = query.to_lowercase();
        let data = self.data.lock().unwrap();
        let mut results = Vec::new();
        for (sid, msgs) in data.iter() {
            for msg in msgs {
                let text = msg.text();
                if text.to_lowercase().contains(&query) {
                    results.push(SearchResult {
                        session_id: sid.clone(),
                        role: format!("{:?}", msg.role),
                        content: text,
                        rank: 1.0,
                        timestamp: msg.timestamp.unwrap_or_else(chrono::Utc::now),
                    });
                    if results.len() >= limit {
                        break;
                    }
                }
            }
        }
        Box::pin(async move { Ok(results) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_in_memory_store_append_and_load() {
        let store = InMemorySessionStore::new();
        let sid = SessionId::from_string("test-session");
        store
            .append_messages(&sid, &[ChatMessage::user("hello")])
            .await
            .unwrap();
        let history = store.load_history(&sid, 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].text(), "hello");
    }

    #[tokio::test]
    async fn test_in_memory_store_search() {
        let store = InMemorySessionStore::new();
        let sid = SessionId::from_string("s1");
        store
            .append_messages(
                &sid,
                &[
                    ChatMessage::user("deploy to production"),
                    ChatMessage::user("run tests"),
                ],
            )
            .await
            .unwrap();
        let results = store.search("deploy", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("deploy"));
    }

    #[tokio::test]
    async fn test_in_memory_store_load_limit() {
        let store = InMemorySessionStore::new();
        let sid = SessionId::from_string("s1");
        for i in 0..10 {
            store
                .append_messages(&sid, &[ChatMessage::user(format!("msg {}", i))])
                .await
                .unwrap();
        }
        let history = store.load_history(&sid, 3).await.unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].text(), "msg 7");
    }
}
