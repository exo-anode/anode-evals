//! Shared state for the web UI

use crate::eval::{EvaluationResults, RunStatus};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Session status for live monitoring
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    /// Session is queued but not yet started
    Queued,
    /// Session is actively running
    Running,
    /// Session completed successfully
    Completed,
    /// Session failed with an error
    Failed,
    /// Session was cancelled
    Cancelled,
}

impl From<RunStatus> for SessionStatus {
    fn from(status: RunStatus) -> Self {
        match status {
            RunStatus::Pending => SessionStatus::Queued,
            RunStatus::Running => SessionStatus::Running,
            RunStatus::Completed => SessionStatus::Completed,
            RunStatus::Failed | RunStatus::Timeout => SessionStatus::Failed,
            RunStatus::Cancelled => SessionStatus::Cancelled,
        }
    }
}

/// Information about a single evaluation session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Unique session ID
    pub session_id: String,
    /// Evaluation ID this session belongs to
    pub eval_id: String,
    /// Evaluation name
    pub eval_name: String,
    /// Prompt ID being evaluated
    pub prompt_id: String,
    /// Agent tool (e.g., claude_code, codex)
    pub agent_tool: String,
    /// Model version
    pub model: String,
    /// Current status
    pub status: SessionStatus,
    /// When the session started
    pub started_at: DateTime<Utc>,
    /// When the session completed (if finished)
    pub completed_at: Option<DateTime<Utc>>,
    /// Current progress message
    pub progress_message: String,
    /// Tests passed so far
    pub tests_passed: u32,
    /// Total tests
    pub tests_total: u32,
    /// Recent log lines (last 100)
    pub recent_logs: Vec<String>,
    /// Full logs path
    pub logs_path: Option<PathBuf>,
    /// Error message if failed
    pub error: Option<String>,
}

impl SessionInfo {
    pub fn new(
        session_id: &str,
        eval_id: &str,
        eval_name: &str,
        prompt_id: &str,
        agent_tool: &str,
        model: &str,
    ) -> Self {
        Self {
            session_id: session_id.to_string(),
            eval_id: eval_id.to_string(),
            eval_name: eval_name.to_string(),
            prompt_id: prompt_id.to_string(),
            agent_tool: agent_tool.to_string(),
            model: model.to_string(),
            status: SessionStatus::Queued,
            started_at: Utc::now(),
            completed_at: None,
            progress_message: "Waiting to start...".to_string(),
            tests_passed: 0,
            tests_total: 0,
            recent_logs: Vec::new(),
            logs_path: None,
            error: None,
        }
    }

    pub fn add_log(&mut self, line: &str) {
        self.recent_logs.push(line.to_string());
        // Keep only last 100 lines
        if self.recent_logs.len() > 100 {
            self.recent_logs.remove(0);
        }
    }

    pub fn set_running(&mut self) {
        self.status = SessionStatus::Running;
        self.progress_message = "Agent is working...".to_string();
    }

    pub fn set_completed(&mut self, tests_passed: u32, tests_total: u32) {
        self.status = SessionStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.tests_passed = tests_passed;
        self.tests_total = tests_total;
        self.progress_message = format!("Completed: {}/{} tests passed", tests_passed, tests_total);
    }

    pub fn set_failed(&mut self, error: &str) {
        self.status = SessionStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error.to_string());
        self.progress_message = format!("Failed: {}", error);
    }

    pub fn duration_seconds(&self) -> i64 {
        let end = self.completed_at.unwrap_or_else(Utc::now);
        (end - self.started_at).num_seconds()
    }
}

/// Stored evaluation result for the results dashboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvalResult {
    /// Path to the results JSON file
    pub path: PathBuf,
    /// The evaluation results
    pub results: EvaluationResults,
    /// When the results were loaded
    pub loaded_at: DateTime<Utc>,
}

/// Application state shared across all handlers
#[derive(Debug, Clone)]
pub struct AppState {
    /// Active sessions being monitored
    pub sessions: Arc<RwLock<HashMap<String, SessionInfo>>>,
    /// Cached evaluation results for the dashboard
    pub results: Arc<RwLock<Vec<StoredEvalResult>>>,
    /// Directory to scan for results
    pub results_dir: PathBuf,
}

impl AppState {
    pub fn new(results_dir: PathBuf) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            results: Arc::new(RwLock::new(Vec::new())),
            results_dir,
        }
    }

    /// Add or update a session
    pub async fn upsert_session(&self, session: SessionInfo) {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.session_id.clone(), session);
    }

    /// Get all sessions
    pub async fn get_sessions(&self) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.values().cloned().collect()
    }

    /// Get sessions for a specific eval
    pub async fn get_sessions_for_eval(&self, eval_id: &str) -> Vec<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions
            .values()
            .filter(|s| s.eval_id == eval_id)
            .cloned()
            .collect()
    }

    /// Get a specific session
    pub async fn get_session(&self, session_id: &str) -> Option<SessionInfo> {
        let sessions = self.sessions.read().await;
        sessions.get(session_id).cloned()
    }

    /// Load results from disk
    pub async fn load_results(&self) -> anyhow::Result<()> {
        let mut stored_results = Vec::new();

        if self.results_dir.exists() {
            for entry in std::fs::read_dir(&self.results_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.extension().map_or(false, |e| e == "json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(results) = serde_json::from_str::<EvaluationResults>(&content) {
                            stored_results.push(StoredEvalResult {
                                path: path.clone(),
                                results,
                                loaded_at: Utc::now(),
                            });
                        }
                    }
                }
            }
        }

        // Also scan subdirectories (each eval creates its own dir)
        if self.results_dir.exists() {
            for entry in std::fs::read_dir(&self.results_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.is_dir() {
                    // Look for results.json in subdirectory
                    let results_file = path.join("results.json");
                    if results_file.exists() {
                        if let Ok(content) = std::fs::read_to_string(&results_file) {
                            if let Ok(results) = serde_json::from_str::<EvaluationResults>(&content)
                            {
                                stored_results.push(StoredEvalResult {
                                    path: results_file,
                                    results,
                                    loaded_at: Utc::now(),
                                });
                            }
                        }
                    }

                    // Also check for eval_id.json pattern
                    for sub_entry in std::fs::read_dir(&path)? {
                        let sub_entry = sub_entry?;
                        let sub_path = sub_entry.path();

                        if sub_path.extension().map_or(false, |e| e == "json") {
                            if let Ok(content) = std::fs::read_to_string(&sub_path) {
                                if let Ok(results) =
                                    serde_json::from_str::<EvaluationResults>(&content)
                                {
                                    // Avoid duplicates
                                    if !stored_results.iter().any(|r| r.results.eval_id == results.eval_id) {
                                        stored_results.push(StoredEvalResult {
                                            path: sub_path,
                                            results,
                                            loaded_at: Utc::now(),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Sort by started_at descending (newest first)
        stored_results.sort_by(|a, b| b.results.started_at.cmp(&a.results.started_at));

        let mut results = self.results.write().await;
        *results = stored_results;

        Ok(())
    }

    /// Get all stored results
    pub async fn get_results(&self) -> Vec<StoredEvalResult> {
        let results = self.results.read().await;
        results.clone()
    }

    /// Get a specific result by eval_id
    pub async fn get_result(&self, eval_id: &str) -> Option<StoredEvalResult> {
        let results = self.results.read().await;
        results.iter().find(|r| r.results.eval_id == eval_id).cloned()
    }
}
