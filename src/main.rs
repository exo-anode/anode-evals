mod agents;
mod cli;
mod eval;
mod kubernetes;
mod scoring;
mod web;

use anyhow::Result;
use clap::Parser;
use cli::{Args, Command, EvalConfig};
use eval::{EvalRunner, LocalEvalRunner};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Setup logging
    let log_level = if args.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    let _subscriber = FmtSubscriber::builder()
        .with_max_level(log_level)
        .with_target(false)
        .init();

    match args.command {
        Command::Run(run_args) => {
            run_evaluation(&args.namespace, run_args).await?;
        }
        Command::Status(status_args) => {
            check_status(&args.namespace, status_args).await?;
        }
        Command::Cancel(cancel_args) => {
            cancel_evaluation(&args.namespace, cancel_args).await?;
        }
        Command::List(list_args) => {
            list_evaluations(&args.namespace, list_args).await?;
        }
        Command::Cleanup(cleanup_args) => {
            cleanup_resources(&args.namespace, cleanup_args).await?;
        }
        Command::Init(init_args) => {
            generate_sample_config(init_args)?;
        }
        Command::Ui(ui_args) => {
            start_ui_server(ui_args).await?;
        }
    }

    Ok(())
}

async fn run_evaluation(namespace: &str, args: cli::RunArgs) -> Result<()> {
    info!("Loading evaluation config from {:?}", args.config);

    let config = EvalConfig::load(&args.config)?;

    if args.dry_run {
        println!("Dry run mode - no pods will be created");
        println!("\nConfiguration:");
        println!("  Name: {}", config.name);
        println!("  Prompts: {}", config.prompts.len());
        println!("  Agents: {}", config.agents.len());
        println!(
            "  Total combinations: {}",
            config.prompts.len() * config.agents.len()
        );
        println!("\nPrompts:");
        for prompt in &config.prompts {
            println!(
                "  - {}: {}",
                prompt.id,
                &prompt.prompt[..prompt.prompt.len().min(50)]
            );
        }
        println!("\nAgents:");
        for agent in &config.agents {
            println!("  - {} ({})", agent.tool, agent.model);
        }
        return Ok(());
    }

    // Use local runner if --local flag is set
    if args.local {
        return run_local_evaluation(config, args).await;
    }

    let runner = EvalRunner::new(config, namespace).await?;
    let results = runner.run(args.parallelism, args.timeout_hours).await?;

    print_results(&results);

    // Save results
    let output_dir = args
        .output
        .unwrap_or_else(|| std::path::PathBuf::from(&results.eval_id));
    runner.save_results(&output_dir).await?;

    println!("\nResults saved to: {:?}", output_dir);

    Ok(())
}

async fn run_local_evaluation(config: EvalConfig, args: cli::RunArgs) -> Result<()> {
    println!("\n*** LOCAL MODE - Running tests without Kubernetes ***\n");

    let runner = LocalEvalRunner::new(config);
    let results = runner.run_local_tests().await?;

    print_results(&results);

    // Save results
    let output_dir = args
        .output
        .unwrap_or_else(|| std::path::PathBuf::from(&results.eval_id));
    runner.save_results(&output_dir).await?;

    println!("\nResults saved to: {:?}", output_dir);

    Ok(())
}

fn print_results(results: &eval::EvaluationResults) {
    println!("\n{}", "=".repeat(60));
    println!("EVALUATION COMPLETE");
    println!("{}", "=".repeat(60));
    println!("\nSummary:");
    println!("  Total runs: {}", results.summary.total_combinations);
    println!("  Completed: {}", results.summary.completed);
    println!("  Failed: {}", results.summary.failed);
    println!(
        "  Overall pass rate: {:.2}%",
        results.summary.overall_pass_rate
    );

    println!("\nAgent Rankings:");
    for score in &results.agent_scores {
        println!(
            "  #{} {} ({}) - {:.2}% ({}/{} tests)",
            score.rank,
            score.agent_tool,
            score.model,
            score.average_score,
            score.passed_tests,
            score.total_tests
        );
    }
}

async fn check_status(namespace: &str, args: cli::StatusArgs) -> Result<()> {
    let pod_manager = kubernetes::PodManager::new(namespace).await?;

    if let Some(run_id) = args.run_id {
        let pods = pod_manager.list_run_pods(&run_id).await?;
        println!("Pods for run {}:", run_id);
        for pod_name in pods {
            let status = pod_manager.get_pod_status(&pod_name).await?;
            println!("  {}: {:?}", pod_name, status);
        }
    } else {
        println!("Use --run-id to check status of a specific run");
    }

    Ok(())
}

async fn cancel_evaluation(namespace: &str, args: cli::CancelArgs) -> Result<()> {
    let pod_manager = kubernetes::PodManager::new(namespace).await?;

    if !args.force {
        println!(
            "Are you sure you want to cancel run {}? (use --force to skip confirmation)",
            args.run_id
        );
        return Ok(());
    }

    pod_manager.cleanup_run(&args.run_id).await?;
    println!("Cancelled run: {}", args.run_id);

    Ok(())
}

async fn list_evaluations(namespace: &str, _args: cli::ListArgs) -> Result<()> {
    // This would query stored results - for now just list pods
    let _pod_manager = kubernetes::PodManager::new(namespace).await?;

    println!("Listing evaluations in namespace: {}", namespace);
    println!("(This feature requires a results storage backend)");

    Ok(())
}

async fn cleanup_resources(namespace: &str, args: cli::CleanupArgs) -> Result<()> {
    let pod_manager = kubernetes::PodManager::new(namespace).await?;

    if !args.force {
        println!(
            "Are you sure you want to cleanup {}? (use --force to skip confirmation)",
            args.run_id
        );
        return Ok(());
    }

    if args.run_id == "all" {
        println!("Cleaning up all resources...");
        // Would need to implement list all runs
    } else {
        pod_manager.cleanup_run(&args.run_id).await?;
        println!("Cleaned up run: {}", args.run_id);
    }

    Ok(())
}

fn generate_sample_config(args: cli::InitArgs) -> Result<()> {
    let config = EvalConfig::sample();

    config.save(&args.output)?;
    println!("Generated sample config at: {:?}", args.output);

    Ok(())
}

async fn start_ui_server(args: cli::UiArgs) -> Result<()> {
    info!("Starting web UI server on port {}", args.port);
    info!("Results directory: {:?}", args.results_dir);

    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    println!("║                    ANODE-EVAL Web UI                          ║");
    println!("╠═══════════════════════════════════════════════════════════════╣");
    println!("║  Open http://localhost:{:<5} in your browser                 ║", args.port);
    println!("║                                                               ║");
    println!("║  Pages:                                                       ║");
    println!("║    /         - Dashboard home                                 ║");
    println!("║    /live     - Live session monitoring                        ║");
    println!("║    /results  - Evaluation results                             ║");
    println!("║                                                               ║");
    println!("║  Press Ctrl+C to stop the server                              ║");
    println!("╚═══════════════════════════════════════════════════════════════╝\n");

    web::start_server(args.port, args.results_dir).await?;

    Ok(())
}
