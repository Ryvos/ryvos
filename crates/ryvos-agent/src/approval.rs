use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{oneshot, Mutex};

use ryvos_core::event::EventBus;
use ryvos_core::security::{ApprovalDecision, ApprovalRequest};
use ryvos_core::types::AgentEvent;

/// Manages pending approval requests with oneshot channels.
pub struct ApprovalBroker {
    pending: Mutex<HashMap<String, (ApprovalRequest, oneshot::Sender<ApprovalDecision>)>>,
    event_bus: Arc<EventBus>,
}

impl ApprovalBroker {
    pub fn new(event_bus: Arc<EventBus>) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            event_bus,
        }
    }

    /// Create an approval request, publish event, return receiver to await.
    pub async fn request(&self, req: ApprovalRequest) -> oneshot::Receiver<ApprovalDecision> {
        let (tx, rx) = oneshot::channel();
        let id = req.id.clone();

        self.event_bus.publish(AgentEvent::ApprovalRequested {
            request: req.clone(),
        });

        self.pending.lock().await.insert(id, (req, tx));
        rx
    }

    /// Respond to a pending approval (called by REPL/WebSocket/Telegram).
    /// Returns true if the request was found and resolved.
    pub async fn respond(&self, request_id: &str, decision: ApprovalDecision) -> bool {
        let entry = self.pending.lock().await.remove(request_id);
        if let Some((_req, tx)) = entry {
            let approved = matches!(decision, ApprovalDecision::Approved);
            self.event_bus.publish(AgentEvent::ApprovalResolved {
                request_id: request_id.to_string(),
                approved,
            });
            // Ignore send error (receiver may have been dropped due to timeout)
            let _ = tx.send(decision);
            true
        } else {
            false
        }
    }

    /// List all pending approvals.
    pub async fn pending_requests(&self) -> Vec<ApprovalRequest> {
        self.pending
            .lock()
            .await
            .values()
            .map(|(req, _)| req.clone())
            .collect()
    }

    /// Find a pending request by prefix match on the ID.
    pub async fn find_by_prefix(&self, prefix: &str) -> Option<String> {
        let pending = self.pending.lock().await;
        for key in pending.keys() {
            if key.starts_with(prefix) {
                return Some(key.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use ryvos_core::security::SecurityTier;

    fn test_request(id: &str) -> ApprovalRequest {
        ApprovalRequest {
            id: id.to_string(),
            tool_name: "bash".to_string(),
            tier: SecurityTier::T2,
            input_summary: "ls -la".to_string(),
            session_id: "test-session".to_string(),
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn respond_approve() {
        let event_bus = Arc::new(EventBus::default());
        let broker = ApprovalBroker::new(event_bus);

        let rx = broker.request(test_request("req-1")).await;
        assert!(broker.respond("req-1", ApprovalDecision::Approved).await);

        let decision = rx.await.unwrap();
        assert!(matches!(decision, ApprovalDecision::Approved));
    }

    #[tokio::test]
    async fn respond_deny() {
        let event_bus = Arc::new(EventBus::default());
        let broker = ApprovalBroker::new(event_bus);

        let rx = broker.request(test_request("req-2")).await;
        assert!(
            broker
                .respond(
                    "req-2",
                    ApprovalDecision::Denied {
                        reason: "too dangerous".into()
                    }
                )
                .await
        );

        let decision = rx.await.unwrap();
        match decision {
            ApprovalDecision::Denied { reason } => assert_eq!(reason, "too dangerous"),
            _ => panic!("expected Denied"),
        }
    }

    #[tokio::test]
    async fn respond_unknown_id() {
        let event_bus = Arc::new(EventBus::default());
        let broker = ApprovalBroker::new(event_bus);
        assert!(
            !broker
                .respond("nonexistent", ApprovalDecision::Approved)
                .await
        );
    }

    #[tokio::test]
    async fn timeout() {
        let event_bus = Arc::new(EventBus::default());
        let broker = ApprovalBroker::new(event_bus);

        let rx = broker.request(test_request("req-3")).await;

        // Simulate timeout by dropping the broker's sender side
        // In practice, the gate uses tokio::time::timeout on the receiver
        let result = tokio::time::timeout(std::time::Duration::from_millis(10), rx).await;
        assert!(result.is_err()); // timeout
    }

    #[tokio::test]
    async fn pending_requests_listed() {
        let event_bus = Arc::new(EventBus::default());
        let broker = ApprovalBroker::new(event_bus);

        let _rx1 = broker.request(test_request("req-a")).await;
        let _rx2 = broker.request(test_request("req-b")).await;

        let pending = broker.pending_requests().await;
        assert_eq!(pending.len(), 2);
    }
}
