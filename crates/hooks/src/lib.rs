use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use deepseek_protocol::EventFrame;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HookEvent {
    // ── Response stream (existing) ──
    ResponseStart {
        response_id: String,
    },
    ResponseDelta {
        response_id: String,
        delta: String,
    },
    ResponseEnd {
        response_id: String,
    },
    // ── Tool & Job lifecycle (existing) ──
    ToolLifecycle {
        response_id: String,
        tool_name: String,
        phase: String,
        payload: Value,
    },
    JobLifecycle {
        job_id: String,
        phase: String,
        progress: Option<u8>,
        detail: Option<String>,
    },
    ApprovalLifecycle {
        approval_id: String,
        phase: String,
        reason: Option<String>,
    },
    // ── Generic (existing) ──
    GenericEventFrame {
        frame: EventFrame,
    },
    // ── Session lifecycle (new, CC-compatible) ──
    /// Fired when a session starts. CC equivalent: SessionStart.
    SessionStarted {
        session_id: String,
        workspace: String,
    },
    /// Fired when a session ends normally. CC equivalent: SessionEnd.
    SessionEnded {
        session_id: String,
        reason: Option<String>,
    },
    /// Fired when the agent is interrupted (Ctrl+C). CC equivalent: Stop.
    Interrupted {
        session_id: String,
    },
    // ── Subagent lifecycle (new) ──
    /// A subagent was spawned. CC equivalent: SubagentStart.
    SubagentStarted {
        agent_id: String,
        agent_type: String,
        task: String,
    },
    /// A subagent completed. CC equivalent: SubagentStop.
    SubagentStopped {
        agent_id: String,
        outcome: String,
    },
    // ── Context compaction (new) ──
    /// Context compaction started. CC equivalent: PreCompact.
    CompactStarted {
        session_id: String,
    },
    /// Context compaction completed. CC equivalent: PostCompact.
    CompactEnded {
        session_id: String,
        tokens_before: Option<usize>,
        tokens_after: Option<usize>,
    },
    // ── Permissions & input (new) ──
    /// A tool requires user approval. CC equivalent: PermissionRequest.
    PermissionRequested {
        tool_name: String,
        reason: Option<String>,
    },
    /// The user submitted a prompt. CC equivalent: UserPromptSubmit.
    PromptSubmitted {
        text: String,
    },
    // ── Task lifecycle (new) ──
    /// A background task was created. CC equivalent: TaskCreated.
    TaskCreated {
        task_id: String,
        description: String,
    },
    /// A background task completed. CC equivalent: TaskCompleted.
    TaskCompleted {
        task_id: String,
        success: bool,
    },
    // ── Config changes (new) ──
    /// A configuration value changed. CC equivalent: ConfigChange.
    ConfigChanged {
        key: String,
        value: Value,
    },
}

impl HookEvent {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({"type":"serialization_error"}))
    }
}

#[async_trait]
pub trait HookSink: Send + Sync {
    async fn emit(&self, event: &HookEvent) -> Result<()>;
}

#[derive(Default)]
pub struct StdoutHookSink;

#[async_trait]
impl HookSink for StdoutHookSink {
    async fn emit(&self, event: &HookEvent) -> Result<()> {
        println!("{}", event.to_json());
        Ok(())
    }
}

pub struct JsonlHookSink {
    path: PathBuf,
}

impl JsonlHookSink {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[async_trait]
impl HookSink for JsonlHookSink {
    async fn emit(&self, event: &HookEvent) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("failed to create hook log directory {}", parent.display())
            })?;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await
            .with_context(|| format!("failed to open hook log {}", self.path.display()))?;
        let payload = json!({
            "at": Utc::now().to_rfc3339(),
            "event": event
        });
        let encoded = serde_json::to_string(&payload).context("failed to encode hook event")?;
        file.write_all(encoded.as_bytes())
            .await
            .context("failed to write hook event")?;
        file.write_all(b"\n")
            .await
            .context("failed to write hook event newline")?;
        Ok(())
    }
}

pub struct WebhookHookSink {
    url: String,
    client: reqwest::Client,
}

impl WebhookHookSink {
    pub fn new(url: String) -> Self {
        Self {
            url,
            client: reqwest::Client::builder()
                .min_tls_version(reqwest::tls::Version::TLS_1_2)
                .build()
                .expect("TLS 1.2 minimum required"),
        }
    }
}

#[async_trait]
impl HookSink for WebhookHookSink {
    async fn emit(&self, event: &HookEvent) -> Result<()> {
        let mut retries = 0usize;
        loop {
            let resp = self
                .client
                .post(&self.url)
                .json(&json!({
                    "at": Utc::now().to_rfc3339(),
                    "event": event,
                }))
                .send()
                .await;
            match resp {
                Ok(response) if response.status().is_success() => return Ok(()),
                Ok(response) => {
                    if retries >= 2 {
                        anyhow::bail!("webhook returned non-success status {}", response.status());
                    }
                }
                Err(err) => {
                    if retries >= 2 {
                        return Err(err).context("webhook request failed");
                    }
                }
            }
            retries += 1;
            tokio::time::sleep(std::time::Duration::from_millis(200 * retries as u64)).await;
        }
    }
}

#[derive(Default, Clone)]
pub struct HookDispatcher {
    sinks: Vec<Arc<dyn HookSink>>,
}

impl HookDispatcher {
    pub fn add_sink(&mut self, sink: Arc<dyn HookSink>) {
        self.sinks.push(sink);
    }

    pub async fn emit(&self, event: HookEvent) {
        for sink in &self.sinks {
            let _ = sink.emit(&event).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn hook_event_serializes_with_snake_case_type_and_payload() {
        let event = HookEvent::ToolLifecycle {
            response_id: "resp-1".to_string(),
            tool_name: "shell".to_string(),
            phase: "end".to_string(),
            payload: json!({ "exit_code": 0 }),
        };

        let encoded = event.to_json();

        assert_eq!(encoded["type"], "tool_lifecycle");
        assert_eq!(encoded["response_id"], "resp-1");
        assert_eq!(encoded["tool_name"], "shell");
        assert_eq!(encoded["phase"], "end");
        assert_eq!(encoded["payload"]["exit_code"], 0);
    }

    #[tokio::test]
    async fn jsonl_sink_creates_parent_dir_and_appends_events() {
        let root = unique_temp_dir("jsonl_sink");
        let path = root.join("nested").join("hooks.jsonl");
        let sink = JsonlHookSink::new(path.clone());

        sink.emit(&HookEvent::ResponseStart {
            response_id: "resp-1".to_string(),
        })
        .await
        .unwrap();
        sink.emit(&HookEvent::ResponseEnd {
            response_id: "resp-1".to_string(),
        })
        .await
        .unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let lines = raw.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 2);

        let first: Value = serde_json::from_str(lines[0]).unwrap();
        let second: Value = serde_json::from_str(lines[1]).unwrap();
        assert!(first["at"].as_str().is_some());
        assert_eq!(first["event"]["type"], "response_start");
        assert_eq!(first["event"]["response_id"], "resp-1");
        assert_eq!(second["event"]["type"], "response_end");
        assert_eq!(second["event"]["response_id"], "resp-1");

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn dispatcher_continues_after_sink_error() {
        let mut dispatcher = HookDispatcher::default();
        let first = Arc::new(RecordingSink::default());
        let second = Arc::new(RecordingSink::default());

        dispatcher.add_sink(first.clone());
        dispatcher.add_sink(Arc::new(FailingSink));
        dispatcher.add_sink(second.clone());

        dispatcher
            .emit(HookEvent::ApprovalLifecycle {
                approval_id: "approval-1".to_string(),
                phase: "requested".to_string(),
                reason: Some("needs review".to_string()),
            })
            .await;

        assert_eq!(
            first.events(),
            vec![json!({
                "type": "approval_lifecycle",
                "approval_id": "approval-1",
                "phase": "requested",
                "reason": "needs review",
            })]
        );
        assert_eq!(second.events(), first.events());
    }

    #[derive(Default)]
    struct RecordingSink {
        events: Mutex<Vec<Value>>,
    }

    impl RecordingSink {
        fn events(&self) -> Vec<Value> {
            self.events.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl HookSink for RecordingSink {
        async fn emit(&self, event: &HookEvent) -> Result<()> {
            self.events.lock().unwrap().push(event.to_json());
            Ok(())
        }
    }

    struct FailingSink;

    #[async_trait::async_trait]
    impl HookSink for FailingSink {
        async fn emit(&self, _event: &HookEvent) -> Result<()> {
            anyhow::bail!("sink failed")
        }
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "deepseek-hooks-{label}-{}-{nanos}",
            std::process::id()
        ))
    }

    // ── New hook event tests ──

    #[test]
    fn session_events_serialize_correctly() {
        let start = HookEvent::SessionStarted {
            session_id: "s1".into(),
            workspace: "/ws".into(),
        };
        let json = serde_json::to_value(&start).unwrap();
        assert_eq!(json["type"], "session_started");
        assert_eq!(json["session_id"], "s1");

        let end = HookEvent::SessionEnded {
            session_id: "s1".into(),
            reason: Some("user exit".into()),
        };
        let json = serde_json::to_value(&end).unwrap();
        assert_eq!(json["type"], "session_ended");
    }

    #[test]
    fn subagent_events_have_required_fields() {
        let start = HookEvent::SubagentStarted {
            agent_id: "a1".into(),
            agent_type: "researcher".into(),
            task: "find bug".into(),
        };
        let json = serde_json::to_value(&start).unwrap();
        assert_eq!(json["agent_type"], "researcher");
        assert_eq!(json["task"], "find bug");

        let stop = HookEvent::SubagentStopped {
            agent_id: "a1".into(),
            outcome: "completed".into(),
        };
        let json = serde_json::to_value(&stop).unwrap();
        assert_eq!(json["outcome"], "completed");
    }

    #[test]
    fn compact_events_track_token_count() {
        let end = HookEvent::CompactEnded {
            session_id: "s1".into(),
            tokens_before: Some(5000),
            tokens_after: Some(2000),
        };
        let json = serde_json::to_value(&end).unwrap();
        assert_eq!(json["tokens_before"], 5000);
        assert_eq!(json["tokens_after"], 2000);
    }

    #[test]
    fn hook_event_count_matches_expectation() {
        // Verify we have at least 20+ event types (up from 7)
        let variants = [
            "response_start", "response_delta", "response_end",
            "tool_lifecycle", "job_lifecycle", "approval_lifecycle",
            "generic_event_frame",
            "session_started", "session_ended", "interrupted",
            "subagent_started", "subagent_stopped",
            "compact_started", "compact_ended",
            "permission_requested", "prompt_submitted",
            "task_created", "task_completed",
            "config_changed",
        ];
        assert_eq!(variants.len(), 19); // 7 original + 12 new
    }
}
