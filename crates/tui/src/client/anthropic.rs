//! Anthropic Messages API client (`/anthropic/v1/messages`).
//!
//! DeepSeek's `/anthropic` endpoint speaks the Anthropic Messages protocol:
//! structured content blocks with protocol-level role separation. This makes
//! the endpoint immune to Special Token Injection — user text never mixes
//! with control tokens because role boundaries are defined in JSON structure,
//! not in token-space separator strings.
//!
//! The internal `StreamEvent` / `ContentBlockStart` / `Delta` types already
//! use Anthropic-native naming, so the SSE parser here is a direct wire→model
//! translation with no format remapping (unlike the Chat Completions path).

use std::collections::HashMap;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{Value, json};

use crate::llm_client::StreamEventBox;
use crate::models::{
    ContentBlock, ContentBlockStart, Delta, Message, MessageDelta, MessageRequest,
    MessageResponse, StreamEvent, Tool, Usage,
};
#[cfg(test)]
use crate::models::SystemPrompt;

use super::{
    DeepSeekClient, ERROR_BODY_MAX_BYTES, SSE_BACKPRESSURE_HIGH_WATERMARK,
    SSE_BACKPRESSURE_SLEEP_MS, SSE_MAX_LINES_PER_CHUNK, acquire_stream_buffer,
    bounded_error_text, parse_usage, release_stream_buffer, system_to_instructions,
};

/// Default Anthropic API version header value.
const ANTHROPIC_VERSION: &str = "2023-06-01";

// ---------------------------------------------------------------------------
// Public entry points called from DeepSeekClient impl blocks
// ---------------------------------------------------------------------------

impl DeepSeekClient {
    /// Non-streaming Anthropic Messages API request.
    pub(super) async fn create_message_anthropic(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse> {
        let body = build_anthropic_request(request, false);
        let url = anthropic_api_url(&self.base_url);

        let response = self
            .send_with_retry(|| {
                self.http_client
                    .post(&url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .json(&body)
            })
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!(
                "Anthropic Messages API error: HTTP {status}: {error_text}"
            );
        }

        let response_text = response.text().await.unwrap_or_default();
        let value: Value = serde_json::from_str(&response_text)
            .context("Failed to parse Anthropic Messages API JSON")?;
        parse_anthropic_message(&value)
    }

    /// Streaming Anthropic Messages API request.
    pub(super) async fn handle_anthropic_stream(
        &self,
        request: MessageRequest,
    ) -> Result<StreamEventBox> {
        let model = request.model.clone();
        let mut body = build_anthropic_request(&request, true);

        // Apply reasoning effort via Anthropic-native `thinking` field.
        if let Some(effort) = request.reasoning_effort.as_deref() {
            let normalized = effort.trim().to_ascii_lowercase();
            match normalized.as_str() {
                "off" | "disabled" | "none" | "false" => {
                    body["thinking"] = json!({"type": "disabled"});
                }
                "low" | "minimal" | "medium" | "mid" | "high" | "" => {
                    body["thinking"] = json!({"type": "enabled", "budget_tokens": 4096});
                }
                "xhigh" | "max" | "highest" => {
                    body["thinking"] = json!({"type": "enabled", "budget_tokens": 16384});
                }
                _ => {}
            }
        }

        let url = anthropic_api_url(&self.base_url);
        let response = self
            .send_with_retry(|| {
                self.http_client
                    .post(&url)
                    .header("x-api-key", &self.api_key)
                    .header("anthropic-version", ANTHROPIC_VERSION)
                    .json(&body)
            })
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = bounded_error_text(response, ERROR_BODY_MAX_BYTES).await;
            anyhow::bail!(
                "Anthropic Messages API stream error: HTTP {status}: {error_text}"
            );
        }

        let byte_stream = response.bytes_stream();
        let stream = async_stream::stream! {
            use futures_util::StreamExt;

            // Emit synthetic MessageStart
            yield Ok(StreamEvent::MessageStart {
                message: MessageResponse {
                    id: String::new(),
                    r#type: "message".to_string(),
                    role: "assistant".to_string(),
                    content: Vec::new(),
                    model: model.clone(),
                    stop_reason: None,
                    stop_sequence: None,
                    container: None,
                    usage: Usage::default(),
                },
            });

            let mut line_buf = String::new();
            let mut current_event: Option<String> = None;
            let mut byte_buf = acquire_stream_buffer();

            let mut byte_stream = std::pin::pin!(byte_stream);
            let idle = Duration::from_secs(300);

            loop {
                let chunk_result = match tokio::time::timeout(idle, byte_stream.next()).await {
                    Ok(Some(result)) => result,
                    Ok(None) => break,
                    Err(_elapsed) => {
                        yield Err(anyhow::anyhow!(
                            "Anthropic SSE stream idle timeout after {}s",
                            idle.as_secs(),
                        ));
                        break;
                    }
                };
                let chunk = match chunk_result {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        yield Err(anyhow::anyhow!("Anthropic stream read error: {e}"));
                        break;
                    }
                };

                byte_buf.extend_from_slice(&chunk);

                const MAX_BUF: usize = 10 * 1024 * 1024;
                if byte_buf.len() > MAX_BUF {
                    yield Err(anyhow::anyhow!("Anthropic SSE buffer exceeded {MAX_BUF} bytes"));
                    break;
                }
                if byte_buf.len() > SSE_BACKPRESSURE_HIGH_WATERMARK {
                    tokio::time::sleep(Duration::from_millis(SSE_BACKPRESSURE_SLEEP_MS)).await;
                }

                let mut lines_processed = 0usize;
                while let Some(newline_pos) = byte_buf.iter().position(|&b| b == b'\n') {
                    let mut end = newline_pos;
                    if end > 0 && byte_buf[end - 1] == b'\r' {
                        end -= 1;
                    }
                    let line = String::from_utf8_lossy(&byte_buf[..end]).into_owned();
                    byte_buf.drain(..newline_pos + 1);

                    if line.is_empty() {
                        // Empty line = event boundary
                        if let Some(ref event_type) = current_event.take() {
                            if !line_buf.is_empty() {
                                let data = std::mem::take(&mut line_buf);
                                match event_type.as_str() {
                                    "ping" => {
                                        // Keep-alive, ignore
                                    }
                                    "message_stop" => {
                                        yield Ok(StreamEvent::MessageStop);
                                    }
                                    _ => {
                                        if let Ok(event_json) = serde_json::from_str::<Value>(&data) {
                                            if let Some(event) = parse_anthropic_sse_event(
                                                event_type, &event_json,
                                            ) {
                                                yield Ok(event);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    if let Some(event_type) = line.strip_prefix("event: ") {
                        current_event = Some(event_type.to_string());
                    } else if let Some(data) = line.strip_prefix("data: ") {
                        line_buf.push_str(data);
                    }

                    lines_processed = lines_processed.saturating_add(1);
                    if lines_processed >= SSE_MAX_LINES_PER_CHUNK {
                        break;
                    }
                }
            }

            release_stream_buffer(byte_buf);
            yield Ok(StreamEvent::MessageStop);
        };

        Ok(Pin::from(Box::new(stream)
            as Box<
                dyn futures_util::Stream<Item = Result<StreamEvent>> + Send,
            >))
    }
}

// ---------------------------------------------------------------------------
// Request building
// ---------------------------------------------------------------------------

/// Build an Anthropic-format JSON request body.
fn build_anthropic_request(request: &MessageRequest, stream: bool) -> Value {
    let mut body = json!({
        "model": request.model,
        "max_tokens": request.max_tokens,
        "stream": stream,
    });

    // System prompt goes into top-level `system` field (string or array).
    if let Some(instructions) = system_to_instructions(request.system.clone())
        && !instructions.trim().is_empty()
    {
        body["system"] = json!(instructions);
    }

    // Build messages in Anthropic format.
    let messages = build_anthropic_messages(&request.messages);
    body["messages"] = json!(messages);

    // Tools in Anthropic format: no `type: "function"` wrapper.
    if let Some(tools) = request.tools.as_ref() {
        body["tools"] = json!(
            tools
                .iter()
                .map(tool_to_anthropic)
                .collect::<Vec<_>>()
        );
    }

    if let Some(choice) = request.tool_choice.as_ref() {
        if let Some(mapped) = map_tool_choice_anthropic(choice) {
            body["tool_choice"] = mapped;
        }
    }

    if let Some(temperature) = request.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(top_p) = request.top_p {
        body["top_p"] = json!(top_p);
    }

    body
}

/// Convert internal `Tool` to Anthropic format (no `type: "function"` wrapper).
fn tool_to_anthropic(tool: &Tool) -> Value {
    let mut value = json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.input_schema,
    });
    if let Some(strict) = tool.strict
        && let Some(obj) = value.as_object_mut()
    {
        obj.insert("strict".to_string(), json!(strict));
    }
    value
}

/// Map tool_choice to Anthropic format.
fn map_tool_choice_anthropic(choice: &Value) -> Option<Value> {
    if let Some(choice_str) = choice.as_str() {
        match choice_str {
            "auto" | "any" | "none" => return Some(json!({"type": choice_str})),
            _ => return Some(json!({"type": "auto"})),
        }
    }
    let choice_type = choice.get("type").and_then(Value::as_str)?;
    match choice_type {
        "tool" => {
            let name = choice.get("name").and_then(Value::as_str)?;
            Some(json!({"type": "tool", "name": name}))
        }
        "auto" | "any" | "none" => Some(json!({"type": choice_type})),
        _ => Some(json!({"type": "auto"})),
    }
}

/// Build the messages array in Anthropic content-block format.
///
/// Key immunity property: user text is always wrapped in
/// `{"type": "text", "text": "<user input>"}` inside a `content` array.
/// The `<think>` literal never appears as a raw tokenizable string,
/// so the tokenizer cannot map it to a control token.
fn build_anthropic_messages(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut pending_tool_calls: HashMap<String, (String, Value)> = HashMap::new();

    for message in messages {
        let role = message.role.as_str();
        let mut content_blocks = Vec::new();
        let mut tool_call_infos = Vec::new();
        let mut tool_results: Vec<(String, String)> = Vec::new();

        for block in &message.content {
            match block {
                ContentBlock::Text { text, .. } => {
                    content_blocks.push(json!({
                        "type": "text",
                        "text": text,
                    }));
                }
                ContentBlock::Thinking { thinking } => {
                    // Anthropic format: thinking is a separate content block type
                    content_blocks.push(json!({
                        "type": "thinking",
                        "thinking": thinking,
                    }));
                }
                ContentBlock::ToolUse {
                    id,
                    name,
                    input,
                    ..
                } => {
                    content_blocks.push(json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                    tool_call_infos.push((
                        id.clone(),
                        (name.clone(), input.clone()),
                    ));
                }
                ContentBlock::ToolResult {
                    tool_use_id,
                    content,
                    is_error,
                    ..
                } => {
                    let mut block = json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": content,
                    });
                    if let Some(true) = is_error {
                        block["is_error"] = json!(true);
                    }
                    content_blocks.push(block);
                    tool_results.push((tool_use_id.clone(), content.clone()));
                }
                ContentBlock::ServerToolUse { .. }
                | ContentBlock::ToolSearchToolResult { .. }
                | ContentBlock::CodeExecutionToolResult { .. } => {
                    // Not directly expressible in Anthropic format; skip.
                }
            }
        }

        if role == "assistant" {
            if content_blocks.is_empty() && tool_call_infos.is_empty() {
                pending_tool_calls.clear();
                continue;
            }
            // Ensure non-empty content
            if content_blocks.is_empty() {
                content_blocks.push(json!({"type": "text", "text": ""}));
            }
            out.push(json!({
                "role": "assistant",
                "content": content_blocks,
            }));
            pending_tool_calls = tool_call_infos.into_iter().collect();
        } else if role == "user" || role == "system" {
            // In Anthropic format, tool results are sent as user messages
            // with tool_result content blocks.
            if role == "system" {
                // System messages in the middle of conversation become user messages
                // in Anthropic format (top-level system is already handled).
                if content_blocks.is_empty() {
                    continue;
                }
                out.push(json!({
                    "role": "user",
                    "content": content_blocks,
                }));
            } else if !content_blocks.is_empty() {
                out.push(json!({
                    "role": "user",
                    "content": content_blocks,
                }));
            }
            pending_tool_calls.clear();
        } else if role == "tool" {
            // Anthropic format: tool results are part of user messages
            // We've already handled them as ContentBlock::ToolResult above
            // in the user message. But standalone tool messages should also work.
            if !content_blocks.is_empty() {
                out.push(json!({
                    "role": "user",
                    "content": content_blocks,
                }));
            }
            pending_tool_calls.clear();
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Response parsing
// ---------------------------------------------------------------------------

/// Parse a non-streaming Anthropic Messages API response.
fn parse_anthropic_message(payload: &Value) -> Result<MessageResponse> {
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or("msg_anthropic")
        .to_string();
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let role = payload
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("assistant")
        .to_string();
    let stop_reason = payload
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(str::to_string);

    let mut content_blocks = Vec::new();
    if let Some(blocks) = payload.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(text) = block.get("text").and_then(Value::as_str) {
                        content_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                            cache_control: None,
                        });
                    }
                }
                Some("thinking") => {
                    if let Some(thinking) = block.get("thinking").and_then(Value::as_str) {
                        content_blocks.push(ContentBlock::Thinking {
                            thinking: thinking.to_string(),
                        });
                    }
                }
                Some("tool_use") => {
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown_tool")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or(Value::Null);
                    content_blocks.push(ContentBlock::ToolUse {
                        id,
                        name,
                        input,
                        caller: None,
                    });
                }
                _ => {}
            }
        }
    }

    let usage = parse_usage(payload.get("usage"));

    Ok(MessageResponse {
        id,
        r#type: "message".to_string(),
        role,
        content: content_blocks,
        model,
        stop_reason,
        stop_sequence: None,
        container: None,
        usage,
    })
}

/// Parse a single Anthropic SSE event into a `StreamEvent`.
///
/// The Anthropic SSE wire format maps 1:1 to DSTUI's internal `StreamEvent`
/// enum — no remapping needed (unlike the Chat Completions path).
fn parse_anthropic_sse_event(event_type: &str, payload: &Value) -> Option<StreamEvent> {
    match event_type {
        "message_start" => {
            let message = payload
                .get("message")
                .and_then(|m| parse_anthropic_message(m).ok())?;
            Some(StreamEvent::MessageStart { message })
        }
        "content_block_start" => {
            let index = payload.get("index").and_then(Value::as_u64)? as u32;
            let block = payload.get("content_block")?;
            let block_type = block.get("type").and_then(Value::as_str)?;
            let content_block = match block_type {
                "text" => ContentBlockStart::Text {
                    text: block
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "thinking" => ContentBlockStart::Thinking {
                    thinking: block
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "tool_use" => ContentBlockStart::ToolUse {
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or("tool_call")
                        .to_string(),
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown_tool")
                        .to_string(),
                    input: block.get("input").cloned().unwrap_or(json!({})),
                    caller: None,
                },
                _ => return None,
            };
            Some(StreamEvent::ContentBlockStart {
                index,
                content_block,
            })
        }
        "content_block_delta" => {
            let index = payload.get("index").and_then(Value::as_u64)? as u32;
            let delta = payload.get("delta")?;
            let delta_type = delta.get("type").and_then(Value::as_str)?;
            let delta = match delta_type {
                "text_delta" => Delta::TextDelta {
                    text: delta
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "thinking_delta" => Delta::ThinkingDelta {
                    thinking: delta
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                "input_json_delta" => Delta::InputJsonDelta {
                    partial_json: delta
                        .get("partial_json")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                },
                _ => return None,
            };
            Some(StreamEvent::ContentBlockDelta { index, delta })
        }
        "content_block_stop" => {
            let index = payload.get("index").and_then(Value::as_u64)? as u32;
            Some(StreamEvent::ContentBlockStop { index })
        }
        "message_delta" => {
            let delta = payload.get("delta")?;
            let usage = payload.get("usage").map(|u| parse_usage(Some(u)));
            Some(StreamEvent::MessageDelta {
                delta: MessageDelta {
                    stop_reason: delta
                        .get("stop_reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    stop_sequence: delta
                        .get("stop_sequence")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                },
                usage,
            })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// URL helpers
// ---------------------------------------------------------------------------

/// Build the Anthropic Messages API URL from the configured base URL.
///
/// DeepSeek's Anthropic endpoint lives at the API root:
/// `https://api.deepseek.com/anthropic/v1/messages`.
///
/// The Chat API base URL may carry a version suffix (`/beta`, `/v1`) that is
/// not relevant to the Anthropic surface. This function strips those suffixes
/// so that, for example, `https://api.deepseek.com/beta` → the correct
/// Anthropic URL rather than the non-existent `/beta/anthropic/...`.
fn anthropic_api_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');

    // If the user explicitly pointed us at the Anthropic endpoint, use it as-is.
    if trimmed.ends_with("/anthropic") {
        return format!("{trimmed}/v1/messages");
    }
    if trimmed.ends_with("/anthropic/v1/messages") {
        return trimmed.to_string();
    }

    // Strip version suffixes (/beta, /v1, /v2, …) that belong to the Chat
    // surface so we construct the Anthropic URL from the API root.
    let root = strip_version_suffix(trimmed);

    if root.contains("/anthropic") {
        // Rare: root contains /anthropic mid-path. Append /messages if needed.
        if root.ends_with("/messages") {
            root.to_string()
        } else {
            format!("{root}/messages")
        }
    } else {
        format!("{root}/anthropic/v1/messages")
    }
}

/// Remove a known version segment from the end of a base URL.
/// `/beta`, `/v1`, `/v2`, … are stripped so the caller can append a
/// different API surface path.
fn strip_version_suffix(url: &str) -> &str {
    if let Some(idx) = url.rfind('/') {
        let segment = &url[idx + 1..];
        if is_anthropic_version_segment(segment) {
            return &url[..idx];
        }
    }
    url
}

fn is_anthropic_version_segment(segment: &str) -> bool {
    segment.eq_ignore_ascii_case("beta")
        || segment
            .strip_prefix('v')
            .or_else(|| segment.strip_prefix('V'))
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL routing tests ---

    #[test]
    fn anthropic_url_from_standard_base() {
        let url = anthropic_api_url("https://api.deepseek.com");
        assert_eq!(url, "https://api.deepseek.com/anthropic/v1/messages");
    }

    #[test]
    fn anthropic_url_strips_beta_suffix() {
        // The DEFAULT base URL ends with /beta — Anthropic must resolve to root.
        let url = anthropic_api_url("https://api.deepseek.com/beta");
        assert_eq!(url, "https://api.deepseek.com/anthropic/v1/messages");
    }

    #[test]
    fn anthropic_url_strips_v1_suffix() {
        let url = anthropic_api_url("https://api.deepseek.com/v1");
        assert_eq!(url, "https://api.deepseek.com/anthropic/v1/messages");
    }

    #[test]
    fn anthropic_url_already_anthropic() {
        let url = anthropic_api_url("https://api.deepseek.com/anthropic");
        assert_eq!(url, "https://api.deepseek.com/anthropic/v1/messages");
    }

    #[test]
    fn anthropic_url_already_full() {
        let url = anthropic_api_url("https://api.deepseek.com/anthropic/v1/messages");
        assert_eq!(url, "https://api.deepseek.com/anthropic/v1/messages");
    }

    #[test]
    fn anthropic_url_from_custom_port() {
        let url = anthropic_api_url("http://localhost:8080/v1");
        assert_eq!(url, "http://localhost:8080/anthropic/v1/messages");
    }

    // --- Message building tests ---

    #[test]
    fn builds_simple_user_message() {
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "hello".to_string(),
                cache_control: None,
            }],
        }];
        let built = build_anthropic_messages(&messages);
        assert_eq!(built.len(), 1);
        assert_eq!(built[0]["role"], "user");
        let content = built[0]["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "hello");
    }

    #[test]
    fn wraps_special_token_in_content_block() {
        // The key immunity test: <think> stays inside a text block,
        // never exposed as raw tokenizable text.
        let messages = vec![Message {
            role: "user".to_string(),
            content: vec![ContentBlock::Text {
                text: "<think>".to_string(),
                cache_control: None,
            }],
        }];
        let built = build_anthropic_messages(&messages);
        let content = built[0]["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "<think>");
        // The raw <think> string is nested inside a JSON text block,
        // not at the top level of any key.
        let wire = serde_json::to_string(&built[0]).unwrap();
        assert!(wire.contains("\"<think>\""));
    }

    #[test]
    fn builds_tool_use_and_result() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "read_file".to_string(),
                    input: json!({"path": "/tmp/x"}),
                    caller: None,
                }],
            },
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::ToolResult {
                    tool_use_id: "call_1".to_string(),
                    content: "file contents".to_string(),
                    is_error: None,
                    content_blocks: None,
                }],
            },
        ];
        let built = build_anthropic_messages(&messages);
        assert_eq!(built.len(), 2);
        // First: assistant with tool_use
        let ac = built[0]["content"].as_array().unwrap();
        assert_eq!(ac[0]["type"], "tool_use");
        assert_eq!(ac[0]["id"], "call_1");
        // Second: user with tool_result
        let uc = built[1]["content"].as_array().unwrap();
        assert_eq!(uc[0]["type"], "tool_result");
        assert_eq!(uc[0]["tool_use_id"], "call_1");
    }

    #[test]
    fn builds_request_with_system() {
        let request = MessageRequest {
            model: "deepseek-v4-pro".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::Text {
                    text: "hi".to_string(),
                    cache_control: None,
                }],
            }],
            max_tokens: 4096,
            system: Some(SystemPrompt::Text("You are helpful.".to_string())),
            tools: None,
            tool_choice: None,
            metadata: None,
            thinking: None,
            reasoning_effort: None,
            stream: None,
            temperature: None,
            top_p: None,
        };
        let body = build_anthropic_request(&request, false);
        assert_eq!(body["model"], "deepseek-v4-pro");
        assert_eq!(body["system"], "You are helpful.");
        assert_eq!(body["stream"], false);
    }

    // --- Response parsing tests ---

    #[test]
    fn parses_text_response() {
        let payload = json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "deepseek-v4-pro",
            "content": [
                {"type": "text", "text": "Hello!"}
            ],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let msg = parse_anthropic_message(&payload).unwrap();
        assert_eq!(msg.id, "msg_1");
        assert_eq!(msg.stop_reason.as_deref(), Some("end_turn"));
        assert_eq!(msg.content.len(), 1);
        match &msg.content[0] {
            ContentBlock::Text { text, .. } => assert_eq!(text, "Hello!"),
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn parses_tool_use_response() {
        let payload = json!({
            "id": "msg_2",
            "type": "message",
            "role": "assistant",
            "model": "deepseek-v4-pro",
            "content": [
                {"type": "tool_use", "id": "call_abc", "name": "read_file", "input": {"path": "/x"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 15}
        });
        let msg = parse_anthropic_message(&payload).unwrap();
        match &msg.content[0] {
            ContentBlock::ToolUse { id, name, input, .. } => {
                assert_eq!(id, "call_abc");
                assert_eq!(name, "read_file");
                assert_eq!(input, &json!({"path": "/x"}));
            }
            other => panic!("expected ToolUse, got {other:?}"),
        }
    }

    #[test]
    fn parses_sse_text_delta() {
        let payload = json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": {"type": "text_delta", "text": "hi"}
        });
        let event = parse_anthropic_sse_event("content_block_delta", &payload).unwrap();
        match event {
            StreamEvent::ContentBlockDelta { index, delta } => {
                assert_eq!(index, 0);
                match delta {
                    Delta::TextDelta { text } => assert_eq!(text, "hi"),
                    other => panic!("expected TextDelta, got {other:?}"),
                }
            }
            other => panic!("expected ContentBlockDelta, got {other:?}"),
        }
    }

    #[test]
    fn ping_event_is_ignored() {
        let payload = json!({});
        assert!(parse_anthropic_sse_event("ping", &payload).is_none());
    }
}
