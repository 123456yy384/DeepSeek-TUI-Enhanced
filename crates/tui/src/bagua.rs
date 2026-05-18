//! Bagua-inspired optimizations for DeepSeek-TUI.
//!
//! Three concepts ported from the Bagua Architecture (八卦架构):
//! 1. Task Self-Awareness — detect task type from first message, suggest mode
//! 2. Smart Context Scoring — score messages by relevance, discard low-value first
//! 3. Left-Ear-In-Right-Ear-Out — per-session memory that auto-clears on switch

use crate::tui::app::AppMode;

/// Detect the likely best mode from the user's first message.
///
/// Bagua's Task Self-Awareness identifies task type across 23 preset scenarios.
/// Here we use a lightweight keyword heuristic that covers the most common cases.
pub fn detect_task_mode(first_message: &str) -> Option<AppMode> {
    let msg = first_message.to_lowercase();

    // Plan mode indicators — analysis, design, review, exploration
    let plan_keywords = [
        "设计", "规划", "分析", "审查", "检查", "调研", "探索",
        "design", "plan", "review", "analyze", "explore", "audit",
        "architecture", "refactor", "investigate", "what is", "how does",
        "explain", "文档", "总结", "梳理", "画图", "diagram",
    ];
    // YOLO mode indicators — fast, direct, bulk operations
    let yolo_keywords = [
        "快速", "直接", "全部", "批量", "自动", "不用确认",
        "fast", "all", "auto", "bulk", "quick", "just do",
        "一口气", "全部改", "all files", "every file",
    ];

    let plan_score = plan_keywords.iter().filter(|k| msg.contains(*k)).count();
    let yolo_score = yolo_keywords.iter().filter(|k| msg.contains(*k)).count();

    if plan_score > yolo_score && plan_score >= 2 {
        Some(AppMode::Plan)
    } else if yolo_score > plan_score && yolo_score >= 2 {
        Some(AppMode::Yolo)
    } else {
        None // default to Agent
    }
}

/// Score a message's relevance for context retention (0.0 = discard first, 1.0 = keep).
///
/// Bagua's Elimination Audit evaluates output quality and zeroes out low-value
/// trigram heads. Here we evaluate message quality for context compaction:
/// - User messages score higher (human intent is valuable)
/// - Recent messages score higher
/// - Tool results with errors score low
/// - Empty/short messages score low
pub fn score_message_relevance(
    index: usize,
    total: usize,
    is_user: bool,
    is_tool_result: bool,
    content_len: usize,
    has_error: bool,
) -> f64 {
    let mut score = 0.5;

    // Recency bonus: newer messages are more relevant
    let recency = index as f64 / total.max(1) as f64;
    score += recency * 0.25;

    // User messages are high-value (human intent)
    if is_user {
        score += 0.15;
    }

    // Tool results with errors are low-value noise
    if is_tool_result && has_error {
        score -= 0.3;
    }

    // Very short messages (<20 chars) are less valuable
    if content_len < 20 {
        score -= 0.15;
    } else if content_len > 200 {
        score += 0.1; // Substantive content bonus
    }

    score.clamp(0.0, 1.0)
}

/// Score and rank messages for compaction. Returns indices sorted by score
/// (lowest first = discard these first).
pub fn rank_messages_for_compaction(
    total: usize,
    user_indices: &[usize],
    error_indices: &[usize],
    content_lengths: &[usize],
) -> Vec<(usize, f64)> {
    let mut scored: Vec<(usize, f64)> = (0..total)
        .map(|i| {
            let is_user = user_indices.contains(&i);
            let has_error = error_indices.contains(&i);
            let len = content_lengths.get(i).copied().unwrap_or(0);
            let s = score_message_relevance(i, total, is_user, !is_user, len, has_error);
            (i, s)
        })
        .collect();
    scored.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}

/// Session memory buffer — accumulates within a session, auto-clears on switch.
///
/// Bagua's "Left Ear In, Right Ear Out" naturally defends against cross-sequence
/// overfitting. Here we apply the same principle to session context: each session
/// builds its own working memory that is completely discarded when switching.
#[derive(Default, Clone)]
pub struct SessionMemory {
    pub key_facts: Vec<String>,
    pub user_preferences: Vec<String>,
    pub recent_decisions: Vec<String>,
    session_id: Option<String>,
}

impl SessionMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a key fact for this session only.
    pub fn record_fact(&mut self, fact: impl Into<String>) {
        let fact = fact.into();
        if !self.key_facts.contains(&fact) {
            self.key_facts.push(fact);
        }
    }

    /// Record a user preference discovered during this session.
    pub fn record_preference(&mut self, pref: impl Into<String>) {
        self.user_preferences.push(pref.into());
    }

    /// Check if we're still in the same session. If not, clear everything.
    /// Returns true if the session changed (memory was cleared).
    pub fn check_session(&mut self, new_session_id: &str) -> bool {
        let changed = self
            .session_id
            .as_ref()
            .is_some_and(|id| id != new_session_id);
        if changed {
            self.key_facts.clear();
            self.user_preferences.clear();
            self.recent_decisions.clear();
        }
        self.session_id = Some(new_session_id.to_string());
        changed
    }

    /// Build a compact context string of session memory for inclusion in system prompt.
    pub fn to_context_string(&self) -> Option<String> {
        if self.key_facts.is_empty() && self.user_preferences.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if !self.key_facts.is_empty() {
            parts.push(format!(
                "Key facts: {}",
                self.key_facts.iter().take(5).cloned().collect::<Vec<_>>().join("; ")
            ));
        }
        if !self.user_preferences.is_empty() {
            parts.push(format!(
                "Preferences: {}",
                self.user_preferences.iter().take(3).cloned().collect::<Vec<_>>().join("; ")
            ));
        }
        Some(parts.join(" | "))
    }
}
