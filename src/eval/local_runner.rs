//! Local runner for testing without Kubernetes
//! This simulates what would happen in a pod by running tests locally

use crate::agents::AgentConfig;
use crate::cli::{EvalConfig, PromptConfig, TestHarness};
use crate::eval::{EvalRunResult, EvaluationResults, RunStatus, TestCaseResult, TestSuiteResult};
use anyhow::Result;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Local evaluation runner (no Kubernetes required)
pub struct LocalEvalRunner {
    config: EvalConfig,
    results: Arc<Mutex<EvaluationResults>>,
}

impl LocalEvalRunner {
    pub fn new(config: EvalConfig) -> Self {
        let eval_id = Uuid::new_v4().to_string();
        let results = Arc::new(Mutex::new(EvaluationResults::new(&config.name, &eval_id)));

        Self { config, results }
    }

    /// Run evaluation locally (simulating what agents would do)
    pub async fn run_local_tests(&self) -> Result<EvaluationResults> {
        info!(
            "Starting LOCAL evaluation: {} (no Kubernetes)",
            self.config.name
        );

        let combinations = self.config.combinations();
        info!("Running {} combinations locally", combinations.len());

        for (prompt, agent) in combinations {
            let result = self.run_single_local(&prompt, &agent).await;
            let mut results = self.results.lock().await;
            results.add_run(result);
        }

        let mut final_results = self.results.lock().await;
        final_results.finalize();

        Ok(final_results.clone())
    }

    async fn run_single_local(&self, prompt: &PromptConfig, agent: &AgentConfig) -> EvalRunResult {
        let run_id = Uuid::new_v4().to_string();
        let agent_id = agent.id();

        info!(
            "[LOCAL] Running tests for prompt '{}' with agent '{}' (simulated)",
            prompt.id, agent_id
        );

        let mut result = EvalRunResult::new(
            &run_id,
            &prompt.id,
            &agent_id,
            &agent.tool.to_string(),
            &agent.model.to_string(),
        );
        result.status = RunStatus::Running;

        // Run the actual tests locally
        match self.run_local_test_harness(&prompt.eval_path, &prompt.test_harness) {
            Ok(test_results) => {
                result.complete_with_results(test_results);
                info!(
                    "[LOCAL] Tests completed for {} with score {:.2}%",
                    agent_id,
                    result.score.unwrap_or(0.0)
                );
            }
            Err(e) => {
                error!("[LOCAL] Tests failed for {}: {}", agent_id, e);
                result.fail_with_error(&format!("Test execution failed: {}", e));
            }
        }

        result
    }

    fn run_local_test_harness(
        &self,
        eval_path: &Path,
        harness: &TestHarness,
    ) -> Result<TestSuiteResult> {
        let (cmd, args) = harness.test_command();

        info!("[LOCAL] Running: {} {:?} in {:?}", cmd, args, eval_path);

        let output = Command::new(&cmd)
            .args(&args)
            .current_dir(eval_path)
            .output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined_output = format!("{}\n{}", stdout, stderr);

        info!("[LOCAL] Test output:\n{}", combined_output);

        // Parse the output
        parse_test_output(harness, &combined_output)
    }

    pub async fn results(&self) -> EvaluationResults {
        self.results.lock().await.clone()
    }

    pub async fn save_results(&self, output_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(output_dir)?;

        let results = self.results.lock().await;

        let json_path = output_dir.join(format!("{}.json", results.eval_id));
        results.save_json(&json_path)?;
        info!("Saved results to {:?}", json_path);

        let report_path = output_dir.join(format!("{}_report.md", results.eval_id));
        let report = results.generate_report();
        std::fs::write(&report_path, report)?;
        info!("Saved report to {:?}", report_path);

        Ok(())
    }
}

fn parse_test_output(harness: &TestHarness, output: &str) -> Result<TestSuiteResult> {
    match harness {
        TestHarness::Cargo { .. } => parse_cargo_test_output(output),
        TestHarness::Npm { .. } => parse_generic_test_output(output),
        TestHarness::Pytest { .. } => parse_pytest_output(output),
        TestHarness::Go { .. } => parse_go_test_output(output),
        TestHarness::Custom { .. } => parse_generic_test_output(output),
    }
}

fn parse_cargo_test_output(output: &str) -> Result<TestSuiteResult> {
    let mut tests = Vec::new();
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines() {
        if line.starts_with("test ") && (line.contains(" ... ok") || line.contains(" ... FAILED")) {
            total += 1;
            let test_passed = line.contains(" ... ok");
            if test_passed {
                passed += 1;
            } else {
                failed += 1;
            }

            let name = line
                .strip_prefix("test ")
                .and_then(|s| s.split(" ... ").next())
                .unwrap_or("unknown")
                .to_string();

            tests.push(TestCaseResult {
                name,
                passed: test_passed,
                duration_ms: None,
                error: None,
                stdout: None,
            });
        }
    }

    Ok(TestSuiteResult {
        total,
        passed,
        failed,
        skipped: 0,
        tests,
        duration_ms: 0,
        raw_output: output.to_string(),
    })
}

fn parse_pytest_output(output: &str) -> Result<TestSuiteResult> {
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines() {
        if line.contains("PASSED") {
            total += 1;
            passed += 1;
        } else if line.contains("FAILED") {
            total += 1;
            failed += 1;
        }
    }

    Ok(TestSuiteResult {
        total,
        passed,
        failed,
        skipped: 0,
        tests: vec![],
        duration_ms: 0,
        raw_output: output.to_string(),
    })
}

fn parse_go_test_output(output: &str) -> Result<TestSuiteResult> {
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines() {
        if line.starts_with("--- PASS:") {
            total += 1;
            passed += 1;
        } else if line.starts_with("--- FAIL:") {
            total += 1;
            failed += 1;
        }
    }

    Ok(TestSuiteResult {
        total,
        passed,
        failed,
        skipped: 0,
        tests: vec![],
        duration_ms: 0,
        raw_output: output.to_string(),
    })
}

fn parse_generic_test_output(output: &str) -> Result<TestSuiteResult> {
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.contains("passed") && line_lower.contains("failed") {
            let parts: Vec<&str> = line.split(|c: char| !c.is_numeric()).collect();
            let nums: Vec<u32> = parts.iter().filter_map(|s| s.parse().ok()).collect();
            if nums.len() >= 2 {
                passed = nums[0];
                failed = nums[1];
                total = passed + failed;
                break;
            }
        }
    }

    Ok(TestSuiteResult {
        total,
        passed,
        failed,
        skipped: 0,
        tests: vec![],
        duration_ms: 0,
        raw_output: output.to_string(),
    })
}
