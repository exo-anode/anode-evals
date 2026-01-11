// Scoring module - most scoring logic is in eval/results.rs
// This module provides additional scoring utilities

use crate::eval::EvaluationResults;
use serde::{Deserialize, Serialize};

/// Detailed score breakdown for an agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedScore {
    /// Pass rate (0-100)
    pub pass_rate: f64,
    /// Completion rate (successful runs / total runs)
    pub completion_rate: f64,
    /// Average run time in seconds
    pub avg_run_time_seconds: f64,
    /// Consistency score (how consistent are results)
    pub consistency: f64,
    /// Final weighted score
    pub weighted_score: f64,
}

impl DetailedScore {
    /// Calculate a weighted score from the components
    pub fn calculate_weighted(
        pass_rate: f64,
        completion_rate: f64,
        consistency: f64,
    ) -> f64 {
        // Weights: pass_rate is most important
        const PASS_RATE_WEIGHT: f64 = 0.7;
        const COMPLETION_WEIGHT: f64 = 0.2;
        const CONSISTENCY_WEIGHT: f64 = 0.1;

        (pass_rate * PASS_RATE_WEIGHT)
            + (completion_rate * COMPLETION_WEIGHT)
            + (consistency * CONSISTENCY_WEIGHT)
    }
}

/// Calculate detailed scores for all agents in the results
pub fn calculate_detailed_scores(results: &EvaluationResults) -> Vec<(String, DetailedScore)> {
    results
        .agent_scores
        .iter()
        .map(|score| {
            let pass_rate = score.average_score;
            let completion_rate = if score.total_runs > 0 {
                (score.completed_runs as f64 / score.total_runs as f64) * 100.0
            } else {
                0.0
            };

            // Calculate average run time (would need to aggregate from runs)
            let avg_run_time = 0.0; // Placeholder

            // Consistency is 100% if all completed runs have same score
            let consistency = 100.0; // Placeholder - would calculate variance

            let weighted = DetailedScore::calculate_weighted(pass_rate, completion_rate, consistency);

            (
                score.agent_id.clone(),
                DetailedScore {
                    pass_rate,
                    completion_rate,
                    avg_run_time_seconds: avg_run_time,
                    consistency,
                    weighted_score: weighted,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weighted_score() {
        let score = DetailedScore::calculate_weighted(80.0, 100.0, 90.0);
        // 80 * 0.7 + 100 * 0.2 + 90 * 0.1 = 56 + 20 + 9 = 85
        assert!((score - 85.0).abs() < 0.001);
    }
}
