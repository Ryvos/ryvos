use crate::types::AgentEvent;

/// Event bus using tokio broadcast channel.
/// All subscribers receive all events.
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<AgentEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, event: AgentEvent) {
        // Ignore error if no receivers
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<AgentEvent> {
        self.tx.subscribe()
    }

    /// Subscribe with a filter — only matching events are delivered.
    ///
    /// Returns a `FilteredReceiver` that wraps the broadcast receiver and
    /// skips non-matching events. This is backwards compatible; callers who
    /// don't need filtering continue to use `subscribe()`.
    pub fn subscribe_filtered(&self, filter: EventFilter) -> FilteredReceiver {
        FilteredReceiver {
            rx: self.tx.subscribe(),
            filter,
        }
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(256)
    }
}

/// Filter criteria for scoped event subscriptions.
///
/// All fields are optional; an event must match **all** specified criteria.
/// Unset fields are treated as "match anything".
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    /// Only events with this session_id (extracted from the event variant).
    pub session_id: Option<String>,
    /// Only events of these types (matched by discriminant name).
    pub event_types: Option<Vec<String>>,
    /// Only events tagged with this node_id (for graph execution).
    pub node_id: Option<String>,
}

impl EventFilter {
    /// Create a filter that matches a specific session.
    pub fn for_session(session_id: impl Into<String>) -> Self {
        Self {
            session_id: Some(session_id.into()),
            ..Default::default()
        }
    }

    /// Create a filter that matches specific event types.
    pub fn for_types(types: Vec<String>) -> Self {
        Self {
            event_types: Some(types),
            ..Default::default()
        }
    }

    /// Add a session_id constraint.
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    /// Add a node_id constraint.
    pub fn with_node(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = Some(node_id.into());
        self
    }

    /// Check whether an event matches this filter.
    pub fn matches(&self, event: &AgentEvent) -> bool {
        // Check session_id
        if let Some(ref sid) = self.session_id {
            if let Some(event_sid) = extract_session_id(event) {
                if event_sid != sid {
                    return false;
                }
            }
            // Events without a session_id field pass the session filter
            // (e.g., TextDelta, UsageUpdate) — they belong to the "current" session.
        }

        // Check event type
        if let Some(ref types) = self.event_types {
            let event_type = event_type_name(event);
            if !types.iter().any(|t| t == event_type) {
                return false;
            }
        }

        // node_id filtering is a no-op for now — events don't carry node_id.
        // When graph execution tags events, this will filter on it.

        true
    }
}

/// Extract the session_id from events that carry one.
fn extract_session_id(event: &AgentEvent) -> Option<&str> {
    match event {
        AgentEvent::RunStarted { session_id } => Some(&session_id.0),
        AgentEvent::RunComplete { session_id, .. } => Some(&session_id.0),
        AgentEvent::GoalEvaluated { session_id, .. } => Some(&session_id.0),
        AgentEvent::JudgeVerdict { session_id, .. } => Some(&session_id.0),
        AgentEvent::GuardianStall { session_id, .. } => Some(&session_id.0),
        AgentEvent::GuardianDoomLoop { session_id, .. } => Some(&session_id.0),
        AgentEvent::GuardianBudgetAlert { session_id, .. } => Some(&session_id.0),
        AgentEvent::GuardianHint { session_id, .. } => Some(&session_id.0),
        AgentEvent::ApprovalRequested { request } => Some(&request.session_id),
        AgentEvent::HeartbeatOk { session_id, .. } => Some(&session_id.0),
        AgentEvent::HeartbeatAlert { session_id, .. } => Some(&session_id.0),
        _ => None,
    }
}

/// Get the discriminant name of an AgentEvent as a string.
fn event_type_name(event: &AgentEvent) -> &'static str {
    match event {
        AgentEvent::RunStarted { .. } => "RunStarted",
        AgentEvent::TextDelta(_) => "TextDelta",
        AgentEvent::ToolStart { .. } => "ToolStart",
        AgentEvent::ToolEnd { .. } => "ToolEnd",
        AgentEvent::TurnComplete { .. } => "TurnComplete",
        AgentEvent::RunComplete { .. } => "RunComplete",
        AgentEvent::RunError { .. } => "RunError",
        AgentEvent::CronFired { .. } => "CronFired",
        AgentEvent::ApprovalRequested { .. } => "ApprovalRequested",
        AgentEvent::ApprovalResolved { .. } => "ApprovalResolved",
        AgentEvent::ToolBlocked { .. } => "ToolBlocked",
        AgentEvent::GuardianStall { .. } => "GuardianStall",
        AgentEvent::GuardianDoomLoop { .. } => "GuardianDoomLoop",
        AgentEvent::GuardianBudgetAlert { .. } => "GuardianBudgetAlert",
        AgentEvent::GuardianHint { .. } => "GuardianHint",
        AgentEvent::UsageUpdate { .. } => "UsageUpdate",
        AgentEvent::GoalEvaluated { .. } => "GoalEvaluated",
        AgentEvent::DecisionMade { .. } => "DecisionMade",
        AgentEvent::JudgeVerdict { .. } => "JudgeVerdict",
        AgentEvent::HeartbeatFired { .. } => "HeartbeatFired",
        AgentEvent::HeartbeatOk { .. } => "HeartbeatOk",
        AgentEvent::HeartbeatAlert { .. } => "HeartbeatAlert",
    }
}

/// A filtered event receiver that skips non-matching events.
pub struct FilteredReceiver {
    rx: tokio::sync::broadcast::Receiver<AgentEvent>,
    filter: EventFilter,
}

impl FilteredReceiver {
    /// Receive the next matching event, blocking until one arrives.
    pub async fn recv(&mut self) -> Result<AgentEvent, tokio::sync::broadcast::error::RecvError> {
        loop {
            let event = self.rx.recv().await?;
            if self.filter.matches(&event) {
                return Ok(event);
            }
        }
    }

    /// Non-blocking try_recv — returns the next matching event or an error.
    pub fn try_recv(
        &mut self,
    ) -> Result<AgentEvent, tokio::sync::broadcast::error::TryRecvError> {
        loop {
            let event = self.rx.try_recv()?;
            if self.filter.matches(&event) {
                return Ok(event);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionId;

    #[test]
    fn test_unfiltered_subscribe() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe();
        bus.publish(AgentEvent::RunStarted {
            session_id: SessionId::from_string("s1"),
        });
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, AgentEvent::RunStarted { .. }));
    }

    #[test]
    fn test_filter_by_session() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe_filtered(EventFilter::for_session("s1"));

        bus.publish(AgentEvent::RunStarted {
            session_id: SessionId::from_string("s2"),
        });
        bus.publish(AgentEvent::RunStarted {
            session_id: SessionId::from_string("s1"),
        });

        let event = rx.try_recv().unwrap();
        match event {
            AgentEvent::RunStarted { session_id } => assert_eq!(session_id.0, "s1"),
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn test_filter_by_event_type() {
        let bus = EventBus::new(16);
        let mut rx = bus.subscribe_filtered(EventFilter::for_types(vec![
            "TurnComplete".to_string(),
        ]));

        bus.publish(AgentEvent::RunStarted {
            session_id: SessionId::from_string("s1"),
        });
        bus.publish(AgentEvent::TurnComplete { turn: 0 });
        bus.publish(AgentEvent::TextDelta("hello".to_string()));
        bus.publish(AgentEvent::TurnComplete { turn: 1 });

        let e1 = rx.try_recv().unwrap();
        assert!(matches!(e1, AgentEvent::TurnComplete { turn: 0 }));

        let e2 = rx.try_recv().unwrap();
        assert!(matches!(e2, AgentEvent::TurnComplete { turn: 1 }));
    }

    #[test]
    fn test_filter_combined() {
        let bus = EventBus::new(16);
        let filter = EventFilter::for_session("s1")
            .with_node("n1"); // node_id is a no-op for now

        let mut rx = bus.subscribe_filtered(filter);

        bus.publish(AgentEvent::RunStarted {
            session_id: SessionId::from_string("s2"),
        });
        bus.publish(AgentEvent::RunStarted {
            session_id: SessionId::from_string("s1"),
        });

        let event = rx.try_recv().unwrap();
        match event {
            AgentEvent::RunStarted { session_id } => assert_eq!(session_id.0, "s1"),
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn test_event_type_names() {
        assert_eq!(
            event_type_name(&AgentEvent::RunStarted {
                session_id: SessionId::from_string("s1")
            }),
            "RunStarted"
        );
        assert_eq!(
            event_type_name(&AgentEvent::TextDelta("hi".to_string())),
            "TextDelta"
        );
        assert_eq!(
            event_type_name(&AgentEvent::TurnComplete { turn: 0 }),
            "TurnComplete"
        );
    }

    #[test]
    fn test_sessionless_events_pass_session_filter() {
        let filter = EventFilter::for_session("s1");
        // TextDelta has no session_id — it passes the filter
        assert!(filter.matches(&AgentEvent::TextDelta("hi".to_string())));
        // But events with a different session_id are blocked
        assert!(!filter.matches(&AgentEvent::RunStarted {
            session_id: SessionId::from_string("s2"),
        }));
    }
}
