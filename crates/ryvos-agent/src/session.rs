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
    pub cli_session_id: Option<String>,
    pub total_runs: u64,
    pub total_tokens: u64,
    pub billing_type: Option<String>,
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
                cli_session_id: None,
                total_runs: 0,
                total_tokens: 0,
                billing_type: None,
            },
        );
        session_id
    }

    /// Set the CLI session ID for a session.
    pub fn set_cli_session_id(&self, key: &str, cli_session_id: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(info) = sessions.get_mut(key) {
            info.cli_session_id = Some(cli_session_id.to_string());
        }
    }

    /// Get the CLI session ID for a session.
    pub fn get_cli_session_id(&self, key: &str) -> Option<String> {
        let sessions = self.sessions.lock().unwrap();
        sessions.get(key).and_then(|i| i.cli_session_id.clone())
    }

    /// Clear the CLI session ID for a session.
    pub fn clear_cli_session_id(&self, key: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(info) = sessions.get_mut(key) {
            info.cli_session_id = None;
        }
    }

    /// Record run stats for a session.
    pub fn record_run_stats(&self, key: &str, tokens: u64, billing_type: &str) {
        let mut sessions = self.sessions.lock().unwrap();
        if let Some(info) = sessions.get_mut(key) {
            info.total_runs += 1;
            info.total_tokens += tokens;
            info.billing_type = Some(billing_type.to_string());
            info.last_active = chrono::Utc::now();
        }
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
