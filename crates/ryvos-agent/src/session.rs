use ryvos_core::types::SessionId;
use std::collections::HashMap;
use std::sync::Mutex;

/// Simple session manager tracking active sessions.
pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionInfo>>,
}

pub struct SessionInfo {
    pub session_id: SessionId,
    pub channel: String,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub last_active: chrono::DateTime<chrono::Utc>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Get or create a session for a given key (e.g., "telegram:user:12345").
    pub fn get_or_create(&self, key: &str, channel: &str) -> SessionId {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(info) = sessions.get_mut(key) {
            info.last_active = chrono::Utc::now();
            return info.session_id.clone();
        }

        let session_id = SessionId::new();
        sessions.insert(
            key.to_string(),
            SessionInfo {
                session_id: session_id.clone(),
                channel: channel.to_string(),
                started_at: chrono::Utc::now(),
                last_active: chrono::Utc::now(),
            },
        );
        session_id
    }

    /// List active session keys.
    pub fn list(&self) -> Vec<String> {
        self.sessions.lock().unwrap().keys().cloned().collect()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
