//! Token usage tracking and cost estimation.
//!
//! Tracks AI token usage per session with estimated costs.

/// Token usage record for a single AI request.
#[derive(Debug, Clone)]
pub struct TokenUsage {
    /// Feature that made the request.
    pub feature: String,
    /// Model used.
    pub model: String,
    /// Estimated input tokens.
    pub input_tokens: usize,
    /// Estimated output tokens.
    pub output_tokens: usize,
}

/// Session-wide token tracker.
#[derive(Debug, Default)]
pub struct TokenTracker {
    /// All usage records.
    pub records: Vec<TokenUsage>,
}

impl TokenTracker {
    /// Create a new tracker.
    pub fn new() -> Self {
        Self {
            records: Vec::new(),
        }
    }

    /// Record a usage event.
    pub fn record(
        &mut self,
        feature: &str,
        model: &str,
        input_tokens: usize,
        output_tokens: usize,
    ) {
        self.records.push(TokenUsage {
            feature: feature.to_string(),
            model: model.to_string(),
            input_tokens,
            output_tokens,
        });
    }

    /// Total input tokens this session.
    pub fn total_input(&self) -> usize {
        self.records.iter().map(|r| r.input_tokens).sum()
    }

    /// Total output tokens this session.
    pub fn total_output(&self) -> usize {
        self.records.iter().map(|r| r.output_tokens).sum()
    }

    /// Total tokens this session.
    pub fn total_tokens(&self) -> usize {
        self.total_input() + self.total_output()
    }

    /// Estimated cost in USD (rough approximation).
    pub fn estimated_cost(&self) -> f64 {
        let mut cost = 0.0;
        for r in &self.records {
            let (input_rate, output_rate) = cost_per_token(&r.model);
            cost += r.input_tokens as f64 * input_rate + r.output_tokens as f64 * output_rate;
        }
        cost
    }

    /// Number of requests.
    pub fn request_count(&self) -> usize {
        self.records.len()
    }

    /// Summary string for status bar.
    pub fn summary(&self) -> String {
        let total = self.total_tokens();
        let cost = self.estimated_cost();
        let reqs = self.request_count();
        if reqs == 0 {
            return "No AI requests yet".to_string();
        }
        format!(
            "{} requests | {}K tokens | ~${:.3}",
            reqs,
            total / 1000,
            cost
        )
    }
}

/// Cost per token (input, output) for known models.
fn cost_per_token(model: &str) -> (f64, f64) {
    if model.contains("haiku") {
        (0.25e-6, 1.25e-6)
    } else if model.contains("sonnet") {
        (3.0e-6, 15.0e-6)
    } else if model.contains("opus") {
        (15.0e-6, 75.0e-6)
    } else if model.contains("gpt-4o-mini") {
        (0.15e-6, 0.6e-6)
    } else if model.contains("gpt-4o") {
        (2.5e-6, 10.0e-6)
    } else if model.contains("gpt-4") {
        (30.0e-6, 60.0e-6)
    } else if model.contains("gpt-3.5") {
        (0.5e-6, 1.5e-6)
    } else {
        // Ollama/local = free.
        (0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_track_and_summarize() {
        let mut tracker = TokenTracker::new();
        tracker.record("chat", "claude-sonnet-4-20250514", 1000, 500);
        tracker.record("commit", "claude-haiku-4-5-20251001", 200, 100);
        assert_eq!(tracker.request_count(), 2);
        assert_eq!(tracker.total_input(), 1200);
        assert_eq!(tracker.total_output(), 600);
        assert!(tracker.estimated_cost() > 0.0);
        assert!(tracker.summary().contains("2 requests"));
    }
}
