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

const CRM_API_PROMPT: &str = r#"Implement a CRM CRUD API in src/main.rs that manages people records.

## Data Model

Each person has:
- id: UUID (auto-generated by the server)
- first_name: String (REQUIRED)
- last_name: String (REQUIRED)
- email: Option<String> (optional)
- phone: Option<String> (optional)

## Required Endpoints

1. POST /people - Create a new person
   - Request body: { first_name, last_name, email?, phone? }
   - Returns: 201 Created with the new person (including generated id)

2. GET /people - List all people
   - Returns: 200 OK with JSON array of all people

3. GET /people/:id - Get a specific person
   - Returns: 200 OK with the person, or 404 Not Found

4. PUT /people/:id - Update a person
   - Request body: { first_name?, last_name?, email?, phone? }
   - Only update fields that are provided (partial update)
   - Returns: 200 OK with updated person, or 404 Not Found

5. DELETE /people/:id - Delete a person
   - Returns: 204 No Content, or 404 Not Found

## Requirements

- Server MUST listen on port 3000
- Use JSON for all request/response bodies
- Store data in memory (HashMap with RwLock already imported)
- Return appropriate HTTP status codes
- The stub types (Person, CreatePersonRequest, UpdatePersonRequest, AppState) are already defined

## Implementation Tips

1. Initialize AppState as Arc<RwLock<HashMap<Uuid, Person>>>
2. Use axum's Router with .route() to define endpoints
3. Use axum::serve() with TcpListener::bind("0.0.0.0:3000")
4. Implement each handler function (create_person, list_people, etc.)

Run `cargo test --test api_conformance` to verify your implementation.
There are 15 conformance tests that must pass."#;

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

    println!("Running Claude Code with Opus on CRM API task...");
    let output = Command::new("claude")
        .args([
            "--model", "claude-opus-4-20250514",
            "--max-turns", "20",
            "--dangerously-skip-permissions",
            "-p", CRM_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    println!("Claude Code stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("Claude Code stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    let (passed, total) = run_crm_api_tests(&workspace);

    println!("\n=== Claude Opus CRM API Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
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

    println!("Running Claude Code with Sonnet on CRM API task...");
    let output = Command::new("claude")
        .args([
            "--model", "claude-sonnet-4-20250514",
            "--max-turns", "20",
            "--dangerously-skip-permissions",
            "-p", CRM_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    println!("Claude Code stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("Claude Code stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    let (passed, total) = run_crm_api_tests(&workspace);

    println!("\n=== Claude Sonnet CRM API Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
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

    println!("Running Claude Code with Haiku on CRM API task...");
    let output = Command::new("claude")
        .args([
            "--model", "claude-haiku-4-5-20251001",
            "--max-turns", "20",
            "--dangerously-skip-permissions",
            "-p", CRM_API_PROMPT,
        ])
        .current_dir(&workspace)
        .env("ANTHROPIC_API_KEY", &api_key)
        .output()
        .expect("Failed to run Claude Code");

    println!("Claude Code stdout:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("Claude Code stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    let (passed, total) = run_crm_api_tests(&workspace);

    println!("\n=== Claude Haiku CRM API Results ===");
    println!("Tests passed: {}/{}", passed, total);
    if total > 0 {
        println!("Pass rate: {:.1}%", (passed as f64 / total as f64) * 100.0);
    }

    assert!(total >= 15, "Expected at least 15 tests, found {}", total);
    assert_eq!(passed, total, "Claude Haiku should pass all {} CRM API tests, but only passed {}", total, passed);
}
