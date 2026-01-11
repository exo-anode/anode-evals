use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// ANODE-EVAL: Agent Node Evaluation Framework
///
/// A framework for running AI coding agents on prompts and evaluating their outputs
/// in parallel Kubernetes pods.
#[derive(Parser, Debug)]
#[command(name = "anode-eval")]
#[command(author = "ANODE Team")]
#[command(version = "0.1.0")]
#[command(about = "Run and evaluate AI coding agents in Kubernetes")]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,

    /// Kubernetes namespace to use
    #[arg(long, default_value = "anode-eval", global = true)]
    pub namespace: String,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Run an evaluation suite
    Run(RunArgs),

    /// Check the status of a running evaluation
    Status(StatusArgs),

    /// Cancel a running evaluation
    Cancel(CancelArgs),

    /// List all evaluations
    List(ListArgs),

    /// Clean up resources from completed evaluations
    Cleanup(CleanupArgs),

    /// Generate a sample evaluation config file
    Init(InitArgs),

    /// Start the web UI server
    Ui(UiArgs),
}

#[derive(Parser, Debug)]
pub struct RunArgs {
    /// Path to the evaluation config file (YAML)
    #[arg(short, long)]
    pub config: PathBuf,

    /// Override the output directory
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Maximum timeout in hours (default: 6)
    #[arg(long, default_value = "6")]
    pub timeout_hours: u32,

    /// Number of parallel pods per agent
    #[arg(long, default_value = "1")]
    pub parallelism: u32,

    /// Dry run - don't actually create pods
    #[arg(long)]
    pub dry_run: bool,

    /// Run tests locally without Kubernetes (for testing the framework)
    #[arg(long)]
    pub local: bool,
}

#[derive(Parser, Debug)]
pub struct StatusArgs {
    /// Run ID to check status for
    #[arg(short, long)]
    pub run_id: Option<String>,

    /// Watch for updates
    #[arg(short, long)]
    pub watch: bool,
}

#[derive(Parser, Debug)]
pub struct CancelArgs {
    /// Run ID to cancel
    pub run_id: String,

    /// Force cancellation without confirmation
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Parser, Debug)]
pub struct ListArgs {
    /// Show only running evaluations
    #[arg(long)]
    pub running: bool,

    /// Show only completed evaluations
    #[arg(long)]
    pub completed: bool,

    /// Limit number of results
    #[arg(short, long, default_value = "20")]
    pub limit: usize,
}

#[derive(Parser, Debug)]
pub struct CleanupArgs {
    /// Run ID to clean up (or "all" for all completed runs)
    pub run_id: String,

    /// Force cleanup without confirmation
    #[arg(short, long)]
    pub force: bool,
}

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Output path for the config file
    #[arg(short, long, default_value = "eval-config.yaml")]
    pub output: PathBuf,

    /// Include example prompts
    #[arg(long)]
    pub with_examples: bool,
}

#[derive(Parser, Debug)]
pub struct UiArgs {
    /// Port to listen on
    #[arg(short, long, default_value = "8080")]
    pub port: u16,

    /// Directory to scan for evaluation results
    #[arg(short, long, default_value = ".")]
    pub results_dir: PathBuf,
}
