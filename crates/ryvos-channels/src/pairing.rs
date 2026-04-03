use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use tokio::sync::Mutex;

/// A pending pairing request from an unknown sender.
#[derive(Debug, Clone)]
pub struct PairingRequest {
    pub code: String,
    pub channel: String,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

/// Manages device pairing with 8-char codes.
pub struct PairingManager {
    pending: Arc<Mutex<HashMap<String, PairingRequest>>>,
    max_pending_per_channel: usize,
}

impl PairingManager {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
            max_pending_per_channel: 3,
        }
    }

    /// Generate a new pairing code for an unknown sender.
    /// Returns None if max pending codes reached for this channel.
    pub async fn create_pairing(
        &self,
        channel: &str,
        sender_id: &str,
        sender_name: Option<&str>,
    ) -> Option<String> {
        let mut pending = self.pending.lock().await;

        // Clean expired
        let now = Utc::now();
        pending.retain(|_, req| req.expires_at > now);

        // Check per-channel limit
        let channel_count = pending.values().filter(|r| r.channel == channel).count();
        if channel_count >= self.max_pending_per_channel {
            return None;
        }

        // Check if sender already has a pending code
        if pending
            .values()
            .any(|r| r.sender_id == sender_id && r.channel == channel)
        {
            return None;
        }

        let code = generate_code();
        let req = PairingRequest {
            code: code.clone(),
            channel: channel.to_string(),
            sender_id: sender_id.to_string(),
            sender_name: sender_name.map(String::from),
            created_at: now,
            expires_at: now + Duration::hours(1),
        };

        pending.insert(code.clone(), req);
        Some(code)
    }

    /// Approve a pairing code. Returns the PairingRequest if valid.
    pub async fn approve(&self, code: &str) -> Option<PairingRequest> {
        let mut pending = self.pending.lock().await;
        let now = Utc::now();

        if let Some(req) = pending.remove(code) {
            if req.expires_at > now {
                return Some(req);
            }
        }
        None
    }

    /// Deny/cancel a pairing code.
    pub async fn deny(&self, code: &str) -> bool {
        self.pending.lock().await.remove(code).is_some()
    }

    /// List all pending pairing requests.
    pub async fn list_pending(&self) -> Vec<PairingRequest> {
        let pending = self.pending.lock().await;
        let now = Utc::now();
        pending
            .values()
            .filter(|r| r.expires_at > now)
            .cloned()
            .collect()
    }

    /// Find a pairing request by code prefix.
    pub async fn find_by_prefix(&self, prefix: &str) -> Option<PairingRequest> {
        let pending = self.pending.lock().await;
        let upper = prefix.to_uppercase();
        pending
            .values()
            .find(|r| r.code.starts_with(&upper))
            .cloned()
    }
}

impl Default for PairingManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate an 8-char uppercase code, excluding ambiguous characters (0/O, 1/I/L).
fn generate_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..8)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_and_approve() {
        let mgr = PairingManager::new();
        let code = mgr.create_pairing("telegram", "12345", Some("Alice")).await;
        assert!(code.is_some());
        let code = code.unwrap();
        assert_eq!(code.len(), 8);

        let req = mgr.approve(&code).await;
        assert!(req.is_some());
        assert_eq!(req.unwrap().sender_id, "12345");

        // Code should be consumed
        assert!(mgr.approve(&code).await.is_none());
    }

    #[tokio::test]
    async fn test_max_per_channel() {
        let mgr = PairingManager::new();
        for i in 0..3 {
            assert!(mgr
                .create_pairing("telegram", &i.to_string(), None)
                .await
                .is_some());
        }
        // 4th should fail
        assert!(mgr.create_pairing("telegram", "999", None).await.is_none());
        // Different channel should work
        assert!(mgr.create_pairing("discord", "999", None).await.is_some());
    }

    #[tokio::test]
    async fn test_no_duplicate_sender() {
        let mgr = PairingManager::new();
        assert!(mgr
            .create_pairing("telegram", "12345", None)
            .await
            .is_some());
        assert!(mgr
            .create_pairing("telegram", "12345", None)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn test_deny_pairing() {
        let mgr = PairingManager::new();
        let code = mgr
            .create_pairing("telegram", "user1", Some("Bob"))
            .await
            .unwrap();

        // Deny returns true for existing code
        assert!(mgr.deny(&code).await);
        // Second deny returns false (already removed)
        assert!(!mgr.deny(&code).await);
        // Approve after deny returns None
        assert!(mgr.approve(&code).await.is_none());
    }

    #[tokio::test]
    async fn test_deny_nonexistent_code() {
        let mgr = PairingManager::new();
        assert!(!mgr.deny("NONEXIST").await);
    }

    #[tokio::test]
    async fn test_list_pending() {
        let mgr = PairingManager::new();
        mgr.create_pairing("telegram", "u1", Some("Alice")).await;
        mgr.create_pairing("discord", "u2", Some("Bob")).await;

        let pending = mgr.list_pending().await;
        assert_eq!(pending.len(), 2);

        let channels: Vec<&str> = pending.iter().map(|r| r.channel.as_str()).collect();
        assert!(channels.contains(&"telegram"));
        assert!(channels.contains(&"discord"));
    }

    #[tokio::test]
    async fn test_list_pending_excludes_approved() {
        let mgr = PairingManager::new();
        let code = mgr.create_pairing("telegram", "u1", None).await.unwrap();
        mgr.create_pairing("discord", "u2", None).await;

        // Approve the first one
        mgr.approve(&code).await;

        let pending = mgr.list_pending().await;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].channel, "discord");
    }

    #[tokio::test]
    async fn test_find_by_prefix() {
        let mgr = PairingManager::new();
        let code = mgr
            .create_pairing("telegram", "u1", Some("Charlie"))
            .await
            .unwrap();

        // Search by first 3 characters
        let prefix = &code[..3];
        let found = mgr.find_by_prefix(prefix).await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().code, code);
    }

    #[tokio::test]
    async fn test_find_by_prefix_case_insensitive() {
        let mgr = PairingManager::new();
        let code = mgr.create_pairing("telegram", "u1", None).await.unwrap();

        // Codes are uppercase, search with lowercase prefix
        let prefix = code[..3].to_lowercase();
        let found = mgr.find_by_prefix(&prefix).await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().code, code);
    }

    #[tokio::test]
    async fn test_find_by_prefix_no_match() {
        let mgr = PairingManager::new();
        mgr.create_pairing("telegram", "u1", None).await;

        let found = mgr.find_by_prefix("ZZZZZZZZ").await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_code_generation_excludes_ambiguous_chars() {
        // Generate many codes and ensure no ambiguous characters appear
        for _ in 0..50 {
            let code = generate_code();
            assert_eq!(code.len(), 8);
            for ch in code.chars() {
                assert!(
                    !"01OIL".contains(ch),
                    "Code '{}' contains ambiguous char '{}'",
                    code,
                    ch
                );
            }
        }
    }
}
