use crate::agents::AgentConfig;
use crate::cli::{EvalConfig, PromptConfig, TestHarness};
use crate::eval::{EvalRunResult, EvaluationResults, RunStatus, TestCaseResult, TestSuiteResult};
use crate::kubernetes::{AgentPodConfig, PodManager, PodStatus};
use anyhow::Result;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Orchestrates the evaluation runs
pub struct EvalRunner {
    pod_manager: Arc<PodManager>,
    config: EvalConfig,
    api_keys: BTreeMap<String, String>,
    results: Arc<Mutex<EvaluationResults>>,
    namespace: String,
}

impl EvalRunner {
    /// Create a new EvalRunner
    pub async fn new(config: EvalConfig, namespace: &str) -> Result<Self> {
        let pod_manager = Arc::new(PodManager::new(namespace).await?);
        let api_keys = config.settings.api_keys.resolve()?;
        let eval_id = Uuid::new_v4().to_string();
        let results = Arc::new(Mutex::new(EvaluationResults::new(&config.name, &eval_id)));

        Ok(Self {
            pod_manager,
            config,
            api_keys,
            results,
            namespace: namespace.to_string(),
        })
    }

    /// Run the evaluation
    pub async fn run(&self, parallelism: u32, timeout_hours: u32) -> Result<EvaluationResults> {
        let eval_id = {
            let results = self.results.lock().await;
            results.eval_id.clone()
        };

        info!(
            "Starting evaluation: {} (ID: {})",
            self.config.name, eval_id
        );

        let combinations = self.config.combinations();
        info!(
            "Running {} combinations with parallelism {}",
            combinations.len(),
            parallelism
        );

        // Process combinations with a semaphore for parallelism
        let semaphore = Arc::new(tokio::sync::Semaphore::new(parallelism as usize));
        let mut handles = Vec::new();

        for (prompt, agent) in combinations {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let pod_manager = Arc::clone(&self.pod_manager);
            let api_keys = self.api_keys.clone();
            let results = Arc::clone(&self.results);
            let namespace = self.namespace.clone();
            let default_timeout = self.config.settings.default_timeout_hours;
            let cleanup_on_complete = self.config.settings.cleanup_on_complete;

            let handle = tokio::spawn(async move {
                let result = run_single_eval(
                    &pod_manager,
                    &prompt,
                    &agent,
                    &api_keys,
                    &namespace,
                    timeout_hours,
                    default_timeout,
                    cleanup_on_complete,
                )
                .await;

                // Add result to the results collection
                {
                    let mut results_guard = results.lock().await;
                    results_guard.add_run(result);
                }

                drop(permit);
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await?;
        }

        // Finalize and return results
        let mut final_results = self.results.lock().await;
        final_results.finalize();

        Ok(final_results.clone())
    }

    /// Get the current results
    pub async fn results(&self) -> EvaluationResults {
        self.results.lock().await.clone()
    }

    /// Save results to the output directory
    pub async fn save_results(&self, output_dir: &Path) -> Result<()> {
        std::fs::create_dir_all(output_dir)?;

        let results = self.results.lock().await;

        // Save JSON results
        let json_path = output_dir.join(format!("{}.json", results.eval_id));
        results.save_json(&json_path)?;
        info!("Saved results to {:?}", json_path);

        // Save markdown report
        let report_path = output_dir.join(format!("{}_report.md", results.eval_id));
        let report = results.generate_report();
        std::fs::write(&report_path, report)?;
        info!("Saved report to {:?}", report_path);

        Ok(())
    }
}

/// Run a single (prompt, agent) combination
async fn run_single_eval(
    pod_manager: &PodManager,
    prompt: &PromptConfig,
    agent: &AgentConfig,
    api_keys: &BTreeMap<String, String>,
    namespace: &str,
    timeout_hours: u32,
    default_timeout: u32,
    cleanup_on_complete: bool,
) -> EvalRunResult {
    let run_id = Uuid::new_v4().to_string();
    let agent_id = agent.id();

    info!(
        "Starting run {} for prompt '{}' with agent '{}'",
        run_id, prompt.id, agent_id
    );

    let mut result = EvalRunResult::new(
        &run_id,
        &prompt.id,
        &agent_id,
        &agent.tool.to_string(),
        &agent.model.to_string(),
    );
    result.status = RunStatus::Running;

    let timeout = prompt.timeout_hours.unwrap_or(default_timeout).min(timeout_hours);

    // Get test command from harness
    let (test_cmd, test_args) = prompt.test_harness.test_command();

    // Create pod configuration
    let pod_config = AgentPodConfig {
        agent: agent.clone(),
        prompt: prompt.prompt.clone(),
        eval_path: prompt.eval_path.to_string_lossy().to_string(),
        run_id: run_id.clone(),
        namespace: namespace.to_string(),
        timeout_hours: timeout,
        api_keys: api_keys.clone(),
        test_command: test_cmd,
        test_args,
        git_repo: None, // TODO: Add git_repo support to PromptConfig
        setup_commands: prompt.setup_commands.clone(),
    };

    // Spawn the pod
    let pod_name = match pod_manager.spawn_pod(&pod_config).await {
        Ok(name) => name,
        Err(e) => {
            error!("Failed to spawn pod for run {}: {}", run_id, e);
            result.fail_with_error(&format!("Failed to spawn pod: {}", e));
            return result;
        }
    };

    // Wait for agent to complete
    let check_interval = Duration::from_secs(30);
    let max_duration = Duration::from_secs((timeout * 3600) as u64);

    let pod_status = match pod_manager
        .wait_for_completion(&pod_name, check_interval, max_duration)
        .await
    {
        Ok(status) => status,
        Err(e) => {
            error!("Error waiting for pod {}: {}", pod_name, e);
            result.fail_with_error(&format!("Error waiting for pod: {}", e));
            // Try to get logs anyway
            if let Ok(logs) = pod_manager.get_pod_logs(&pod_name).await {
                result.agent_logs = Some(logs);
            }
            return result;
        }
    };

    // Get agent logs
    if let Ok(logs) = pod_manager.get_pod_logs(&pod_name).await {
        result.agent_logs = Some(logs);
    }

    match pod_status {
        PodStatus::Succeeded => {
            info!("Pod completed for run {}, parsing test results from logs", run_id);

            // Get logs which contain test output
            if let Ok(logs) = pod_manager.get_pod_logs(&pod_name).await {
                result.agent_logs = Some(logs.clone());

                // Extract test output from logs (between TEST_OUTPUT_START and TEST_OUTPUT_END)
                if let Some(test_output) = extract_test_output(&logs) {
                    match parse_test_output(&prompt.test_harness, &test_output) {
                        Ok(test_results) => {
                            result.complete_with_results(test_results);
                            info!(
                                "Run {} completed with score {:.2}%",
                                run_id,
                                result.score.unwrap_or(0.0)
                            );
                        }
                        Err(e) => {
                            error!("Failed to parse test results for run {}: {}", run_id, e);
                            result.fail_with_error(&format!("Failed to parse test results: {}", e));
                        }
                    }
                } else {
                    warn!("No test output found in logs for run {}", run_id);
                    result.fail_with_error("No test output found in pod logs");
                }
            } else {
                error!("Failed to get logs for run {}", run_id);
                result.fail_with_error("Failed to retrieve pod logs");
            }
        }
        PodStatus::Failed(reason) => {
            error!("Agent failed for run {}: {}", run_id, reason);
            result.fail_with_error(&reason);
        }
        _ => {
            warn!("Unexpected pod status for run {}: {:?}", run_id, pod_status);
            result.fail_with_error(&format!("Unexpected pod status: {:?}", pod_status));
        }
    }

    // Cleanup pod if configured
    if cleanup_on_complete {
        if let Err(e) = pod_manager.delete_pod(&pod_name).await {
            warn!("Failed to cleanup pod {}: {}", pod_name, e);
        }
    }

    result
}

/// Extract test output from pod logs (between TEST_OUTPUT_START and TEST_OUTPUT_END markers)
fn extract_test_output(logs: &str) -> Option<String> {
    let start_marker = "TEST_OUTPUT_START";
    let end_marker = "TEST_OUTPUT_END";

    if let Some(start_idx) = logs.find(start_marker) {
        let after_start = &logs[start_idx + start_marker.len()..];
        if let Some(end_idx) = after_start.find(end_marker) {
            return Some(after_start[..end_idx].trim().to_string());
        }
    }
    None
}

/// Parse test output based on the harness type
fn parse_test_output(harness: &TestHarness, output: &str) -> Result<TestSuiteResult> {
    match harness {
        TestHarness::Cargo { .. } => parse_cargo_test_output(output),
        TestHarness::Npm { .. } => parse_generic_test_output(output),
        TestHarness::Pytest { .. } => parse_pytest_output(output),
        TestHarness::Go { .. } => parse_go_test_output(output),
        TestHarness::Custom { .. } => parse_generic_test_output(output),
    }
}

/// Parse cargo test JSON output
fn parse_cargo_test_output(output: &str) -> Result<TestSuiteResult> {
    let mut tests = Vec::new();
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    // Parse each line as JSON
    for line in output.lines() {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            if let Some(event_type) = json.get("type").and_then(|v| v.as_str()) {
                if event_type == "test" {
                    if let Some(event) = json.get("event").and_then(|v| v.as_str()) {
                        if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                            total += 1;
                            let test_passed = event == "ok";
                            if test_passed {
                                passed += 1;
                            } else {
                                failed += 1;
                            }

                            tests.push(TestCaseResult {
                                name: name.to_string(),
                                passed: test_passed,
                                duration_ms: json
                                    .get("exec_time")
                                    .and_then(|v| v.as_f64())
                                    .map(|t| (t * 1000.0) as u64),
                                error: if !test_passed {
                                    json.get("stdout").and_then(|v| v.as_str()).map(String::from)
                                } else {
                                    None
                                },
                                stdout: json.get("stdout").and_then(|v| v.as_str()).map(String::from),
                            });
                        }
                    }
                }
            }
        }
    }

    // If JSON parsing didn't work, try plain text parsing
    if tests.is_empty() {
        return parse_cargo_test_plain(output);
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

/// Parse cargo test plain text output
fn parse_cargo_test_plain(output: &str) -> Result<TestSuiteResult> {
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

/// Parse pytest output
fn parse_pytest_output(output: &str) -> Result<TestSuiteResult> {
    let mut tests = Vec::new();
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;
    let mut skipped = 0;

    for line in output.lines() {
        if line.contains("PASSED") {
            total += 1;
            passed += 1;
            let name = line.split("::").last().unwrap_or("unknown").trim();
            let name = name.split_whitespace().next().unwrap_or(name);
            tests.push(TestCaseResult {
                name: name.to_string(),
                passed: true,
                duration_ms: None,
                error: None,
                stdout: None,
            });
        } else if line.contains("FAILED") {
            total += 1;
            failed += 1;
            let name = line.split("::").last().unwrap_or("unknown").trim();
            let name = name.split_whitespace().next().unwrap_or(name);
            tests.push(TestCaseResult {
                name: name.to_string(),
                passed: false,
                duration_ms: None,
                error: Some("Test failed".to_string()),
                stdout: None,
            });
        } else if line.contains("SKIPPED") {
            total += 1;
            skipped += 1;
        }
    }

    Ok(TestSuiteResult {
        total,
        passed,
        failed,
        skipped,
        tests,
        duration_ms: 0,
        raw_output: output.to_string(),
    })
}

/// Parse go test output
fn parse_go_test_output(output: &str) -> Result<TestSuiteResult> {
    let mut tests = Vec::new();
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    for line in output.lines() {
        if line.starts_with("--- PASS:") {
            total += 1;
            passed += 1;
            let name = line
                .strip_prefix("--- PASS: ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("unknown")
                .to_string();
            tests.push(TestCaseResult {
                name,
                passed: true,
                duration_ms: None,
                error: None,
                stdout: None,
            });
        } else if line.starts_with("--- FAIL:") {
            total += 1;
            failed += 1;
            let name = line
                .strip_prefix("--- FAIL: ")
                .and_then(|s| s.split_whitespace().next())
                .unwrap_or("unknown")
                .to_string();
            tests.push(TestCaseResult {
                name,
                passed: false,
                duration_ms: None,
                error: Some("Test failed".to_string()),
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

/// Generic test output parser (for custom harnesses)
fn parse_generic_test_output(output: &str) -> Result<TestSuiteResult> {
    // Try to extract test counts from common patterns
    let mut total = 0;
    let mut passed = 0;
    let mut failed = 0;

    // Look for common summary patterns
    for line in output.lines() {
        let line_lower = line.to_lowercase();

        // Pattern: "X passed, Y failed"
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

        // Pattern: "Tests: X passed, Y failed, Z total"
        if line_lower.contains("tests:") {
            let parts: Vec<&str> = line.split(|c: char| !c.is_numeric()).collect();
            let nums: Vec<u32> = parts.iter().filter_map(|s| s.parse().ok()).collect();

            if !nums.is_empty() {
                for (i, num) in nums.iter().enumerate() {
                    if i == 0 {
                        passed = *num;
                    } else if i == 1 {
                        failed = *num;
                    } else if i == 2 {
                        total = *num;
                    }
                }
                if total == 0 {
                    total = passed + failed;
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cargo_test_plain() {
        let output = r#"
running 3 tests
test tests::test_one ... ok
test tests::test_two ... ok
test tests::test_three ... FAILED

failures:

failures:
    tests::test_three

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out
"#;

        let result = parse_cargo_test_plain(output).unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn test_parse_pytest_output() {
        let output = r#"
test_example.py::test_one PASSED
test_example.py::test_two PASSED
test_example.py::test_three FAILED
"#;

        let result = parse_pytest_output(output).unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
    }

    #[test]
    fn test_parse_go_test_output() {
        let output = r#"
=== RUN   TestOne
--- PASS: TestOne (0.00s)
=== RUN   TestTwo
--- PASS: TestTwo (0.00s)
=== RUN   TestThree
--- FAIL: TestThree (0.00s)
"#;

        let result = parse_go_test_output(output).unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(result.passed, 2);
        assert_eq!(result.failed, 1);
    }
}
