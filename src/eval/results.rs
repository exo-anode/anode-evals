use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Result of a single test case
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestCaseResult {
    /// Name of the test
    pub name: String,
    /// Whether the test passed
    pub passed: bool,
    /// Duration in milliseconds
    pub duration_ms: Option<u64>,
    /// Error message if failed
    pub error: Option<String>,
    /// stdout output
    pub stdout: Option<String>,
}

/// Result of running the eval test suite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestSuiteResult {
    /// Total number of tests
    pub total: u32,
    /// Number of passed tests
    pub passed: u32,
    /// Number of failed tests
    pub failed: u32,
    /// Number of skipped tests
    pub skipped: u32,
    /// Individual test results
    pub tests: Vec<TestCaseResult>,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Raw output from test runner
    pub raw_output: String,
}

impl TestSuiteResult {
    /// Calculate the pass rate as a percentage
    pub fn pass_rate(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            (self.passed as f64 / self.total as f64) * 100.0
        }
    }
}

/// Result of a single (prompt, agent) evaluation run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRunResult {
    /// Unique identifier for this run
    pub run_id: String,
    /// Prompt ID
    pub prompt_id: String,
    /// Agent configuration identifier
    pub agent_id: String,
    /// Agent tool name
    pub agent_tool: String,
    /// Model version
    pub model: String,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// End time
    pub completed_at: Option<DateTime<Utc>>,
    /// Duration in seconds
    pub duration_seconds: Option<u64>,
    /// Status of the run
    pub status: RunStatus,
    /// Test results if completed
    pub test_results: Option<TestSuiteResult>,
    /// Score (percentage of tests passed)
    pub score: Option<f64>,
    /// Agent logs
    pub agent_logs: Option<String>,
    /// Error message if failed
    pub error: Option<String>,
}

impl EvalRunResult {
    pub fn new(run_id: &str, prompt_id: &str, agent_id: &str, agent_tool: &str, model: &str) -> Self {
        Self {
            run_id: run_id.to_string(),
            prompt_id: prompt_id.to_string(),
            agent_id: agent_id.to_string(),
            agent_tool: agent_tool.to_string(),
            model: model.to_string(),
            started_at: Utc::now(),
            completed_at: None,
            duration_seconds: None,
            status: RunStatus::Pending,
            test_results: None,
            score: None,
            agent_logs: None,
            error: None,
        }
    }

    pub fn complete_with_results(&mut self, test_results: TestSuiteResult) {
        self.completed_at = Some(Utc::now());
        self.duration_seconds = Some(
            (self.completed_at.unwrap() - self.started_at)
                .num_seconds()
                .max(0) as u64,
        );
        self.score = Some(test_results.pass_rate());
        self.test_results = Some(test_results);
        self.status = RunStatus::Completed;
    }

    pub fn fail_with_error(&mut self, error: &str) {
        self.completed_at = Some(Utc::now());
        self.duration_seconds = Some(
            (self.completed_at.unwrap() - self.started_at)
                .num_seconds()
                .max(0) as u64,
        );
        self.error = Some(error.to_string());
        self.status = RunStatus::Failed;
        self.score = Some(0.0);
    }
}

/// Status of a run
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Timeout,
    Cancelled,
}

/// Aggregated results for an agent across all prompts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentScore {
    /// Agent identifier
    pub agent_id: String,
    /// Agent tool name
    pub agent_tool: String,
    /// Model version
    pub model: String,
    /// Total runs
    pub total_runs: u32,
    /// Completed runs
    pub completed_runs: u32,
    /// Failed runs
    pub failed_runs: u32,
    /// Total tests across all runs
    pub total_tests: u32,
    /// Total passed tests
    pub passed_tests: u32,
    /// Average score (pass rate)
    pub average_score: f64,
    /// Rank among all agents
    pub rank: u32,
    /// Individual run results
    pub runs: Vec<String>, // run_ids
}

/// Complete evaluation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationResults {
    /// Evaluation name
    pub name: String,
    /// Unique evaluation ID
    pub eval_id: String,
    /// Start time
    pub started_at: DateTime<Utc>,
    /// End time
    pub completed_at: Option<DateTime<Utc>>,
    /// Individual run results
    pub runs: Vec<EvalRunResult>,
    /// Agent scores and rankings
    pub agent_scores: Vec<AgentScore>,
    /// Summary statistics
    pub summary: EvalSummary,
}

/// Summary statistics for the evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSummary {
    /// Total number of (prompt, agent) combinations
    pub total_combinations: u32,
    /// Completed combinations
    pub completed: u32,
    /// Failed combinations
    pub failed: u32,
    /// Timed out combinations
    pub timed_out: u32,
    /// Total tests run
    pub total_tests: u32,
    /// Total passed tests
    pub passed_tests: u32,
    /// Overall pass rate
    pub overall_pass_rate: f64,
    /// Best performing agent
    pub best_agent: Option<String>,
    /// Worst performing agent
    pub worst_agent: Option<String>,
}

impl EvaluationResults {
    pub fn new(name: &str, eval_id: &str) -> Self {
        Self {
            name: name.to_string(),
            eval_id: eval_id.to_string(),
            started_at: Utc::now(),
            completed_at: None,
            runs: Vec::new(),
            agent_scores: Vec::new(),
            summary: EvalSummary {
                total_combinations: 0,
                completed: 0,
                failed: 0,
                timed_out: 0,
                total_tests: 0,
                passed_tests: 0,
                overall_pass_rate: 0.0,
                best_agent: None,
                worst_agent: None,
            },
        }
    }

    /// Add a run result
    pub fn add_run(&mut self, result: EvalRunResult) {
        self.runs.push(result);
    }

    /// Calculate agent scores and rankings
    pub fn calculate_scores(&mut self) {
        let mut agent_map: BTreeMap<String, AgentScore> = BTreeMap::new();

        for run in &self.runs {
            let entry = agent_map.entry(run.agent_id.clone()).or_insert(AgentScore {
                agent_id: run.agent_id.clone(),
                agent_tool: run.agent_tool.clone(),
                model: run.model.clone(),
                total_runs: 0,
                completed_runs: 0,
                failed_runs: 0,
                total_tests: 0,
                passed_tests: 0,
                average_score: 0.0,
                rank: 0,
                runs: Vec::new(),
            });

            entry.total_runs += 1;
            entry.runs.push(run.run_id.clone());

            match run.status {
                RunStatus::Completed => {
                    entry.completed_runs += 1;
                    if let Some(ref test_results) = run.test_results {
                        entry.total_tests += test_results.total;
                        entry.passed_tests += test_results.passed;
                    }
                }
                RunStatus::Failed | RunStatus::Timeout => {
                    entry.failed_runs += 1;
                }
                _ => {}
            }
        }

        // Calculate average scores
        for score in agent_map.values_mut() {
            if score.total_tests > 0 {
                score.average_score = (score.passed_tests as f64 / score.total_tests as f64) * 100.0;
            }
        }

        // Sort by average score and assign ranks
        let mut scores: Vec<AgentScore> = agent_map.into_values().collect();
        scores.sort_by(|a, b| b.average_score.partial_cmp(&a.average_score).unwrap());

        for (i, score) in scores.iter_mut().enumerate() {
            score.rank = (i + 1) as u32;
        }

        // Update summary
        self.summary.total_combinations = self.runs.len() as u32;
        self.summary.completed = self
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Completed)
            .count() as u32;
        self.summary.failed = self
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Failed)
            .count() as u32;
        self.summary.timed_out = self
            .runs
            .iter()
            .filter(|r| r.status == RunStatus::Timeout)
            .count() as u32;
        self.summary.total_tests = scores.iter().map(|s| s.total_tests).sum();
        self.summary.passed_tests = scores.iter().map(|s| s.passed_tests).sum();

        if self.summary.total_tests > 0 {
            self.summary.overall_pass_rate =
                (self.summary.passed_tests as f64 / self.summary.total_tests as f64) * 100.0;
        }

        self.summary.best_agent = scores.first().map(|s| s.agent_id.clone());
        self.summary.worst_agent = scores.last().map(|s| s.agent_id.clone());

        self.agent_scores = scores;
    }

    /// Finalize the results
    pub fn finalize(&mut self) {
        self.completed_at = Some(Utc::now());
        self.calculate_scores();
    }

    /// Save results to a JSON file
    pub fn save_json(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Generate a human-readable report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str(&format!("# Evaluation Report: {}\n\n", self.name));
        report.push_str(&format!("Evaluation ID: {}\n", self.eval_id));
        report.push_str(&format!("Started: {}\n", self.started_at));
        if let Some(completed) = self.completed_at {
            report.push_str(&format!("Completed: {}\n", completed));
        }
        report.push_str("\n");

        report.push_str("## Summary\n\n");
        report.push_str(&format!(
            "- Total Combinations: {}\n",
            self.summary.total_combinations
        ));
        report.push_str(&format!("- Completed: {}\n", self.summary.completed));
        report.push_str(&format!("- Failed: {}\n", self.summary.failed));
        report.push_str(&format!("- Timed Out: {}\n", self.summary.timed_out));
        report.push_str(&format!("- Total Tests: {}\n", self.summary.total_tests));
        report.push_str(&format!("- Passed Tests: {}\n", self.summary.passed_tests));
        report.push_str(&format!(
            "- Overall Pass Rate: {:.2}%\n",
            self.summary.overall_pass_rate
        ));
        report.push_str("\n");

        report.push_str("## Agent Rankings\n\n");
        report.push_str("| Rank | Agent | Model | Score | Tests Passed | Runs |\n");
        report.push_str("|------|-------|-------|-------|--------------|------|\n");

        for score in &self.agent_scores {
            report.push_str(&format!(
                "| {} | {} | {} | {:.2}% | {}/{} | {}/{} |\n",
                score.rank,
                score.agent_tool,
                score.model,
                score.average_score,
                score.passed_tests,
                score.total_tests,
                score.completed_runs,
                score.total_runs
            ));
        }

        report.push_str("\n## Individual Run Results\n\n");

        for run in &self.runs {
            report.push_str(&format!("### {} - {}\n", run.prompt_id, run.agent_id));
            report.push_str(&format!("- Status: {:?}\n", run.status));
            if let Some(score) = run.score {
                report.push_str(&format!("- Score: {:.2}%\n", score));
            }
            if let Some(ref test_results) = run.test_results {
                report.push_str(&format!(
                    "- Tests: {}/{} passed\n",
                    test_results.passed, test_results.total
                ));
            }
            if let Some(ref error) = run.error {
                report.push_str(&format!("- Error: {}\n", error));
            }
            report.push_str("\n");
        }

        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pass_rate() {
        let result = TestSuiteResult {
            total: 10,
            passed: 8,
            failed: 2,
            skipped: 0,
            tests: vec![],
            duration_ms: 1000,
            raw_output: String::new(),
        };

        assert_eq!(result.pass_rate(), 80.0);
    }

    #[test]
    fn test_eval_run_result_complete() {
        let mut run = EvalRunResult::new("run-1", "prompt-1", "agent-1", "claude-code", "opus-4.5");

        let test_results = TestSuiteResult {
            total: 5,
            passed: 4,
            failed: 1,
            skipped: 0,
            tests: vec![],
            duration_ms: 500,
            raw_output: String::new(),
        };

        run.complete_with_results(test_results);

        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.score, Some(80.0));
    }

    #[test]
    fn test_evaluation_results_scoring() {
        let mut results = EvaluationResults::new("Test Eval", "eval-1");

        let mut run1 = EvalRunResult::new(
            "run-1",
            "prompt-1",
            "agent-1",
            "claude-code",
            "opus-4.5",
        );
        run1.complete_with_results(TestSuiteResult {
            total: 10,
            passed: 8,
            failed: 2,
            skipped: 0,
            tests: vec![],
            duration_ms: 1000,
            raw_output: String::new(),
        });

        let mut run2 = EvalRunResult::new(
            "run-2",
            "prompt-1",
            "agent-2",
            "codex",
            "gpt-5.2-xhigh",
        );
        run2.complete_with_results(TestSuiteResult {
            total: 10,
            passed: 6,
            failed: 4,
            skipped: 0,
            tests: vec![],
            duration_ms: 1000,
            raw_output: String::new(),
        });

        results.add_run(run1);
        results.add_run(run2);
        results.finalize();

        assert_eq!(results.agent_scores.len(), 2);
        assert_eq!(results.agent_scores[0].rank, 1);
        assert_eq!(results.agent_scores[0].agent_id, "agent-1");
        assert_eq!(results.summary.best_agent, Some("agent-1".to_string()));
    }
}
