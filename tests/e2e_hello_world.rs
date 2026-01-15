//! End-to-end tests for agent evaluations
//!
//! These tests require actual agent CLIs to be installed and API keys to be available.
//! Run with: cargo test --features e2e -- --ignored
//!
//! Requirements:
//! - Claude Code CLI installed (`npm install -g @anthropic-ai/claude-code`)
//! - OpenCode CLI installed (`npm install -g opencode-ai`)
//! - ANTHROPIC_API_KEY file in project root (for Claude Code)
//! - Ollama running locally with qwen2.5-coder:7b model (for OpenCode)

#![cfg(feature = "e2e")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use serde_json;

/// Get the project root directory
fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Read the Anthropic API key from the file
fn read_anthropic_api_key() -> Option<String> {
    let key_path = project_root().join("ANTHROPIC_API_KEY");
    fs::read_to_string(&key_path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Create a temporary workspace with the hello world template
fn setup_workspace(name: &str) -> PathBuf {
    let workspace = project_root().join("target").join("e2e").join(name);

    // Clean up if exists
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("Failed to clean workspace");
    }
    fs::create_dir_all(&workspace).expect("Failed to create workspace");

    // Create Cargo.toml
    fs::write(
        workspace.join("Cargo.toml"),
        r#"[package]
name = "hello_world"
version = "0.1.0"
edition = "2021"

[dependencies]
"#,
    )
    .expect("Failed to write Cargo.toml");

    // Create src directory
    fs::create_dir_all(workspace.join("src")).expect("Failed to create src dir");

    // Copy the lib.rs template with TODOs
    let template = project_root().join("examples/hello_world/src/lib.rs");
    let dest = workspace.join("src/lib.rs");
    fs::copy(&template, &dest).expect("Failed to copy lib.rs");

    workspace
}

/// Run cargo test in the workspace and return pass rate
fn run_cargo_tests(workspace: &Path) -> (u32, u32) {
    let output = Command::new("cargo")
        .args(["test", "--", "--test-threads=1"])
        .current_dir(workspace)
        .output()
        .expect("Failed to run cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    println!("Test output:\n{}", combined);

    // Parse test results
    let mut passed = 0;
    let mut failed = 0;

    for line in combined.lines() {
        if line.starts_with("test ") && line.contains(" ... ok") {
            passed += 1;
        } else if line.starts_with("test ") && line.contains(" ... FAILED") {
            failed += 1;
        }
    }

    (passed, passed + failed)
}

/// Check if Claude Code CLI is installed
fn is_claude_code_installed() -> bool {
    Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if OpenCode CLI is installed
fn is_opencode_installed() -> bool {
    Command::new("opencode")
        .args(["--version"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if Ollama is running with the required model
fn is_ollama_ready() -> bool {
    Command::new("ollama")
        .args(["list"])
        .output()
        .map(|o| {
            o.status.success()
                && String::from_utf8_lossy(&o.stdout).contains("qwen2.5-coder")
        })
        .unwrap_or(false)
}

/// Token usage statistics parsed from Claude Code JSON output
#[derive(Debug, Clone)]
struct TokenUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_input_tokens: u64,
    cache_creation_input_tokens: u64,
    total_cost_usd: f64,
    num_turns: u32,
}

impl TokenUsage {
    fn total_input_tokens(&self) -> u64 {
        self.input_tokens + self.cache_read_input_tokens + self.cache_creation_input_tokens
    }
}

/// Parse Claude Code JSON output to extract token usage and turn count
/// Returns None if parsing fails or output is not JSON
fn parse_claude_output(stdout: &str) -> Option<TokenUsage> {
    // Find the JSON object in the output (it should be the last line or the whole output)
    let json_str = stdout.lines()
        .filter(|line| line.starts_with('{') && line.contains("\"type\""))
        .last()?;

    // Parse as JSON
    let json: serde_json::Value = serde_json::from_str(json_str).ok()?;

    // Extract fields
    let num_turns = json.get("num_turns")?.as_u64()? as u32;
    let total_cost_usd = json.get("total_cost_usd")?.as_f64()?;

    // Get usage object
    let usage = json.get("usage")?;
    let input_tokens = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let output_tokens = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_read_input_tokens = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation_input_tokens = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

    Some(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_input_tokens,
        cache_creation_input_tokens,
        total_cost_usd,
        num_turns,
    })
}

/// Check if output indicates max turns was reached
fn hit_max_turns(stdout: &str, stderr: &str) -> bool {
    let combined = format!("{}\n{}", stdout, stderr);
    combined.contains("Reached max turns")
}

/// Create OpenCode config file for Ollama in the workspace
fn setup_opencode_config(workspace: &Path) {
    let config = r#"{
  "$schema": "https://opencode.ai/config.json",
  "model": "ollama/qwen2.5-coder:7b",
  "provider": {
    "ollama": {
      "npm": "@ai-sdk/openai-compatible",
      "name": "Ollama",
      "options": {
        "baseURL": "http://localhost:11434/v1"
      },
      "models": {
        "qwen2.5-coder:7b": {
          "name": "Qwen 2.5 Coder 7B",
          "tools": true
        }
      }
    }
  }
}"#;
    fs::write(workspace.join("opencode.json"), config).expect("Failed to write opencode.json");
}

const HELLO_WORLD_PROMPT: &str = r#"Implement the two functions in src/lib.rs:

1. `hello_world()` should return the string "Hello, World!" exactly.

2. `hello_name(name: &str)` should return "Hello, {name}!" where {name} is the input parameter.

For example:
- hello_world() returns "Hello, World!"
- hello_name("Alice") returns "Hello, Alice!"
- hello_name("") returns "Hello, !"

The functions currently return empty strings. Replace String::new() with the correct implementations.
Run `cargo test` to verify your implementation passes all 8 tests."#;

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_hello_world() {
    // Check prerequisites
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    // Setup workspace
    let workspace = setup_workspace("claude_code_test");
    println!("Workspace: {:?}", workspace);

    // Run Claude Code with the prompt
    println!("Running Claude Code agent...");
    let output = Command::new("claude")
        .args([
            "--model", "claude-sonnet-4-20250514",
            "--max-turns", "10",
            "--dangerously-skip-permissions",
            "-p", HELLO_WORLD_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    println!("Claude Code stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("Claude Code stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    // Run tests
    let (passed, total) = run_cargo_tests(&workspace);

    println!("\n=== Claude Code Results ===");
    println!("Tests passed: {}/{}", passed, total);
    println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);

    // Assert all tests pass
    assert_eq!(passed, total, "Claude Code should pass all {} tests, but only passed {}", total, passed);
    assert!(total >= 8, "Expected at least 8 tests, found {}", total);
}

#[test]
#[ignore = "Requires OpenCode CLI and Ollama with qwen2.5-coder model"]
fn test_opencode_qwen_hello_world() {
    // Check prerequisites
    if !is_opencode_installed() {
        panic!("OpenCode CLI not installed. Run: npm install -g opencode-ai");
    }

    if !is_ollama_ready() {
        panic!("Ollama not running or qwen2.5-coder model not available. Run: ollama pull qwen2.5-coder:7b");
    }

    // Setup workspace
    let workspace = setup_workspace("opencode_qwen_test");
    println!("Workspace: {:?}", workspace);

    // Note: Ollama provider must be configured in ~/.config/opencode/opencode.json
    // The global config sets up the ollama provider with tool support enabled

    // Run OpenCode with qwen model using the `run` subcommand
    println!("Running OpenCode agent with qwen2.5-coder:7b...");
    let output = Command::new("opencode")
        .args([
            "run",
            "--format", "json",
            "--model", "ollama/qwen2.5-coder:7b",
            HELLO_WORLD_PROMPT,
        ])
        .current_dir(&workspace)
        .output()
        .expect("Failed to run OpenCode");

    println!("OpenCode stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("OpenCode stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    // Run tests
    let (passed, total) = run_cargo_tests(&workspace);

    println!("\n=== OpenCode Qwen Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }

    // Note: Qwen 2.5 Coder 7B may not fully support agentic tool use
    // This test validates the integration works, even if the model doesn't
    // successfully complete the task. The Claude Code test is the primary test.
    if passed < total {
        println!("\nNote: OpenCode with local Qwen model may not fully support tool use.");
        println!("This is a known limitation of smaller local models.");
        println!("Consider using a larger model or cloud-based provider for better results.");
    }

    // Only fail if we got 0 tests (something went wrong)
    // Allow partial success since local models vary in capability
    assert!(total >= 8, "Expected at least 8 tests, found {}", total);
}

/// Helper function to run Claude Code with a specific model
fn run_claude_code_test(model: &str, workspace_name: &str) -> (u32, u32) {
    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_workspace(workspace_name);
    println!("Workspace: {:?}", workspace);

    println!("Running Claude Code with model {}...", model);
    let output = Command::new("claude")
        .args([
            "--model", model,
            "--max-turns", "10",
            "--dangerously-skip-permissions",
            "-p", HELLO_WORLD_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    println!("Claude Code stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("Claude Code stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    run_cargo_tests(&workspace)
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_sonnet_hello_world() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let (passed, total) = run_claude_code_test("claude-sonnet-4-20250514", "claude_sonnet_test");

    println!("\n=== Claude Sonnet 4 Results ===");
    println!("Tests passed: {}/{}", passed, total);
    println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);

    assert_eq!(passed, total, "Claude Sonnet should pass all {} tests, but only passed {}", total, passed);
    assert!(total >= 8, "Expected at least 8 tests, found {}", total);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_haiku_hello_world() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let (passed, total) = run_claude_code_test("claude-haiku-4-5-20251001", "claude_haiku_test");

    println!("\n=== Claude Haiku 4.5 Results ===");
    println!("Tests passed: {}/{}", passed, total);
    println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);

    assert_eq!(passed, total, "Claude Haiku should pass all {} tests, but only passed {}", total, passed);
    assert!(total >= 8, "Expected at least 8 tests, found {}", total);
}

#[test]
#[ignore = "Requires both Claude Code and OpenCode agents"]
fn test_both_agents_hello_world() {
    // Run both tests and compare
    let api_key = read_anthropic_api_key();

    let mut results = Vec::new();

    // Test Claude Code if available
    if is_claude_code_installed() && api_key.is_some() {
        let workspace = setup_workspace("claude_code_compare");
        let api_key = api_key.as_ref().unwrap();

        println!("Running Claude Code...");
        Command::new("claude")
            .args([
                "--model", "claude-sonnet-4-20250514",
                "--max-turns", "10",
                "--dangerously-skip-permissions",
                "-p", HELLO_WORLD_PROMPT,
            ])
            .current_dir(&workspace)
            .env("ANTHROPIC_API_KEY", api_key)
            .output()
            .expect("Failed to run Claude Code");

        let (passed, total) = run_cargo_tests(&workspace);
        results.push(("Claude Code (Sonnet 4)", passed, total));
    }

    // Test OpenCode if available
    if is_opencode_installed() && is_ollama_ready() {
        let workspace = setup_workspace("opencode_compare");
        setup_opencode_config(&workspace);

        println!("Running OpenCode...");
        Command::new("opencode")
            .args([
                "run",
                "--model", "ollama/qwen2.5-coder:7b",
                HELLO_WORLD_PROMPT,
            ])
            .current_dir(&workspace)
            .output()
            .expect("Failed to run OpenCode");

        let (passed, total) = run_cargo_tests(&workspace);
        results.push(("OpenCode (Qwen 7B)", passed, total));
    }

    // Print comparison
    println!("\n{}", "=".repeat(60));
    println!("AGENT COMPARISON RESULTS");
    println!("{}", "=".repeat(60));

    for (agent, passed, total) in &results {
        let rate = if *total > 0 {
            (*passed as f64 / *total as f64) * 100.0
        } else {
            0.0
        };
        println!("{}: {}/{} tests passed ({:.1}%)", agent, passed, total, rate);
    }

    // At least one agent should have been tested
    assert!(!results.is_empty(), "No agents were available to test");

    // All tested agents should pass all tests
    for (agent, passed, total) in results {
        assert_eq!(
            passed, total,
            "{} should pass all tests but only passed {}/{}",
            agent, passed, total
        );
    }
}

// ============================================================================
// CRM API Evaluation Tests
// ============================================================================

/// Create a temporary workspace with the CRM API template
fn setup_crm_workspace(name: &str) -> PathBuf {
    let workspace = project_root().join("target").join("e2e").join(name);

    // Clean up if exists
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("Failed to clean workspace");
    }
    fs::create_dir_all(&workspace).expect("Failed to create workspace");

    // Copy the entire CRM API template
    let template_dir = project_root().join("examples/crm_api");

    // Copy Cargo.toml
    fs::copy(
        template_dir.join("Cargo.toml"),
        workspace.join("Cargo.toml"),
    )
    .expect("Failed to copy Cargo.toml");

    // Create src directory and copy main.rs
    fs::create_dir_all(workspace.join("src")).expect("Failed to create src dir");
    fs::copy(
        template_dir.join("src/main.rs"),
        workspace.join("src/main.rs"),
    )
    .expect("Failed to copy main.rs");

    // Create tests directory and copy api_conformance.rs
    fs::create_dir_all(workspace.join("tests")).expect("Failed to create tests dir");
    fs::copy(
        template_dir.join("tests/api_conformance.rs"),
        workspace.join("tests/api_conformance.rs"),
    )
    .expect("Failed to copy api_conformance.rs");

    workspace
}

/// Run the CRM API conformance tests and return pass rate
fn run_crm_api_tests(workspace: &Path) -> (u32, u32) {
    let output = Command::new("cargo")
        .args(["test", "--test", "api_conformance", "--", "--test-threads=1"])
        .current_dir(workspace)
        .output()
        .expect("Failed to run cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    println!("Test output:\n{}", combined);

    // Parse test results - look for "test <name> ... ok" pattern
    // The output may have server output interleaved, so we look for the pattern anywhere
    let mut passed = 0;
    let mut failed = 0;

    for line in combined.lines() {
        // Match lines containing "test test_" and "... ok" or "... FAILED"
        if line.contains("test test_") && line.contains(" ... ok") {
            passed += 1;
        } else if line.contains("test test_") && line.contains(" ... FAILED") {
            failed += 1;
        }
    }

    // Also check for the summary line like "test result: ok. 15 passed; 0 failed;"
    for line in combined.lines() {
        if line.contains("test result:") && line.contains("passed") {
            // Parse "test result: ok. 15 passed; 0 failed;"
            if let Some(passed_str) = line.split("passed").next() {
                if let Some(num_str) = passed_str.split_whitespace().last() {
                    if let Ok(num) = num_str.parse::<u32>() {
                        if num > passed {
                            passed = num;
                        }
                    }
                }
            }
            if let Some(after_passed) = line.split("passed").nth(1) {
                if let Some(failed_part) = after_passed.split("failed").next() {
                    if let Some(num_str) = failed_part.split_whitespace().last() {
                        if let Ok(num) = num_str.parse::<u32>() {
                            if num > failed {
                                failed = num;
                            }
                        }
                    }
                }
            }
        }
    }

    (passed, passed + failed)
}

const CRM_API_PROMPT: &str = r#"Build a CRM REST API server in Rust that manages people records.

## Requirements

Create a server that:
- Listens on port 3000
- Stores data in memory (no database)
- Uses JSON for request/response bodies

## Data Model

A Person has:
- id: UUID (server-generated)
- first_name: String (required)
- last_name: String (required)
- email: String (optional)
- phone: String (optional)

## Endpoints

| Method | Path | Description | Success | Not Found |
|--------|------|-------------|---------|-----------|
| POST | /people | Create person | 201 + person JSON | - |
| GET | /people | List all people | 200 + array | - |
| GET | /people/:id | Get one person | 200 + person JSON | 404 |
| PUT | /people/:id | Update person (partial) | 200 + person JSON | 404 |
| DELETE | /people/:id | Delete person | 204 (no body) | 404 |

## Example Requests

Create:
POST /people
{"first_name": "John", "last_name": "Doe", "email": "john@example.com"}

Update (partial - only updates provided fields):
PUT /people/uuid-here
{"first_name": "Jane"}

## Tech Stack

The Cargo.toml already has these dependencies:
- axum (web framework)
- tokio (async runtime)
- serde/serde_json (JSON)
- uuid (ID generation)

## Instructions

1. Implement the server in src/main.rs
2. Run `cargo build` to check for compile errors
3. Run `cargo test --test api_conformance` to verify (15 tests must pass)"#;

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_crm_api_opus() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_crm_workspace("claude_crm_opus_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 100;
    println!("Running Claude Code with Opus on CRM API task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-opus-4-20250514",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", CRM_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_crm_api_tests(&workspace);

    println!("\n=== Claude Opus CRM API Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 15, "Expected at least 15 tests, found {}", total);
    assert_eq!(passed, total, "Claude Opus should pass all {} CRM API tests, but only passed {}", total, passed);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_crm_api_sonnet() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_crm_workspace("claude_crm_sonnet_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 100;
    println!("Running Claude Code with Sonnet on CRM API task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-sonnet-4-20250514",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", CRM_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_crm_api_tests(&workspace);

    println!("\n=== Claude Sonnet CRM API Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 15, "Expected at least 15 tests, found {}", total);
    assert_eq!(passed, total, "Claude Sonnet should pass all {} CRM API tests, but only passed {}", total, passed);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_crm_api_haiku() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_crm_workspace("claude_crm_haiku_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 100;
    println!("Running Claude Code with Haiku on CRM API task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-haiku-4-5-20251001",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", CRM_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_crm_api_tests(&workspace);

    println!("\n=== Claude Haiku CRM API Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 15, "Expected at least 15 tests, found {}", total);
    assert_eq!(passed, total, "Claude Haiku should pass all {} CRM API tests, but only passed {}", total, passed);
}

// ============================================================================
// S3 Storage Evaluation Tests
// ============================================================================

const S3_API_PROMPT: &str = r#"Build an S3-compatible object storage server in Rust that handles basic bucket and object operations.

## Requirements

Create a server that:
- Listens on port 3000
- Stores data in memory (no persistence needed)
- Implements S3-compatible REST API using path-style URLs

## API Operations to Implement

### Bucket Operations

| Operation | Method | Path | Success | Error |
|-----------|--------|------|---------|-------|
| CreateBucket | PUT | /{bucket} | 200 | 409 BucketAlreadyExists |
| HeadBucket | HEAD | /{bucket} | 200 | 404 NoSuchBucket |
| DeleteBucket | DELETE | /{bucket} | 204 | 404 NoSuchBucket, 409 BucketNotEmpty |
| ListBuckets | GET | / | 200 + XML | - |

### Object Operations

| Operation | Method | Path | Success | Error |
|-----------|--------|------|---------|-------|
| PutObject | PUT | /{bucket}/{key} | 200 | 404 NoSuchBucket |
| GetObject | GET | /{bucket}/{key} | 200 + body | 404 NoSuchKey/NoSuchBucket |
| HeadObject | HEAD | /{bucket}/{key} | 200 | 404 NoSuchKey |
| DeleteObject | DELETE | /{bucket}/{key} | 204 (always, even if key doesn't exist) | - |
| ListObjectsV2 | GET | /{bucket}?list-type=2 | 200 + XML | 404 NoSuchBucket |

**Important S3 behaviors:**
- DeleteObject should succeed (return 204) even if the object doesn't exist (idempotent delete)
- PutObject to the same key should overwrite the existing object
- Content-Type header from PutObject should be stored and returned on GetObject
- Keys can contain special characters like spaces, slashes for paths, dashes, underscores
- Empty objects (0 bytes) are valid and should be handled correctly
- Binary content must be preserved exactly

## XML Response Formats

### ListBuckets Response
```xml
<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Owner>
    <ID>owner-id</ID>
    <DisplayName>owner</DisplayName>
  </Owner>
  <Buckets>
    <Bucket>
      <Name>bucket-name</Name>
      <CreationDate>2024-01-01T00:00:00.000Z</CreationDate>
    </Bucket>
  </Buckets>
</ListAllMyBucketsResult>
```

### ListObjectsV2 Response
```xml
<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/">
  <Name>bucket-name</Name>
  <Prefix></Prefix>
  <MaxKeys>1000</MaxKeys>
  <IsTruncated>false</IsTruncated>
  <Contents>
    <Key>object-key</Key>
    <LastModified>2024-01-01T00:00:00.000Z</LastModified>
    <ETag>"etag-value"</ETag>
    <Size>1234</Size>
    <StorageClass>STANDARD</StorageClass>
  </Contents>
</ListBucketResult>
```

### Error Response
```xml
<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>NoSuchBucket</Code>
  <Message>The specified bucket does not exist</Message>
  <BucketName>bucket-name</BucketName>
  <RequestId>request-id</RequestId>
</Error>
```

## Required Response Headers

All responses should include:
- `x-amz-request-id`: Unique request identifier

PutObject/GetObject/HeadObject should include:
- `ETag`: MD5 hash of content in quotes (e.g., "d41d8cd98f00b204e9800998ecf8427e")
- `Content-Length`: Size in bytes
- `Last-Modified`: RFC 2822 format timestamp
- `Content-Type`: The content type (store from PutObject, return on GetObject/HeadObject)

## ListObjectsV2 Query Parameters

Support these query parameters:
- `list-type=2` (required to identify ListObjectsV2)
- `prefix`: Filter objects by key prefix
- `max-keys`: Maximum number of keys to return (default 1000)
- `continuation-token`: Token for pagination

## Tech Stack

The Cargo.toml has these dependencies:
- axum (web framework)
- tokio (async runtime)
- serde + quick-xml (XML serialization)
- chrono (timestamps)
- md-5 + base64 (ETag generation)
- uuid (request IDs)

## Instructions

1. Implement the server in src/main.rs
2. Run `cargo build` to check for compile errors
3. Run `cargo test --test s3_conformance -- --test-threads=1` to verify (33 tests must pass)"#;

/// Create a temporary workspace with the S3 storage template
fn setup_s3_workspace(name: &str) -> PathBuf {
    let workspace = project_root().join("target").join("e2e").join(name);

    // Clean up if exists
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("Failed to clean workspace");
    }
    fs::create_dir_all(&workspace).expect("Failed to create workspace");

    // Copy the entire S3 storage template
    let template_dir = project_root().join("examples/s3_storage");

    // Copy Cargo.toml
    fs::copy(
        template_dir.join("Cargo.toml"),
        workspace.join("Cargo.toml"),
    )
    .expect("Failed to copy Cargo.toml");

    // Create src directory and copy main.rs
    fs::create_dir_all(workspace.join("src")).expect("Failed to create src dir");
    fs::copy(
        template_dir.join("src/main.rs"),
        workspace.join("src/main.rs"),
    )
    .expect("Failed to copy main.rs");

    // Create tests directory and copy s3_conformance.rs
    fs::create_dir_all(workspace.join("tests")).expect("Failed to create tests dir");
    fs::copy(
        template_dir.join("tests/s3_conformance.rs"),
        workspace.join("tests/s3_conformance.rs"),
    )
    .expect("Failed to copy s3_conformance.rs");

    workspace
}

/// Run S3 conformance tests and return (passed, total)
fn run_s3_tests(workspace: &Path) -> (u32, u32) {
    let output = Command::new("cargo")
        .args(["test", "--test", "s3_conformance", "--", "--test-threads=1"])
        .current_dir(workspace)
        .output()
        .expect("Failed to run cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    println!("Test output:\n{}", combined);

    // Parse from the summary line: "test result: ok. N passed; M failed; ..."
    let mut passed = 0;
    let mut failed = 0;

    for line in combined.lines() {
        // Look for the test result summary line
        if line.contains("test result:") && line.contains("passed") {
            // Parse "N passed"
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if *part == "passed" || part.starts_with("passed;") || part.starts_with("passed,") {
                    if i > 0 {
                        if let Ok(n) = parts[i - 1].parse::<u32>() {
                            passed = n;
                        }
                    }
                }
                if *part == "failed" || part.starts_with("failed;") || part.starts_with("failed,") {
                    if i > 0 {
                        if let Ok(n) = parts[i - 1].parse::<u32>() {
                            failed = n;
                        }
                    }
                }
            }
        }
    }

    (passed, passed + failed)
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_s3_opus() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_s3_workspace("claude_s3_opus_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 100;
    println!("Running Claude Code with Opus on S3 storage task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-opus-4-20250514",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", S3_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_s3_tests(&workspace);

    println!("\n=== Claude Opus S3 Storage Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 30, "Expected at least 30 tests, found {}", total);
    assert_eq!(passed, total, "Claude Opus should pass all {} S3 tests, but only passed {}", total, passed);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_s3_sonnet() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_s3_workspace("claude_s3_sonnet_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 100;
    println!("Running Claude Code with Sonnet on S3 storage task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-sonnet-4-20250514",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", S3_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_s3_tests(&workspace);

    println!("\n=== Claude Sonnet S3 Storage Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 30, "Expected at least 30 tests, found {}", total);
    assert_eq!(passed, total, "Claude Sonnet should pass all {} S3 tests, but only passed {}", total, passed);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY"]
fn test_claude_code_s3_haiku() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_s3_workspace("claude_s3_haiku_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 100;
    println!("Running Claude Code with Haiku on S3 storage task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-haiku-4-5-20251001",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", S3_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_s3_tests(&workspace);

    println!("\n=== Claude Haiku S3 Storage Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 30, "Expected at least 30 tests, found {}", total);
    assert_eq!(passed, total, "Claude Haiku should pass all {} S3 tests, but only passed {}", total, passed);
}

// ============================================================================
// Distributed S3 Storage Evaluation Tests (Chaos Testing)
// ============================================================================

const DISTRIBUTED_S3_PROMPT: &str = r#"# Distributed S3-Compatible Object Storage Cluster

Build a **distributed, fault-tolerant S3-compatible object storage server** in Rust that runs as a 3-node cluster with consensus-based replication.

## Architecture Overview

You are building a single binary that can be launched multiple times with different configurations to form a cluster. The cluster must:

1. **Run 3 nodes** that communicate over HTTP for replication
2. **Replicate all data** across nodes using quorum-based consensus
3. **Tolerate single node failures** - the cluster must continue operating when 1 node is down
4. **Maintain consistency** - reads after writes must see the written data (linearizable or sequential consistency)

## Command Line Interface

The binary MUST accept these command-line arguments:

```
s3_distributed --node-id <ID> --port <PORT> --peers <PEER_URLS>
```

- `--node-id`: Unique identifier for this node (1, 2, or 3)
- `--port`: HTTP port for S3 API (e.g., 3001, 3002, 3003)
- `--peers`: Comma-separated list of peer node URLs (e.g., "http://localhost:3002,http://localhost:3003")

Example cluster startup:
```bash
# Terminal 1
./s3_distributed --node-id 1 --port 3001 --peers "http://localhost:3002,http://localhost:3003"

# Terminal 2
./s3_distributed --node-id 2 --port 3002 --peers "http://localhost:3001,http://localhost:3003"

# Terminal 3
./s3_distributed --node-id 3 --port 3003 --peers "http://localhost:3001,http://localhost:3002"
```

## Consensus Requirements

### Write Path (PutObject, CreateBucket, DeleteObject, DeleteBucket)

For any write operation to succeed, you MUST:
1. Receive the write request on any node
2. Replicate the operation to peer nodes
3. Wait for acknowledgment from **at least 2 out of 3 nodes** (quorum) before responding success
4. If quorum cannot be reached (e.g., 2 nodes are down), return an error

### Read Path (GetObject, HeadObject, ListObjects, ListBuckets, HeadBucket)

For reads, you have two options:
- **Option A (Simpler)**: Read from local storage - acceptable if writes are quorum-replicated
- **Option B (Stronger)**: Read from quorum for stronger consistency guarantees

### Replication Protocol

Implement a simple replication protocol:

1. **Internal Replication Endpoints**: Each node exposes internal HTTP endpoints for replication:
   - `POST /internal/replicate` - Receive replicated write operations
   - `GET /internal/health` - Health check endpoint

2. **Replication Request Format**:
   ```json
   {
     "operation": "put_object" | "delete_object" | "create_bucket" | "delete_bucket",
     "bucket": "bucket-name",
     "key": "object-key",        // for object operations
     "data": "<base64-encoded>", // for put_object
     "content_type": "text/plain",
     "timestamp": 1234567890
   }
   ```

3. **Conflict Resolution**: Use Last-Writer-Wins based on timestamp. If timestamps are equal, higher node-id wins.

## S3 API Requirements

Each node must expose these S3-compatible endpoints on its configured port:

### Bucket Operations
- `PUT /{bucket}` - CreateBucket
- `GET /` - ListBuckets
- `DELETE /{bucket}` - DeleteBucket (must be empty)
- `HEAD /{bucket}` - HeadBucket

### Object Operations
- `PUT /{bucket}/{key}` - PutObject
- `GET /{bucket}/{key}` - GetObject
- `DELETE /{bucket}/{key}` - DeleteObject
- `HEAD /{bucket}/{key}` - HeadObject
- `GET /{bucket}?list-type=2` - ListObjectsV2

### Response Format

Use standard S3 XML response format. Example ListBuckets response:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<ListAllMyBucketsResult>
  <Buckets>
    <Bucket>
      <Name>my-bucket</Name>
      <CreationDate>2024-01-15T10:30:00Z</CreationDate>
    </Bucket>
  </Buckets>
</ListAllMyBucketsResult>
```

### Error Responses

Return proper S3 error XML:
```xml
<?xml version="1.0" encoding="UTF-8"?>
<Error>
  <Code>NoSuchBucket</Code>
  <Message>The specified bucket does not exist</Message>
  <RequestId>abc123</RequestId>
</Error>
```

Error codes to implement:
- `NoSuchBucket` (404) - Bucket doesn't exist
- `NoSuchKey` (404) - Object doesn't exist
- `BucketAlreadyExists` (409) - Bucket already exists
- `BucketNotEmpty` (409) - Cannot delete non-empty bucket
- `InternalError` (500) - Replication/consensus failure
- `ServiceUnavailable` (503) - Cannot reach quorum

## Fault Tolerance Requirements

Your implementation MUST handle these scenarios:

### 1. Single Node Failure
- When 1 node is down, the remaining 2 nodes MUST continue to:
  - Accept and process write operations (2/3 quorum still possible)
  - Serve read operations
  - Return success for all operations

### 2. Node Recovery
- When a failed node comes back online, it should:
  - Rejoin the cluster
  - Sync any missed data from peers (can be lazy/background sync)
  - Resume normal operation

### 3. Network Partitions
- If a node cannot reach its peers during a write:
  - Try to reach quorum with available nodes
  - If quorum impossible, return 503 ServiceUnavailable

### 4. Timeout Handling
- Replication requests to peers should timeout after 5 seconds
- Don't block indefinitely waiting for a dead node

## Implementation Tips

### Data Storage
- Use in-memory storage (HashMap) for simplicity
- Key structure: `buckets: HashMap<String, Bucket>` where Bucket contains `objects: HashMap<String, Object>`

### Concurrency
- Use `Arc<RwLock<...>>` for thread-safe storage access
- Be careful about holding locks during network calls (can cause deadlocks)

### Replication Flow
```
Client Request → Node 1
     ↓
Node 1 applies locally
     ↓
Node 1 replicates to Node 2 (async) ──→ Node 2 applies & ACKs
Node 1 replicates to Node 3 (async) ──→ Node 3 applies & ACKs
     ↓
Wait for 1 more ACK (need 2 total including self)
     ↓
Return success to client
```

### Health Checking
- Implement `GET /internal/health` returning 200 OK
- Use this to check if peers are alive before attempting replication

## Testing

The test suite will:
1. Start 3 nodes on ports 3001, 3002, 3003
2. Run S3 API conformance tests against any node
3. Kill one node and verify the cluster still works
4. Verify data written before the kill is still readable
5. Verify new writes succeed with 2 nodes
6. Restart the killed node and verify it syncs

Run tests with:
```bash
cargo test --test distributed_conformance -- --test-threads=1
```

## Deliverables

1. Complete implementation in `src/main.rs`
2. The binary must compile with `cargo build --release`
3. The binary must accept the CLI arguments specified above
4. All S3 operations must work when 3 nodes are running
5. All S3 operations must work when only 2 nodes are running
6. Proper error handling when fewer than 2 nodes are available"#;

/// Create a temporary workspace with the distributed S3 template
fn setup_distributed_s3_workspace(name: &str) -> PathBuf {
    let workspace = project_root().join("target").join("e2e").join(name);

    // Clean up if exists
    if workspace.exists() {
        fs::remove_dir_all(&workspace).expect("Failed to clean workspace");
    }
    fs::create_dir_all(&workspace).expect("Failed to create workspace");

    // Copy the distributed S3 template
    let template_dir = project_root().join("examples/s3_distributed");

    // Copy Cargo.toml
    fs::copy(
        template_dir.join("Cargo.toml"),
        workspace.join("Cargo.toml"),
    )
    .expect("Failed to copy Cargo.toml");

    // Create src directory and copy main.rs
    fs::create_dir_all(workspace.join("src")).expect("Failed to create src dir");
    fs::copy(
        template_dir.join("src/main.rs"),
        workspace.join("src/main.rs"),
    )
    .expect("Failed to copy main.rs");

    // Create tests directory and copy distributed_conformance.rs
    fs::create_dir_all(workspace.join("tests")).expect("Failed to create tests dir");
    fs::copy(
        template_dir.join("tests/distributed_conformance.rs"),
        workspace.join("tests/distributed_conformance.rs"),
    )
    .expect("Failed to copy distributed_conformance.rs");

    workspace
}

/// Run distributed S3 conformance tests and return (passed, total)
fn run_distributed_s3_tests(workspace: &Path) -> (u32, u32) {
    let output = Command::new("cargo")
        .args(["test", "--test", "distributed_conformance", "--", "--test-threads=1"])
        .current_dir(workspace)
        .output()
        .expect("Failed to run cargo test");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}\n{}", stdout, stderr);

    println!("Test output:\n{}", combined);

    // Parse from the summary line: "test result: ok. N passed; M failed; ..."
    let mut passed = 0;
    let mut failed = 0;

    for line in combined.lines() {
        // Look for the test result summary line
        if line.contains("test result:") && line.contains("passed") {
            // Parse "N passed"
            let parts: Vec<&str> = line.split_whitespace().collect();
            for (i, part) in parts.iter().enumerate() {
                if *part == "passed" || part.starts_with("passed;") || part.starts_with("passed,") {
                    if i > 0 {
                        if let Ok(n) = parts[i - 1].parse::<u32>() {
                            passed = n;
                        }
                    }
                }
                if *part == "failed" || part.starts_with("failed;") || part.starts_with("failed,") {
                    if i > 0 {
                        if let Ok(n) = parts[i - 1].parse::<u32>() {
                            failed = n;
                        }
                    }
                }
            }
        }
    }

    (passed, passed + failed)
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Long running distributed test"]
fn test_claude_code_distributed_s3_opus() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_distributed_s3_workspace("claude_dist_s3_opus_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 150;
    println!("Running Claude Code with Opus on distributed S3 task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-opus-4-20250514",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", DISTRIBUTED_S3_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_distributed_s3_tests(&workspace);

    println!("\n=== Claude Opus Distributed S3 Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    // Distributed S3 has ~25 tests including chaos tests
    assert!(total >= 20, "Expected at least 20 tests, found {}", total);
    assert_eq!(passed, total, "Claude Opus should pass all {} distributed S3 tests, but only passed {}", total, passed);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Long running distributed test"]
fn test_claude_code_distributed_s3_sonnet() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_distributed_s3_workspace("claude_dist_s3_sonnet_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 150;
    println!("Running Claude Code with Sonnet on distributed S3 task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-sonnet-4-20250514",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", DISTRIBUTED_S3_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_distributed_s3_tests(&workspace);

    println!("\n=== Claude Sonnet Distributed S3 Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 20, "Expected at least 20 tests, found {}", total);
    assert_eq!(passed, total, "Claude Sonnet should pass all {} distributed S3 tests, but only passed {}", total, passed);
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Long running distributed test"]
fn test_claude_code_distributed_s3_haiku() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed. Run: npm install -g @anthropic-ai/claude-code");
    }

    let api_key = read_anthropic_api_key()
        .expect("ANTHROPIC_API_KEY file not found in project root");

    let workspace = setup_distributed_s3_workspace("claude_dist_s3_haiku_test");
    println!("Workspace: {:?}", workspace);

    let max_turns = 150;
    println!("Running Claude Code with Haiku on distributed S3 task (max {} turns)...", max_turns);
    let output = Command::new("claude")
        .args([
            "--model", "claude-haiku-4-5-20251001",
            "--max-turns", &max_turns.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
            "-p", DISTRIBUTED_S3_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Parse token usage from JSON output
    let token_usage = parse_claude_output(&stdout);
    let hit_max = hit_max_turns(&stdout, &stderr);

    let (passed, total) = run_distributed_s3_tests(&workspace);

    println!("\n=== Claude Haiku Distributed S3 Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }
    if let Some(ref usage) = token_usage {
        println!("Turns: {}/{}{}", usage.num_turns, max_turns, if hit_max { " (hit max)" } else { "" });
        println!("Tokens: {} input, {} output", usage.total_input_tokens(), usage.output_tokens);
        println!("Cost: ${:.4}", usage.total_cost_usd);
    } else {
        println!("Token usage: not available");
        if hit_max {
            println!("Note: Hit max turns limit");
        }
    }

    assert!(total >= 20, "Expected at least 20 tests, found {}", total);
    assert_eq!(passed, total, "Claude Haiku should pass all {} distributed S3 tests, but only passed {}", total, passed);
}

// ============================================================================
// Multi-run evaluation with statistics
// ============================================================================

/// Configuration for running an eval
#[derive(Debug, Clone)]
struct EvalConfig {
    model: &'static str,
    model_name: &'static str,
    ralph_loop: bool,
    max_iterations: usize,
    turns_per_iteration: u32,
}

impl EvalConfig {
    fn single_shot(model: &'static str, model_name: &'static str) -> Self {
        Self {
            model,
            model_name,
            ralph_loop: false,
            max_iterations: 1,
            turns_per_iteration: 150,
        }
    }

    fn with_ralph_loop(model: &'static str, model_name: &'static str) -> Self {
        Self {
            model,
            model_name,
            ralph_loop: true,
            max_iterations: 10,
            turns_per_iteration: 50,
        }
    }

    fn display_name(&self) -> String {
        if self.ralph_loop {
            format!("{} (Ralph)", self.model_name)
        } else {
            self.model_name.to_string()
        }
    }
}

/// Results from a single eval run
#[derive(Debug, Clone)]
struct EvalRunStats {
    passed: u32,
    total: u32,
    turns: u32,
    cost_usd: f64,
    input_tokens: u64,
    output_tokens: u64,
    iterations_used: usize,
}

/// Aggregated statistics from multiple runs
#[derive(Debug)]
struct MultiRunStats {
    runs: Vec<EvalRunStats>,
    config: EvalConfig,
}

impl MultiRunStats {
    fn new(config: &EvalConfig) -> Self {
        Self {
            runs: Vec::new(),
            config: config.clone(),
        }
    }

    fn add_run(&mut self, run: EvalRunStats) {
        self.runs.push(run);
    }

    fn pass_rates(&self) -> Vec<f64> {
        self.runs.iter()
            .map(|r| if r.total > 0 { r.passed as f64 / r.total as f64 * 100.0 } else { 0.0 })
            .collect()
    }

    fn avg_pass_rate(&self) -> f64 {
        let rates = self.pass_rates();
        if rates.is_empty() { return 0.0; }
        rates.iter().sum::<f64>() / rates.len() as f64
    }

    fn std_dev_pass_rate(&self) -> f64 {
        let rates = self.pass_rates();
        if rates.len() < 2 { return 0.0; }
        let avg = self.avg_pass_rate();
        let variance = rates.iter()
            .map(|r| (r - avg).powi(2))
            .sum::<f64>() / (rates.len() - 1) as f64;
        variance.sqrt()
    }

    fn avg_cost(&self) -> f64 {
        if self.runs.is_empty() { return 0.0; }
        self.runs.iter().map(|r| r.cost_usd).sum::<f64>() / self.runs.len() as f64
    }

    fn total_cost(&self) -> f64 {
        self.runs.iter().map(|r| r.cost_usd).sum()
    }

    fn avg_turns(&self) -> f64 {
        if self.runs.is_empty() { return 0.0; }
        self.runs.iter().map(|r| r.turns as f64).sum::<f64>() / self.runs.len() as f64
    }

    fn avg_iterations(&self) -> f64 {
        if self.runs.is_empty() { return 0.0; }
        self.runs.iter().map(|r| r.iterations_used as f64).sum::<f64>() / self.runs.len() as f64
    }

    fn print_report(&self) {
        let display_name = self.config.display_name();
        println!("\n{}", "=".repeat(70));
        println!("=== {} - {} Runs Summary ===", display_name, self.runs.len());
        println!("{}\n", "=".repeat(70));

        // Raw results
        println!("Raw Results:");
        println!("{:-<60}", "");
        for (i, run) in self.runs.iter().enumerate() {
            let pass_rate = if run.total > 0 { run.passed as f64 / run.total as f64 * 100.0 } else { 0.0 };
            if self.config.ralph_loop {
                println!("  Run {}: {}/{} ({:.1}%) | {} iters, {} turns | ${:.4}",
                    i + 1, run.passed, run.total, pass_rate, run.iterations_used, run.turns, run.cost_usd);
            } else {
                println!("  Run {}: {}/{} ({:.1}%) | {} turns | ${:.4}",
                    i + 1, run.passed, run.total, pass_rate, run.turns, run.cost_usd);
            }
        }

        // Statistics
        println!("\nStatistics:");
        println!("{:-<60}", "");
        println!("  Pass Rate: {:.1}% avg, {:.1}% std dev", self.avg_pass_rate(), self.std_dev_pass_rate());
        println!("  Pass Rates: {:?}", self.pass_rates().iter().map(|r| format!("{:.1}%", r)).collect::<Vec<_>>());
        if self.config.ralph_loop {
            println!("  Iterations: {:.1} avg", self.avg_iterations());
        }
        println!("  Turns: {:.1} avg", self.avg_turns());
        println!("  Cost: ${:.4} avg, ${:.4} total", self.avg_cost(), self.total_cost());
        println!();
    }
}

/// Run a single distributed S3 evaluation with the given config
fn run_distributed_s3_eval(config: &EvalConfig, run_num: usize, api_key: &str) -> EvalRunStats {
    let mode_suffix = if config.ralph_loop { "_ralph" } else { "" };
    let workspace_name = format!("claude_dist_s3_{}{}_run{}",
        config.model_name.to_lowercase().replace(" ", "_"), mode_suffix, run_num);
    let workspace = setup_distributed_s3_workspace(&workspace_name);

    let display_name = config.display_name();

    if config.ralph_loop {
        println!("\n[{}] Run {} - Workspace: {:?}", display_name, run_num, workspace);
        println!("[{}] Max iterations: {}, turns per iteration: {}",
            display_name, config.max_iterations, config.turns_per_iteration);
    } else {
        println!("\n[{}] Run {} - Workspace: {:?}", display_name, run_num, workspace);
    }

    let mut total_turns = 0u32;
    let mut total_cost = 0.0f64;
    let mut total_input_tokens = 0u64;
    let mut total_output_tokens = 0u64;
    let mut passed = 0u32;
    let mut total = 0u32;
    let mut iterations_used = 0usize;

    for iteration in 1..=config.max_iterations {
        iterations_used = iteration;

        if config.ralph_loop {
            println!("\n[{}] Run {}, Iteration {}/{}", display_name, run_num, iteration, config.max_iterations);
        }

        let (prompt, use_continue) = if iteration == 1 {
            (DISTRIBUTED_S3_PROMPT.to_string(), false)
        } else {
            let fix_prompt = format!(
                "The tests are not all passing yet. {} out of {} tests pass.\n\n\
                Run `cargo test --test distributed_conformance -- --test-threads=1` to see which tests are failing.\n\n\
                Analyze the failing tests, understand why they fail, and fix the implementation in src/main.rs.\n\n\
                Keep working until all {} tests pass.",
                passed, total, total
            );
            (fix_prompt, true)
        };

        let mut cmd = Command::new("claude");
        cmd.args([
            "--model", config.model,
            "--max-turns", &config.turns_per_iteration.to_string(),
            "--output-format", "json",
            "--dangerously-skip-permissions",
        ]);

        if use_continue {
            cmd.args(["--continue", "-p", &prompt]);
        } else {
            cmd.args(["-p", &prompt]);
        }

        let output = cmd
            .current_dir(&workspace)
            .env("ANTHROPIC_API_KEY", api_key)
            .output()
            .expect("Failed to run Claude Code");

        let stdout = String::from_utf8_lossy(&output.stdout);

        if let Some(ref usage) = parse_claude_output(&stdout) {
            total_turns += usage.num_turns;
            total_cost += usage.total_cost_usd;
            total_input_tokens += usage.total_input_tokens();
            total_output_tokens += usage.output_tokens;
        }

        let (new_passed, new_total) = run_distributed_s3_tests(&workspace);
        passed = new_passed;
        total = new_total;

        let pass_rate = if total > 0 { passed as f64 / total as f64 * 100.0 } else { 0.0 };

        if config.ralph_loop {
            println!("[{}] Iteration {} result: {}/{} ({:.1}%) | total turns: {} | cost: ${:.4}",
                display_name, iteration, passed, total, pass_rate, total_turns, total_cost);

            if passed == total && total > 0 {
                println!("[{}] All tests pass! Complete after {} iterations.", display_name, iteration);
                break;
            }

            if iteration == config.max_iterations {
                println!("[{}] Max iterations reached. Final: {}/{}", display_name, passed, total);
            }
        } else {
            // Single-shot mode: just run once
            break;
        }
    }

    let pass_rate = if total > 0 { passed as f64 / total as f64 * 100.0 } else { 0.0 };
    if config.ralph_loop {
        println!("[{}] Run {} complete: {}/{} ({:.1}%) | {} iters, {} turns | ${:.4}",
            display_name, run_num, passed, total, pass_rate, iterations_used, total_turns, total_cost);
    } else {
        println!("[{}] Run {} complete: {}/{} ({:.1}%) | {} turns | ${:.4}",
            display_name, run_num, passed, total, pass_rate, total_turns, total_cost);
    }

    EvalRunStats {
        passed,
        total,
        turns: total_turns,
        cost_usd: total_cost,
        input_tokens: total_input_tokens,
        output_tokens: total_output_tokens,
        iterations_used,
    }
}

/// Run distributed S3 eval N times with the given config
fn run_distributed_s3_multi(config: &EvalConfig, num_runs: usize, api_key: &str) -> MultiRunStats {
    let mut stats = MultiRunStats::new(config);
    let display_name = config.display_name();

    println!("\n{}", "=".repeat(70));
    if config.ralph_loop {
        println!("Starting {} runs for {} (max {} iterations, {} turns each)",
            num_runs, display_name, config.max_iterations, config.turns_per_iteration);
    } else {
        println!("Starting {} runs for {} (single-shot, {} turns)",
            num_runs, display_name, config.turns_per_iteration);
    }
    println!("{}", "=".repeat(70));

    for i in 1..=num_runs {
        let run_stats = run_distributed_s3_eval(config, i, api_key);
        stats.add_run(run_stats);
    }

    stats.print_report();
    stats
}

/// Model constants
const SONNET_MODEL: &str = "claude-sonnet-4-20250514";
const OPUS_MODEL: &str = "claude-opus-4-20250514";
const HAIKU_MODEL: &str = "claude-haiku-4-5-20251001";

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Multi-run statistical test"]
fn test_distributed_s3_multi_run_sonnet() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

    let config = EvalConfig::single_shot(SONNET_MODEL, "Sonnet");
    let stats = run_distributed_s3_multi(&config, num_runs, &api_key);
    println!("\nFinal Sonnet Stats: {:.1}% +/- {:.1}%", stats.avg_pass_rate(), stats.std_dev_pass_rate());
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Multi-run statistical test"]
fn test_distributed_s3_multi_run_opus() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

    let config = EvalConfig::single_shot(OPUS_MODEL, "Opus");
    let stats = run_distributed_s3_multi(&config, num_runs, &api_key);
    println!("\nFinal Opus Stats: {:.1}% +/- {:.1}%", stats.avg_pass_rate(), stats.std_dev_pass_rate());
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Multi-run statistical test"]
fn test_distributed_s3_multi_run_haiku() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

    let config = EvalConfig::single_shot(HAIKU_MODEL, "Haiku");
    let stats = run_distributed_s3_multi(&config, num_runs, &api_key);
    println!("\nFinal Haiku Stats: {:.1}% +/- {:.1}%", stats.avg_pass_rate(), stats.std_dev_pass_rate());
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Ralph Loop test for Sonnet"]
fn test_distributed_s3_ralph_loop_sonnet() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);

    let config = EvalConfig::with_ralph_loop(SONNET_MODEL, "Sonnet");
    let stats = run_distributed_s3_multi(&config, num_runs, &api_key);
    println!("\nFinal Sonnet (Ralph) Stats: {:.1}% +/- {:.1}%", stats.avg_pass_rate(), stats.std_dev_pass_rate());
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Ralph Loop test for Opus"]
fn test_distributed_s3_ralph_loop_opus() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);

    let config = EvalConfig::with_ralph_loop(OPUS_MODEL, "Opus");
    let stats = run_distributed_s3_multi(&config, num_runs, &api_key);
    println!("\nFinal Opus (Ralph) Stats: {:.1}% +/- {:.1}%", stats.avg_pass_rate(), stats.std_dev_pass_rate());
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Ralph Loop test for Haiku"]
fn test_distributed_s3_ralph_loop_haiku() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);

    let config = EvalConfig::with_ralph_loop(HAIKU_MODEL, "Haiku");
    let stats = run_distributed_s3_multi(&config, num_runs, &api_key);
    println!("\nFinal Haiku (Ralph) Stats: {:.1}% +/- {:.1}%", stats.avg_pass_rate(), stats.std_dev_pass_rate());
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - All 6 variants comparison"]
fn test_distributed_s3_all_variants() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(1);

    println!("\n{}", "#".repeat(80));
    println!("# DISTRIBUTED S3 EVAL - ALL 6 VARIANTS ({} run(s) each)", num_runs);
    println!("# Variants: Haiku, Haiku+Ralph, Sonnet, Sonnet+Ralph, Opus, Opus+Ralph");
    println!("{}", "#".repeat(80));

    // Define all 6 configurations
    let configs = vec![
        EvalConfig::single_shot(HAIKU_MODEL, "Haiku"),
        EvalConfig::with_ralph_loop(HAIKU_MODEL, "Haiku"),
        EvalConfig::single_shot(SONNET_MODEL, "Sonnet"),
        EvalConfig::with_ralph_loop(SONNET_MODEL, "Sonnet"),
        EvalConfig::single_shot(OPUS_MODEL, "Opus"),
        EvalConfig::with_ralph_loop(OPUS_MODEL, "Opus"),
    ];

    let mut all_stats: Vec<MultiRunStats> = Vec::new();

    for config in &configs {
        let stats = run_distributed_s3_multi(config, num_runs, &api_key);
        all_stats.push(stats);
    }

    // Final comparison table
    println!("\n{}", "#".repeat(80));
    println!("# FINAL COMPARISON - ALL 6 VARIANTS ({} run(s) each)", num_runs);
    println!("{}", "#".repeat(80));
    println!("\n{:<20} {:>12} {:>10} {:>10} {:>10} {:>10}",
        "Variant", "Pass Rate", "Std Dev", "Avg Iters", "Avg Turns", "Avg Cost");
    println!("{:-<80}", "");

    for stats in &all_stats {
        let display_name = stats.config.display_name();
        let iters = if stats.config.ralph_loop {
            format!("{:.1}", stats.avg_iterations())
        } else {
            "-".to_string()
        };
        println!("{:<20} {:>11.1}% {:>9.1}% {:>10} {:>10.1} {:>10.4}",
            display_name,
            stats.avg_pass_rate(),
            stats.std_dev_pass_rate(),
            iters,
            stats.avg_turns(),
            stats.avg_cost());
    }
    println!();

    // Summary insights
    println!("Insights:");
    println!("{:-<80}", "");
    for stats in &all_stats {
        let display_name = stats.config.display_name();
        let success_runs = stats.runs.iter().filter(|r| r.passed == r.total && r.total > 0).count();
        println!("  {}: {} of {} runs achieved 100%", display_name, success_runs, stats.runs.len());
    }
    println!();
}

#[test]
#[ignore = "Requires Claude Code CLI and ANTHROPIC_API_KEY - Multi-run all models (single-shot only)"]
fn test_distributed_s3_multi_run_all() {
    if !is_claude_code_installed() {
        panic!("Claude Code CLI not installed");
    }
    let api_key = read_anthropic_api_key().expect("ANTHROPIC_API_KEY not found");
    let num_runs = std::env::var("EVAL_RUNS").ok().and_then(|s| s.parse().ok()).unwrap_or(5);

    println!("\n{}", "#".repeat(70));
    println!("# DISTRIBUTED S3 EVAL - {} RUNS PER MODEL (single-shot)", num_runs);
    println!("{}", "#".repeat(70));

    let configs = vec![
        EvalConfig::single_shot(SONNET_MODEL, "Sonnet"),
        EvalConfig::single_shot(OPUS_MODEL, "Opus"),
        EvalConfig::single_shot(HAIKU_MODEL, "Haiku"),
    ];

    let mut all_stats: Vec<MultiRunStats> = Vec::new();
    for config in &configs {
        let stats = run_distributed_s3_multi(config, num_runs, &api_key);
        all_stats.push(stats);
    }

    // Final comparison
    println!("\n{}", "#".repeat(70));
    println!("# FINAL COMPARISON ({} runs each)", num_runs);
    println!("{}", "#".repeat(70));
    println!("\n{:<10} {:>15} {:>15} {:>12} {:>12}", "Model", "Avg Pass Rate", "Std Dev", "Avg Cost", "Total Cost");
    println!("{:-<70}", "");
    for stats in &all_stats {
        println!("{:<10} {:>14.1}% {:>14.1}% {:>11.4} {:>11.4}",
            stats.config.model_name,
            stats.avg_pass_rate(),
            stats.std_dev_pass_rate(),
            stats.avg_cost(),
            stats.total_cost());
    }
    println!();
}
