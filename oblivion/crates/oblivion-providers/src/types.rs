//! Chat message and tool types in the OpenAI chat-completions wire format.
//! Almost every free provider (OpenRouter, Groq, NVIDIA, Cerebras, Mistral,
//! Cloudflare, Ollama, Gemini's OpenAI endpoint) accepts this format.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// An attached image for multi-modal vision requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageAttachment {
    /// MIME type, e.g. "image/png", "image/jpeg", "image/webp".
    pub mime_type: String,
    /// Base64 encoded image data (without data: URL scheme prefix).
    pub data_base64: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

impl ImageAttachment {
    pub fn new(mime_type: impl Into<String>, data_base64: impl Into<String>) -> Self {
        let mut data = data_base64.into();
        if let Some(pos) = data.find(";base64,") {
            data = data[pos + 8..].to_string();
        }
        Self {
            mime_type: mime_type.into(),
            data_base64: data,
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Formats as a standard data URI string: `data:<mime>;base64,<data>`.
    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime_type, self.data_base64)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    /// Text content. `None` is sent as JSON null (assistant turns that only call tools).
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageAttachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: Some(text.into()),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Some(text.into()),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn user_with_images(
        text: impl Into<String>,
        images: impl Into<Vec<ImageAttachment>>,
    ) -> Self {
        Self {
            role: Role::User,
            content: Some(text.into()),
            images: images.into(),
            tool_calls: vec![],
            tool_call_id: None,
        }
    }
    pub fn assistant(text: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: text,
            images: vec![],
            tool_calls,
            tool_call_id: None,
        }
    }
    pub fn tool_result(call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: Some(text.into()),
            images: vec![],
            tool_calls: vec![],
            tool_call_id: Some(call_id.into()),
        }
    }

    /// Converts message to wire JSON for OpenAI / Groq / OpenRouter and Ollama APIs.
    /// When images are present, constructs the multimodal content array and images array.
    pub fn to_wire(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("role".into(), serde_json::json!(self.role));

        if self.images.is_empty() {
            match &self.content {
                Some(text) => map.insert("content".into(), serde_json::Value::String(text.clone())),
                None => map.insert("content".into(), serde_json::Value::Null),
            };
        } else {
            // Multimodal content array for OpenAI / Groq / OpenRouter / Anthropic proxies
            let mut parts = Vec::new();
            if let Some(text) = &self.content {
                if !text.is_empty() {
                    parts.push(serde_json::json!({
                        "type": "text",
                        "text": text,
                    }));
                }
            }
            for img in &self.images {
                parts.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": {
                        "url": img.data_url()
                    }
                }));
            }
            map.insert("content".into(), serde_json::Value::Array(parts));

            // Also provide "images" array for Ollama compatibility
            let b64_list: Vec<serde_json::Value> = self
                .images
                .iter()
                .map(|img| serde_json::Value::String(img.data_base64.clone()))
                .collect();
            map.insert("images".into(), serde_json::Value::Array(b64_list));
        }

        if !self.tool_calls.is_empty() {
            map.insert("tool_calls".into(), serde_json::json!(self.tool_calls));
        }
        if let Some(id) = &self.tool_call_id {
            map.insert("tool_call_id".into(), serde_json::Value::String(id.clone()));
        }

        serde_json::Value::Object(map)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "function_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn function_type() -> String {
    "function".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON-encoded arguments exactly as the model produced them (may be invalid JSON).
    pub arguments: String,
}

/// A tool the model may call, described by a JSON Schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    pub fn to_wire(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            }
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
}

/// One finished model turn.
#[derive(Debug, Clone)]
pub struct Completion {
    pub message: Message,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
    /// "provider/model" that actually answered.
    pub served_by: String,
}

/// Rough token count for a request (about 4 characters per token, plus
/// per-message overhead). Good enough to stay under free-tier limits.
pub fn estimate_tokens(messages: &[Message], tools: &[ToolSpec]) -> usize {
    let mut chars = 0usize;
    for m in messages {
        chars += 16;
        chars += m.content.as_deref().map_or(0, str::len);
        for c in &m.tool_calls {
            chars += c.function.name.len() + c.function.arguments.len() + 16;
        }
    }
    for t in tools {
        chars += t.name.len() + t.description.len() + t.parameters.to_string().len();
    }
    chars / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_to_wire_text_only() {
        let msg = Message::user("Hello world");
        let wire = msg.to_wire();
        assert_eq!(wire["role"], "user");
        assert_eq!(wire["content"], "Hello world");
        assert!(wire.get("images").is_none());
    }

    #[test]
    fn test_message_to_wire_with_images() {
        let img = ImageAttachment::new("image/png", "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=");
        let msg = Message::user_with_images("What is in this image?", vec![img]);
        let wire = msg.to_wire();

        assert_eq!(wire["role"], "user");
        let content_parts = wire["content"].as_array().expect("expected content array");
        assert_eq!(content_parts.len(), 2);
        assert_eq!(content_parts[0]["type"], "text");
        assert_eq!(content_parts[0]["text"], "What is in this image?");
        assert_eq!(content_parts[1]["type"], "image_url");
        assert!(content_parts[1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,iVBOR"));

        let ollama_images = wire["images"].as_array().expect("expected images array");
        assert_eq!(ollama_images.len(), 1);
        assert!(ollama_images[0].as_str().unwrap().starts_with("iVBOR"));
    }

    #[test]
    fn test_image_attachment_strips_data_url_prefix() {
        let raw_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk";
        let with_prefix = format!("data:image/jpeg;base64,{raw_b64}");
        let img = ImageAttachment::new("image/jpeg", with_prefix);
        assert_eq!(img.data_base64, raw_b64);
        assert_eq!(img.mime_type, "image/jpeg");
        assert_eq!(img.data_url(), format!("data:image/jpeg;base64,{raw_b64}"));
    }

    #[test]
    fn test_message_roundtrip_serde() {
        let img =
            ImageAttachment::new("image/webp", "dGVzdC1pbWFnZQ==").with_name("screenshot.webp");
        let msg = Message::user_with_images("Inspect screenshot", vec![img]);
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, Role::User);
        assert_eq!(parsed.content, Some("Inspect screenshot".into()));
        assert_eq!(parsed.images.len(), 1);
        assert_eq!(parsed.images[0].name.as_deref(), Some("screenshot.webp"));
    }
}
