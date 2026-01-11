//! HTTP handlers for the web UI

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::state::{AppState, SessionInfo, SessionStatus, StoredEvalResult};
use crate::eval::EvaluationResults;

/// Query parameters for listing sessions
#[derive(Debug, Deserialize)]
pub struct ListSessionsQuery {
    /// Filter by eval_id
    pub eval_id: Option<String>,
    /// Filter by status
    pub status: Option<String>,
}

/// Query parameters for listing results
#[derive(Debug, Deserialize)]
pub struct ListResultsQuery {
    /// Limit number of results
    pub limit: Option<usize>,
}

/// Response for session list
#[derive(Debug, Serialize)]
pub struct SessionListResponse {
    pub sessions: Vec<SessionInfo>,
    pub total: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
}

/// Response for results list
#[derive(Debug, Serialize)]
pub struct ResultListResponse {
    pub results: Vec<ResultSummary>,
    pub total: usize,
}

/// Summary of an evaluation result
#[derive(Debug, Serialize)]
pub struct ResultSummary {
    pub eval_id: String,
    pub name: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub total_runs: usize,
    pub completed_runs: usize,
    pub failed_runs: usize,
    pub overall_pass_rate: f64,
    pub best_agent: Option<String>,
    pub agents: Vec<AgentSummary>,
}

/// Summary of an agent's performance
#[derive(Debug, Serialize)]
pub struct AgentSummary {
    pub agent_id: String,
    pub agent_tool: String,
    pub model: String,
    pub rank: u32,
    pub average_score: f64,
    pub passed_tests: u32,
    pub total_tests: u32,
}

impl From<&StoredEvalResult> for ResultSummary {
    fn from(stored: &StoredEvalResult) -> Self {
        let results = &stored.results;
        ResultSummary {
            eval_id: results.eval_id.clone(),
            name: results.name.clone(),
            started_at: results.started_at.to_rfc3339(),
            completed_at: results.completed_at.map(|t| t.to_rfc3339()),
            total_runs: results.runs.len(),
            completed_runs: results.summary.completed as usize,
            failed_runs: results.summary.failed as usize,
            overall_pass_rate: results.summary.overall_pass_rate,
            best_agent: results.summary.best_agent.clone(),
            agents: results
                .agent_scores
                .iter()
                .map(|s| AgentSummary {
                    agent_id: s.agent_id.clone(),
                    agent_tool: s.agent_tool.clone(),
                    model: s.model.clone(),
                    rank: s.rank,
                    average_score: s.average_score,
                    passed_tests: s.passed_tests,
                    total_tests: s.total_tests,
                })
                .collect(),
        }
    }
}

// ============================================================================
// Page Handlers (HTML)
// ============================================================================

/// Home page - redirects to live view
pub async fn index() -> Response {
    Html(include_str!("../../templates/index.html")).into_response()
}

/// Live sessions monitoring page
pub async fn live_page() -> Response {
    Html(include_str!("../../templates/live.html")).into_response()
}

/// Results dashboard page
pub async fn results_page() -> Response {
    Html(include_str!("../../templates/results.html")).into_response()
}

/// Single evaluation detail page
pub async fn eval_detail_page(Path(_eval_id): Path<String>) -> Response {
    // eval_id is handled in the client-side JS by parsing the URL
    Html(include_str!("../../templates/eval_detail.html")).into_response()
}

/// Single session detail page
pub async fn session_detail_page(Path(_session_id): Path<String>) -> Response {
    // session_id is handled in the client-side JS by parsing the URL
    Html(include_str!("../../templates/session_detail.html")).into_response()
}

// ============================================================================
// API Handlers (JSON)
// ============================================================================

/// List all active sessions
pub async fn api_list_sessions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListSessionsQuery>,
) -> Json<SessionListResponse> {
    let mut sessions = state.get_sessions().await;

    // Apply filters
    if let Some(eval_id) = query.eval_id {
        sessions.retain(|s| s.eval_id == eval_id);
    }

    if let Some(status) = query.status {
        let filter_status = match status.as_str() {
            "running" => Some(SessionStatus::Running),
            "completed" => Some(SessionStatus::Completed),
            "failed" => Some(SessionStatus::Failed),
            "queued" => Some(SessionStatus::Queued),
            _ => None,
        };

        if let Some(filter) = filter_status {
            sessions.retain(|s| s.status == filter);
        }
    }

    let total = sessions.len();
    let running = sessions.iter().filter(|s| s.status == SessionStatus::Running).count();
    let completed = sessions.iter().filter(|s| s.status == SessionStatus::Completed).count();
    let failed = sessions.iter().filter(|s| s.status == SessionStatus::Failed).count();

    // Sort by started_at descending
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    Json(SessionListResponse {
        sessions,
        total,
        running,
        completed,
        failed,
    })
}

/// Get a specific session
pub async fn api_get_session(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionInfo>, StatusCode> {
    state
        .get_session(&session_id)
        .await
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

/// Get session logs
pub async fn api_get_session_logs(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<String>>, StatusCode> {
    state
        .get_session(&session_id)
        .await
        .map(|s| Json(s.recent_logs))
        .ok_or(StatusCode::NOT_FOUND)
}

/// List all evaluation results
pub async fn api_list_results(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListResultsQuery>,
) -> Json<ResultListResponse> {
    // Reload results from disk
    let _ = state.load_results().await;

    let results = state.get_results().await;
    let total = results.len();

    let limit = query.limit.unwrap_or(50);
    let summaries: Vec<ResultSummary> = results
        .iter()
        .take(limit)
        .map(ResultSummary::from)
        .collect();

    Json(ResultListResponse {
        results: summaries,
        total,
    })
}

/// Get a specific evaluation result
pub async fn api_get_result(
    State(state): State<Arc<AppState>>,
    Path(eval_id): Path<String>,
) -> Result<Json<EvaluationResults>, StatusCode> {
    // Reload results from disk first
    let _ = state.load_results().await;

    state
        .get_result(&eval_id)
        .await
        .map(|r| Json(r.results))
        .ok_or(StatusCode::NOT_FOUND)
}

/// Refresh results from disk
pub async fn api_refresh_results(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    state
        .load_results()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let count = state.get_results().await.len();
    Ok(Json(serde_json::json!({
        "status": "ok",
        "results_loaded": count
    })))
}

/// Health check endpoint
pub async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "anode-eval-ui"
    }))
}
