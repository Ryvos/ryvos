use serde::{Deserialize, Serialize};

/// A frame sent from the client.
#[derive(Debug, Deserialize)]
pub struct ClientFrame {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    pub frame_type: String,
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A response frame sent to the client.
#[derive(Debug, Serialize)]
pub struct ServerResponse {
    #[serde(rename = "type")]
    pub frame_type: String,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorPayload>,
}

/// An event frame pushed to the client.
#[derive(Debug, Serialize)]
pub struct ServerEvent {
    #[serde(rename = "type")]
    pub frame_type: String,
    pub session_id: String,
    pub event: EventPayload,
}

#[derive(Debug, Serialize)]
pub struct ErrorPayload {
    pub code: i32,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventPayload {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ServerResponse {
    pub fn ok(id: String, result: serde_json::Value) -> Self {
        Self {
            frame_type: "response".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: String, code: i32, message: String) -> Self {
        Self {
            frame_type: "response".to_string(),
            id,
            result: None,
            error: Some(ErrorPayload { code, message }),
        }
    }
}

impl ServerEvent {
    pub fn new(session_id: String, kind: &str) -> Self {
        Self {
            frame_type: "event".to_string(),
            session_id,
            event: EventPayload {
                kind: kind.to_string(),
                text: None,
                tool: None,
                data: None,
            },
        }
    }

    pub fn with_text(mut self, text: String) -> Self {
        self.event.text = Some(text);
        self
    }

    pub fn with_tool(mut self, tool: String) -> Self {
        self.event.tool = Some(tool);
        self
    }

    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        self.event.data = Some(data);
        self
    }
}
