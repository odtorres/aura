//! Agent execution plan — structured task planning with step tracking.
//!
//! When the agent runs in planning mode (`:agent plan <task>`), the AI first
//! produces a numbered plan that the user can approve before execution begins.
//! Each step's status is tracked as the agent works through the plan.

/// Status of a plan step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanStepStatus {
    /// Not yet started.
    Pending,
    /// Currently being executed.
    InProgress,
    /// Successfully completed.
    Completed,
    /// Skipped by the agent.
    Skipped,
    /// Failed with an error message.
    Failed(String),
}

/// A single step in an agent's execution plan.
#[derive(Debug, Clone)]
pub struct PlanStep {
    /// Step number (1-based).
    pub index: usize,
    /// Description of what this step does.
    pub description: String,
    /// Which tool this step is likely to use (hint for UI).
    pub tool_hint: Option<String>,
    /// Current status.
    pub status: PlanStepStatus,
}

/// An agent execution plan parsed from AI output.
#[derive(Debug, Clone)]
pub struct AgentPlan {
    /// The original task description.
    pub task: String,
    /// Ordered steps to execute.
    pub steps: Vec<PlanStep>,
    /// Whether the user has approved this plan.
    pub approved: bool,
    /// When the plan was created.
    pub created_at: std::time::Instant,
}

impl AgentPlan {
    /// Create a new unapproved plan.
    pub fn new(task: &str, steps: Vec<PlanStep>) -> Self {
        Self {
            task: task.to_string(),
            steps,
            approved: false,
            created_at: std::time::Instant::now(),
        }
    }

    /// Mark a step as in-progress.
    pub fn mark_step_started(&mut self, index: usize) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.index == index) {
            step.status = PlanStepStatus::InProgress;
        }
    }

    /// Mark a step as completed.
    pub fn mark_step_completed(&mut self, index: usize) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.index == index) {
            step.status = PlanStepStatus::Completed;
        }
    }

    /// Mark a step as failed.
    pub fn mark_step_failed(&mut self, index: usize, reason: &str) {
        if let Some(step) = self.steps.iter_mut().find(|s| s.index == index) {
            step.status = PlanStepStatus::Failed(reason.to_string());
        }
    }

    /// Return the next pending step.
    pub fn next_pending_step(&self) -> Option<&PlanStep> {
        self.steps
            .iter()
            .find(|s| s.status == PlanStepStatus::Pending)
    }

    /// Count of completed steps.
    pub fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == PlanStepStatus::Completed)
            .count()
    }
}

/// Parse a structured plan from AI response text.
///
/// Looks for numbered lists like:
/// ```text
/// 1. Analyze the codebase structure
/// 2. Add error handling to api.rs
/// 3. Run tests to verify changes
/// ```
pub fn parse_plan_from_response(text: &str, task: &str) -> Option<AgentPlan> {
    let mut steps = Vec::new();
    let mut step_index = 1usize;

    for line in text.lines() {
        let trimmed = line.trim();
        // Match "1. description", "1) description", "- 1. description" patterns.
        let desc = try_parse_numbered_line(trimmed, step_index);
        if let Some(description) = desc {
            steps.push(PlanStep {
                index: step_index,
                description,
                tool_hint: None,
                status: PlanStepStatus::Pending,
            });
            step_index += 1;
        }
    }

    if steps.len() >= 2 {
        Some(AgentPlan::new(task, steps))
    } else {
        None
    }
}

/// Try to parse a numbered line like "N. text" or "N) text".
fn try_parse_numbered_line(line: &str, _expected: usize) -> Option<String> {
    // Strip optional leading "- " or "* ".
    let line = line
        .strip_prefix("- ")
        .or_else(|| line.strip_prefix("* "))
        .unwrap_or(line);

    // Match "N. " or "N) " pattern.
    let mut chars = line.chars().peekable();
    let mut num_str = String::new();

    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            num_str.push(c);
            chars.next();
        } else {
            break;
        }
    }

    if num_str.is_empty() {
        return None;
    }

    // Expect ". " or ") " after the number.
    match chars.next() {
        Some('.') | Some(')') => {}
        _ => return None,
    }

    // Skip whitespace.
    let rest: String = chars.collect();
    let rest = rest.trim();

    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plan_basic() {
        let text = "Here's my plan:\n\
                     1. Read the source files\n\
                     2. Add error handling\n\
                     3. Run the tests\n";
        let plan = parse_plan_from_response(text, "add error handling").unwrap();
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].description, "Read the source files");
        assert_eq!(plan.steps[1].description, "Add error handling");
        assert_eq!(plan.steps[2].description, "Run the tests");
        assert!(!plan.approved);
    }

    #[test]
    fn test_parse_plan_too_few_steps() {
        let text = "1. Just one step";
        assert!(parse_plan_from_response(text, "task").is_none());
    }

    #[test]
    fn test_step_lifecycle() {
        let mut plan = AgentPlan::new(
            "test",
            vec![
                PlanStep {
                    index: 1,
                    description: "Step 1".into(),
                    tool_hint: None,
                    status: PlanStepStatus::Pending,
                },
                PlanStep {
                    index: 2,
                    description: "Step 2".into(),
                    tool_hint: None,
                    status: PlanStepStatus::Pending,
                },
            ],
        );

        assert_eq!(plan.next_pending_step().unwrap().index, 1);
        plan.mark_step_started(1);
        plan.mark_step_completed(1);
        assert_eq!(plan.completed_count(), 1);
        assert_eq!(plan.next_pending_step().unwrap().index, 2);
    }
}
