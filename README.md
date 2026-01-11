# ANODE-EVAL

**A**gent **Node** **Eval**uation Framework - A Rust-based framework for running and evaluating AI coding agents on prompts in parallel Kubernetes pods.

## Features

- Run multiple AI coding agents (Claude Code, Codex, OpenCode) on the same prompts
- Parallel execution in Kubernetes pods with configurable parallelism
- Automatic timeout (default 6 hours) to prevent runaway agents
- Multiple test harness support: Cargo (Rust), npm, pytest, Go, and custom commands
- Automatic scoring based on test pass rates
- Agent ranking and comparison across prompts
- JSON and Markdown result reports

## Supported Agents

| Agent | Tool | Models |
|-------|------|--------|
| Claude Code | `claude_code` | `claude_opus_4_5`, `claude_sonnet_4` |
| Codex | `codex` | `gpt_5_2_xhigh`, `gpt_5_2_high`, `gpt_5`, `o3` |
| OpenCode | `opencode` | Various |

## Installation

### Prerequisites

- Rust 1.70+
- Docker
- Kubernetes cluster (kind for local development)
- kubectl configured

### Build

```bash
cargo build --release
```

### Setup Local Kubernetes

```bash
./scripts/setup-local-k8s.sh
```

## Usage

### Generate a sample config

```bash
anode-eval init --output eval-config.yaml
```

### Run an evaluation (dry-run)

```bash
anode-eval run --config eval-config.yaml --dry-run
```

### Run an evaluation

```bash
anode-eval run --config eval-config.yaml
```

### Check status

```bash
anode-eval status --run-id <RUN_ID>
```

### Cancel a run

```bash
anode-eval cancel <RUN_ID> --force
```

### Cleanup resources

```bash
anode-eval cleanup <RUN_ID> --force
```

## Configuration

### Example eval-config.yaml

```yaml
name: "Hello World Evaluation"
description: "Simple evaluation with hello world function"

prompts:
  - id: "hello-world"
    prompt: |
      Implement a function that returns 'Hello, World!'
      The tests are already written. Run `cargo test` to verify.
    eval_path: "./examples/hello_world"
    test_harness: !cargo
      features: []
      release: false

agents:
  - tool: claude_code
    model: claude_opus_4_5
    iterations: 10
  - tool: codex
    model: gpt_5_2_xhigh
    iterations: 10

settings:
  default_timeout_hours: 6
  output_dir: "./eval-results"
  cleanup_on_complete: true
  api_keys:
    env_vars:
      - ANTHROPIC_API_KEY
      - OPENAI_API_KEY
```

### Test Harnesses

- `!cargo` - Rust cargo test
- `!npm` - Node.js npm test
- `!pytest` - Python pytest
- `!go` - Go test
- `!custom` - Custom command

## Output

Results are saved in two formats:
- `{eval_id}.json` - Full results in JSON
- `{eval_id}_report.md` - Human-readable Markdown report

### Example Report

```
# Evaluation Report: Hello World Evaluation

## Summary
- Total Combinations: 2
- Completed: 2
- Failed: 0
- Overall Pass Rate: 85.00%

## Agent Rankings
| Rank | Agent | Model | Score | Tests Passed | Runs |
|------|-------|-------|-------|--------------|------|
| 1 | claude-code | opus-4.5 | 90.00% | 9/10 | 1/1 |
| 2 | codex | gpt-5.2-xhigh | 80.00% | 8/10 | 1/1 |
```

## Architecture

```
anode-eval/
├── src/
│   ├── agents/       # Agent types and configurations
│   ├── cli/          # CLI parsing and config loading
│   ├── eval/         # Evaluation runner and results
│   ├── kubernetes/   # Pod management
│   └── scoring/      # Score calculations
├── k8s/              # Kubernetes manifests
├── examples/         # Example evaluation projects
└── scripts/          # Setup scripts
```

## License

MIT
