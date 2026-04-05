//! Speculative execution engine for background AI analysis.
//!
//! The AI thinks ahead: when the cursor is idle, it analyzes nearby code
//! and generates improvement suggestions. These are displayed as ghost text
//! that the user can accept, reject, or cycle through.

use aura_ai::{AiConfig, AiEvent, AnthropicClient, EditorContext, Message};
use aura_core::{Buffer, Cursor};
use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Instant;

/// How aggressively the AI generates suggestions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aggressiveness {
    /// Only suggest fixes for obvious issues (errors, warnings).
    Minimal,
    /// Suggest improvements when clear wins are available.
    Moderate,
    /// Proactively suggest refactors, simplifications, and optimisations.
    Proactive,
}

impl Aggressiveness {
    /// Display label.
    pub fn label(&self) -> &str {
        match self {
            Self::Minimal => "minimal",
            Self::Moderate => "moderate",
            Self::Proactive => "proactive",
        }
    }

    /// Cycle to the next level.
    pub fn next(self) -> Self {
        match self {
            Self::Minimal => Self::Moderate,
            Self::Moderate => Self::Proactive,
            Self::Proactive => Self::Minimal,
        }
    }
}

/// Category of a suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SuggestionCategory {
    /// Bug fix or error correction.
    Fix,
    /// Code simplification.
    Simplify,
    /// Add missing error handling.
    ErrorHandling,
    /// Performance improvement.
    Performance,
    /// Code style / readability.
    Refactor,
}

impl SuggestionCategory {
    /// Short label for display.
    pub fn label(&self) -> &str {
        match self {
            Self::Fix => "fix",
            Self::Simplify => "simplify",
            Self::ErrorHandling => "errors",
            Self::Performance => "perf",
            Self::Refactor => "refactor",
        }
    }
}

// ---------------------------------------------------------------------------
// Next-edit prediction
// ---------------------------------------------------------------------------

/// Why a particular line was predicted as the next edit location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PredictionReason {
    /// LSP diagnostic (error/warning) at this line.
    Diagnostic,
    /// Continuation of a sequential edit pattern.
    Sequential,
    /// Same identifier was edited elsewhere.
    PatternMatch,
    /// Recently edited line the cursor moved away from.
    RecentReturn,
}

impl PredictionReason {
    /// Short label for status bar display.
    pub fn label(&self) -> &str {
        match self {
            Self::Diagnostic => "diagnostic",
            Self::Sequential => "sequential",
            Self::PatternMatch => "pattern",
            Self::RecentReturn => "recent",
        }
    }
}

/// A predicted next-edit location.
#[derive(Debug, Clone)]
pub struct NextEditPrediction {
    /// Predicted line number (0-indexed).
    pub line: usize,
    /// Confidence score (0.0–1.0).
    pub confidence: f32,
    /// Why this line was predicted.
    pub reason: PredictionReason,
}

/// Predict where the user will edit next based on heuristics.
///
/// `recent_edit_lines` should be line numbers of recent edits in reverse
/// chronological order (most recent first). Returns up to 3 predictions
/// sorted by confidence, excluding `cursor_line`.
pub fn predict_next_edits(
    recent_edit_lines: &[usize],
    cursor_line: usize,
    diagnostics: &[(usize, String)],
    buffer_line_count: usize,
) -> Vec<NextEditPrediction> {
    let mut predictions: Vec<NextEditPrediction> = Vec::new();
    let mut seen_lines = std::collections::HashSet::new();
    seen_lines.insert(cursor_line);

    // 1. Diagnostic-driven: lines with errors/warnings not recently edited.
    let recent_set: std::collections::HashSet<usize> = recent_edit_lines.iter().copied().collect();
    for (line, _msg) in diagnostics {
        if *line < buffer_line_count && seen_lines.insert(*line) && !recent_set.contains(line) {
            predictions.push(NextEditPrediction {
                line: *line,
                confidence: 0.9,
                reason: PredictionReason::Diagnostic,
            });
        }
    }

    // 2. Sequential pattern: detect consecutive edit lines and predict next.
    if recent_edit_lines.len() >= 2 {
        let last = recent_edit_lines[0];
        let prev = recent_edit_lines[1];
        if last == prev.saturating_add(1) {
            let next = last.saturating_add(1);
            if next < buffer_line_count && seen_lines.insert(next) {
                predictions.push(NextEditPrediction {
                    line: next,
                    confidence: 0.7,
                    reason: PredictionReason::Sequential,
                });
            }
        } else if last == prev.saturating_sub(1) && last > 0 {
            let next = last.saturating_sub(1);
            if seen_lines.insert(next) {
                predictions.push(NextEditPrediction {
                    line: next,
                    confidence: 0.7,
                    reason: PredictionReason::Sequential,
                });
            }
        }
    }

    // 3. Return to recent: lines edited recently that cursor moved away from.
    for &line in recent_edit_lines.iter().take(5) {
        if line < buffer_line_count
            && seen_lines.insert(line)
            && (line as isize - cursor_line as isize).unsigned_abs() > 5
        {
            predictions.push(NextEditPrediction {
                line,
                confidence: 0.4,
                reason: PredictionReason::RecentReturn,
            });
        }
    }

    // Sort by confidence descending, take top 3.
    predictions.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    predictions.truncate(3);
    predictions
}

/// A single AI-generated ghost suggestion.
#[derive(Debug, Clone)]
pub struct GhostSuggestion {
    /// The suggested replacement text.
    pub text: String,
    /// What category this suggestion falls into.
    pub category: SuggestionCategory,
    /// Brief explanation (shown in status bar).
    pub explanation: String,
    /// Start line of the region this applies to.
    pub start_line: usize,
    /// End line of the region this applies to.
    pub end_line: usize,
}

/// A proposed change to a single file (for multi-file changesets).
#[derive(Debug, Clone)]
pub struct FileChange {
    /// Path of the file to modify.
    pub file_path: String,
    /// Description of the change.
    pub description: String,
    /// The proposed replacement text.
    pub proposed_text: String,
    /// Start line in the target file.
    pub start_line: usize,
    /// End line in the target file.
    pub end_line: usize,
    /// Whether the user has accepted this individual change.
    pub accepted: Option<bool>,
}

/// A cross-file changeset proposed by the AI.
#[derive(Debug, Clone)]
pub struct Changeset {
    /// Human-readable summary.
    pub summary: String,
    /// Individual file changes.
    pub changes: Vec<FileChange>,
}

impl Changeset {
    /// Count of changes not yet decided.
    pub fn pending_count(&self) -> usize {
        self.changes.iter().filter(|c| c.accepted.is_none()).count()
    }

    /// Accept all pending changes.
    pub fn accept_all(&mut self) {
        for change in &mut self.changes {
            if change.accepted.is_none() {
                change.accepted = Some(true);
            }
        }
    }

    /// Reject all pending changes.
    pub fn reject_all(&mut self) {
        for change in &mut self.changes {
            if change.accepted.is_none() {
                change.accepted = Some(false);
            }
        }
    }
}

/// Cache key for suggestion lookups.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct CacheKey {
    file_path: String,
    start_line: usize,
    end_line: usize,
    content_hash: u64,
}

/// Simple hash for code content (FNV-1a).
fn hash_content(content: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in content.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Events from the speculative analysis background thread.
#[derive(Debug)]
pub enum SpecEvent {
    /// Analysis produced suggestions for a region.
    Suggestions {
        /// Path of the file the suggestions apply to.
        file_path: String,
        /// First line of the analysed region.
        start_line: usize,
        /// Last line of the analysed region.
        end_line: usize,
        /// Suggested ghost-text edits.
        suggestions: Vec<GhostSuggestion>,
    },
    /// Multi-file changeset proposed.
    ChangesetProposed(Changeset),
    /// Analysis failed.
    Error(String),
}

/// The speculative execution engine.
pub struct SpeculativeEngine {
    /// AI client for background requests.
    ai_client: AnthropicClient,
    /// Optional model override for speculative requests.
    pub model_override: String,
    /// Receiver for background analysis results.
    event_rx: Option<mpsc::Receiver<SpecEvent>>,
    /// Cached suggestions keyed by region.
    cache: HashMap<CacheKey, Vec<GhostSuggestion>>,
    /// Last cursor position observed.
    last_cursor: Cursor,
    /// When the cursor last moved.
    last_cursor_move: Instant,
    /// Whether analysis is currently in-flight.
    analyzing: bool,
    /// Current aggressiveness level.
    pub aggressiveness: Aggressiveness,
    /// Currently active ghost suggestions for display.
    pub active_suggestions: Vec<GhostSuggestion>,
    /// Which suggestion is currently displayed (index into active_suggestions).
    pub suggestion_index: usize,
    /// Pending changeset for cross-file changes.
    pub pending_changeset: Option<Changeset>,
    /// Idle time (in ms) before triggering analysis.
    idle_threshold_ms: u64,
    /// Predicted next-edit locations (heuristic-based).
    pub edit_predictions: Vec<NextEditPrediction>,
    /// Index for cycling through predictions.
    pub prediction_index: usize,
}

impl SpeculativeEngine {
    /// Create a new engine from an AI config.
    pub fn new(config: AiConfig) -> anyhow::Result<Self> {
        let ai_client = AnthropicClient::new(config)?;
        Ok(Self {
            ai_client,
            model_override: String::new(),
            event_rx: None,
            cache: HashMap::new(),
            last_cursor: Cursor::origin(),
            last_cursor_move: Instant::now(),
            analyzing: false,
            aggressiveness: Aggressiveness::Moderate,
            active_suggestions: Vec::new(),
            suggestion_index: 0,
            pending_changeset: None,
            idle_threshold_ms: 3000, // 3 seconds of idle before analyzing
            edit_predictions: Vec::new(),
            prediction_index: 0,
        })
    }

    /// Notify that the cursor moved.
    pub fn cursor_moved(&mut self, cursor: &Cursor) {
        if cursor.row != self.last_cursor.row || cursor.col != self.last_cursor.col {
            self.last_cursor = *cursor;
            self.last_cursor_move = Instant::now();
            // Clear active suggestions when cursor moves to a different line.
            if cursor.row != self.last_cursor.row {
                self.active_suggestions.clear();
                self.suggestion_index = 0;
            }
        }
    }

    /// Notify that the buffer was edited (invalidates cache for affected region).
    pub fn buffer_edited(&mut self, _line: usize) {
        // Invalidate all cache entries — simple strategy for now.
        self.cache.clear();
        self.active_suggestions.clear();
        self.suggestion_index = 0;
        self.edit_predictions.clear();
        self.prediction_index = 0;
    }

    /// Update next-edit predictions from heuristics.
    pub fn update_predictions(
        &mut self,
        recent_edit_lines: &[usize],
        cursor_line: usize,
        diagnostics: &[(usize, String)],
        buffer_line_count: usize,
    ) {
        // Only predict after 500ms idle.
        if self.last_cursor_move.elapsed().as_millis() < 500 {
            self.edit_predictions.clear();
            self.prediction_index = 0;
            return;
        }
        let new_predictions = predict_next_edits(
            recent_edit_lines,
            cursor_line,
            diagnostics,
            buffer_line_count,
        );
        if new_predictions.iter().map(|p| p.line).collect::<Vec<_>>()
            != self
                .edit_predictions
                .iter()
                .map(|p| p.line)
                .collect::<Vec<_>>()
        {
            self.edit_predictions = new_predictions;
            self.prediction_index = 0;
        }
    }

    /// Clear all edit predictions.
    pub fn clear_predictions(&mut self) {
        self.edit_predictions.clear();
        self.prediction_index = 0;
    }

    /// Get the currently selected prediction.
    pub fn current_prediction(&self) -> Option<&NextEditPrediction> {
        self.edit_predictions.get(self.prediction_index)
    }

    /// Cycle to the next prediction.
    pub fn next_prediction(&mut self) {
        if !self.edit_predictions.is_empty() {
            self.prediction_index = (self.prediction_index + 1) % self.edit_predictions.len();
        }
    }

    /// Cycle to the previous prediction.
    pub fn prev_prediction(&mut self) {
        if !self.edit_predictions.is_empty() {
            self.prediction_index = if self.prediction_index == 0 {
                self.edit_predictions.len() - 1
            } else {
                self.prediction_index - 1
            };
        }
    }

    /// Check if we should trigger background analysis (cursor idle long enough).
    pub fn should_analyze(&self) -> bool {
        !self.analyzing
            && self.last_cursor_move.elapsed().as_millis() >= self.idle_threshold_ms as u128
            && self.active_suggestions.is_empty()
            && self.aggressiveness != Aggressiveness::Minimal
    }

    /// Trigger background analysis for the code near the cursor.
    pub fn analyze(
        &mut self,
        buffer: &Buffer,
        cursor: &Cursor,
        semantic_context: Option<String>,
        diagnostics: &[String],
    ) {
        if self.analyzing {
            return;
        }

        let file_path = buffer
            .file_path()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        // Determine the region to analyze (current function or ±15 lines).
        let total_lines = buffer.line_count();
        let start_line = cursor.row.saturating_sub(15);
        let end_line = (cursor.row + 15).min(total_lines);

        // Build region content for cache check.
        let mut region_content = String::new();
        for i in start_line..end_line {
            if let Some(text) = buffer.line_text(i) {
                region_content.push_str(&text);
            }
        }

        let cache_key = CacheKey {
            file_path: file_path.clone(),
            start_line,
            end_line,
            content_hash: hash_content(&region_content),
        };

        // Check cache first.
        if let Some(cached) = self.cache.get(&cache_key) {
            self.active_suggestions = cached.clone();
            self.suggestion_index = 0;
            return;
        }

        // Build context and send to AI.
        let ctx = EditorContext::from_buffer_with_semantic(buffer, cursor, None, semantic_context);
        let system = build_analysis_prompt(&ctx, self.aggressiveness, diagnostics);
        let messages = vec![Message::text(
            "user",
            &format!(
                "Analyze lines {}-{} and suggest improvements. \
                 Respond with one suggestion per line in format: \
                 CATEGORY|EXPLANATION|REPLACEMENT_CODE\n\
                 Categories: fix, simplify, errors, perf, refactor\n\
                 If no improvements needed, respond with: NONE",
                start_line + 1,
                end_line
            ),
        )];

        let rx = if self.model_override.is_empty() {
            self.ai_client.stream_completion(&system, messages)
        } else {
            self.ai_client
                .stream_completion_with_model(&system, messages, &self.model_override)
        };
        let (event_tx, event_rx) = mpsc::channel();

        let fp = file_path.clone();
        let sl = start_line;
        let el = end_line;

        // Spawn a thread to collect the streaming response and parse it.
        std::thread::Builder::new()
            .name("spec-analyze".to_string())
            .spawn(move || {
                let mut full_text = String::new();
                loop {
                    match rx.recv() {
                        Ok(AiEvent::Token(text)) => full_text.push_str(&text),
                        Ok(AiEvent::Done(text)) => {
                            full_text = text;
                            break;
                        }
                        Ok(AiEvent::Error(e)) => {
                            let _ = event_tx.send(SpecEvent::Error(e));
                            return;
                        }
                        Ok(AiEvent::ToolUse { .. })
                        | Ok(AiEvent::ToolUseComplete { .. })
                        | Ok(AiEvent::Activity(_)) => {}
                        Err(_) => break,
                    }
                }

                let suggestions = parse_suggestions(&full_text, sl, el);
                let _ = event_tx.send(SpecEvent::Suggestions {
                    file_path: fp,
                    start_line: sl,
                    end_line: el,
                    suggestions,
                });
            })
            .ok();

        self.event_rx = Some(event_rx);
        self.analyzing = true;
    }

    /// Poll for completed analysis results.
    pub fn poll_events(&mut self) {
        let rx = match &self.event_rx {
            Some(rx) => rx,
            None => return,
        };

        match rx.try_recv() {
            Ok(SpecEvent::Suggestions {
                file_path,
                start_line,
                end_line,
                suggestions,
            }) => {
                // Cache the result.
                // We don't have the content hash here, so we use a simplified key.
                let cache_key = CacheKey {
                    file_path,
                    start_line,
                    end_line,
                    content_hash: 0, // Will be refreshed on next cache check.
                };
                self.cache.insert(cache_key, suggestions.clone());

                self.active_suggestions = suggestions;
                self.suggestion_index = 0;
                self.analyzing = false;
                self.event_rx = None;
            }
            Ok(SpecEvent::ChangesetProposed(changeset)) => {
                self.pending_changeset = Some(changeset);
                self.analyzing = false;
                self.event_rx = None;
            }
            Ok(SpecEvent::Error(_)) => {
                self.analyzing = false;
                self.event_rx = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.analyzing = false;
                self.event_rx = None;
            }
        }
    }

    /// Get the currently displayed ghost suggestion, if any.
    pub fn current_suggestion(&self) -> Option<&GhostSuggestion> {
        self.active_suggestions.get(self.suggestion_index)
    }

    /// Cycle to the next suggestion.
    pub fn next_suggestion(&mut self) {
        if !self.active_suggestions.is_empty() {
            self.suggestion_index = (self.suggestion_index + 1) % self.active_suggestions.len();
        }
    }

    /// Cycle to the previous suggestion.
    pub fn prev_suggestion(&mut self) {
        if !self.active_suggestions.is_empty() {
            self.suggestion_index = if self.suggestion_index == 0 {
                self.active_suggestions.len() - 1
            } else {
                self.suggestion_index - 1
            };
        }
    }

    /// Accept the current ghost suggestion (returns it for the caller to apply).
    pub fn accept_suggestion(&mut self) -> Option<GhostSuggestion> {
        if self.active_suggestions.is_empty() {
            return None;
        }
        let suggestion = self.active_suggestions[self.suggestion_index].clone();
        self.active_suggestions.clear();
        self.suggestion_index = 0;
        Some(suggestion)
    }

    /// Dismiss current suggestions.
    pub fn dismiss_suggestions(&mut self) {
        self.active_suggestions.clear();
        self.suggestion_index = 0;
    }

    /// Whether the engine is currently analyzing.
    pub fn is_analyzing(&self) -> bool {
        self.analyzing
    }

    /// Propose cross-file changes after an edit was accepted.
    pub fn propose_cross_file_changes(
        &mut self,
        buffer: &Buffer,
        cursor: &Cursor,
        semantic_context: Option<String>,
        related_files: Vec<String>,
    ) {
        if related_files.is_empty() || self.analyzing {
            return;
        }

        let ctx = EditorContext::from_buffer_with_semantic(buffer, cursor, None, semantic_context);
        let system = ctx.to_system_prompt();

        let file_list = related_files.join(", ");
        let messages = vec![Message::text(
            "user",
            &format!(
                "A change was just accepted in this file. \
                 Check if these related files need updates: {file_list}\n\
                 For each file that needs changes, respond with:\n\
                 FILE|path|start_line|end_line|description|replacement_code\n\
                 If no changes needed, respond with: NONE"
            ),
        )];

        let rx = if self.model_override.is_empty() {
            self.ai_client.stream_completion(&system, messages)
        } else {
            self.ai_client
                .stream_completion_with_model(&system, messages, &self.model_override)
        };
        let (event_tx, event_rx) = mpsc::channel();

        std::thread::Builder::new()
            .name("spec-crossfile".to_string())
            .spawn(move || {
                let mut full_text = String::new();
                loop {
                    match rx.recv() {
                        Ok(AiEvent::Token(text)) => full_text.push_str(&text),
                        Ok(AiEvent::Done(text)) => {
                            full_text = text;
                            break;
                        }
                        Ok(AiEvent::Error(e)) => {
                            let _ = event_tx.send(SpecEvent::Error(e));
                            return;
                        }
                        Ok(AiEvent::ToolUse { .. })
                        | Ok(AiEvent::ToolUseComplete { .. })
                        | Ok(AiEvent::Activity(_)) => {}
                        Err(_) => break,
                    }
                }

                let changeset = parse_changeset(&full_text);
                if let Some(cs) = changeset {
                    let _ = event_tx.send(SpecEvent::ChangesetProposed(cs));
                }
            })
            .ok();

        self.event_rx = Some(event_rx);
        self.analyzing = true;
    }
}

/// Build the system prompt for background analysis.
fn build_analysis_prompt(
    ctx: &EditorContext,
    aggressiveness: Aggressiveness,
    diagnostics: &[String],
) -> String {
    let mut prompt = ctx.to_system_prompt();

    prompt.push_str("\n--- ANALYSIS MODE ---\n");
    prompt.push_str("You are performing background code analysis. ");
    prompt.push_str("Suggest concrete improvements, not explanations.\n\n");

    match aggressiveness {
        Aggressiveness::Minimal => {
            prompt.push_str("Only suggest fixes for actual bugs or compiler/linter warnings.\n");
        }
        Aggressiveness::Moderate => {
            prompt
                .push_str("Suggest fixes for bugs, simplifications, and missing error handling.\n");
        }
        Aggressiveness::Proactive => {
            prompt.push_str(
                "Suggest all improvements: bugs, simplifications, error handling, \
                 performance, readability, and idiomatic patterns.\n",
            );
        }
    }

    if !diagnostics.is_empty() {
        prompt.push_str("\nCurrent diagnostics:\n");
        for diag in diagnostics {
            prompt.push_str(&format!("  - {diag}\n"));
        }
    }

    prompt
}

/// Parse the AI response into ghost suggestions.
fn parse_suggestions(text: &str, start_line: usize, end_line: usize) -> Vec<GhostSuggestion> {
    let mut suggestions = Vec::new();

    if text.trim() == "NONE" {
        return suggestions;
    }

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line == "NONE" {
            continue;
        }

        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() < 3 {
            continue;
        }

        let category = match parts[0].trim().to_lowercase().as_str() {
            "fix" => SuggestionCategory::Fix,
            "simplify" => SuggestionCategory::Simplify,
            "errors" | "error_handling" => SuggestionCategory::ErrorHandling,
            "perf" | "performance" => SuggestionCategory::Performance,
            "refactor" => SuggestionCategory::Refactor,
            _ => SuggestionCategory::Refactor,
        };

        let explanation = parts[1].trim().to_string();
        let replacement = parts[2].trim().to_string();

        if !replacement.is_empty() {
            suggestions.push(GhostSuggestion {
                text: replacement,
                category,
                explanation,
                start_line,
                end_line,
            });
        }
    }

    suggestions
}

/// Parse cross-file changeset from AI response.
fn parse_changeset(text: &str) -> Option<Changeset> {
    if text.trim() == "NONE" {
        return None;
    }

    let mut changes = Vec::new();

    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with("FILE|") {
            continue;
        }

        let parts: Vec<&str> = line.splitn(6, '|').collect();
        if parts.len() < 6 {
            continue;
        }

        let file_path = parts[1].trim().to_string();
        let start_line = parts[2].trim().parse::<usize>().unwrap_or(0);
        let end_line = parts[3].trim().parse::<usize>().unwrap_or(0);
        let description = parts[4].trim().to_string();
        let proposed_text = parts[5].trim().to_string();

        changes.push(FileChange {
            file_path,
            description,
            proposed_text,
            start_line,
            end_line,
            accepted: None,
        });
    }

    if changes.is_empty() {
        None
    } else {
        Some(Changeset {
            summary: format!("{} file(s) may need updates", changes.len()),
            changes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_suggestions_none() {
        let suggestions = parse_suggestions("NONE", 0, 10);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_parse_suggestions() {
        let text = "fix|Missing null check|if value.is_some() { value.unwrap() }\n\
                    simplify|Use if-let|if let Some(v) = value { v }\n\
                    perf|Avoid allocation|use &str instead of String";
        let suggestions = parse_suggestions(text, 5, 10);
        assert_eq!(suggestions.len(), 3);
        assert_eq!(suggestions[0].category, SuggestionCategory::Fix);
        assert_eq!(suggestions[1].category, SuggestionCategory::Simplify);
        assert_eq!(suggestions[2].category, SuggestionCategory::Performance);
        assert_eq!(suggestions[0].explanation, "Missing null check");
    }

    #[test]
    fn test_parse_changeset_none() {
        assert!(parse_changeset("NONE").is_none());
    }

    #[test]
    fn test_parse_changeset() {
        let text = "FILE|src/test.rs|10|20|Update test|fn new_test() {}";
        let changeset = parse_changeset(text).unwrap();
        assert_eq!(changeset.changes.len(), 1);
        assert_eq!(changeset.changes[0].file_path, "src/test.rs");
        assert_eq!(changeset.changes[0].start_line, 10);
    }

    #[test]
    fn test_aggressiveness_cycle() {
        assert_eq!(Aggressiveness::Minimal.next(), Aggressiveness::Moderate);
        assert_eq!(Aggressiveness::Moderate.next(), Aggressiveness::Proactive);
        assert_eq!(Aggressiveness::Proactive.next(), Aggressiveness::Minimal);
    }

    #[test]
    fn test_changeset_accept_reject() {
        let mut cs = Changeset {
            summary: "test".to_string(),
            changes: vec![
                FileChange {
                    file_path: "a.rs".to_string(),
                    description: "change a".to_string(),
                    proposed_text: "new code".to_string(),
                    start_line: 0,
                    end_line: 5,
                    accepted: None,
                },
                FileChange {
                    file_path: "b.rs".to_string(),
                    description: "change b".to_string(),
                    proposed_text: "new code".to_string(),
                    start_line: 0,
                    end_line: 5,
                    accepted: None,
                },
            ],
        };
        assert_eq!(cs.pending_count(), 2);
        cs.accept_all();
        assert_eq!(cs.pending_count(), 0);
        assert!(cs.changes.iter().all(|c| c.accepted == Some(true)));
    }

    #[test]
    fn test_hash_content() {
        let h1 = hash_content("hello");
        let h2 = hash_content("hello");
        let h3 = hash_content("world");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_ghost_suggestion_display() {
        let s = GhostSuggestion {
            text: "improved code".to_string(),
            category: SuggestionCategory::Simplify,
            explanation: "Use iterator".to_string(),
            start_line: 5,
            end_line: 10,
        };
        assert_eq!(s.category.label(), "simplify");
    }
}
