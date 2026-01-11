use crate::agents::{AgentConfig, AgentTool};
use k8s_openapi::api::core::v1::{
    Container, EnvVar, Pod, PodSpec, ResourceRequirements, SecurityContext, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::api::ObjectMeta;
use std::collections::BTreeMap;

/// Configuration for creating an agent pod
#[derive(Debug, Clone)]
pub struct AgentPodConfig {
    pub agent: AgentConfig,
    pub prompt: String,
    pub eval_path: String,
    pub run_id: String,
    pub namespace: String,
    pub timeout_hours: u32,
    pub api_keys: BTreeMap<String, String>,
    /// Test command to run after agent completes (e.g., "cargo test")
    pub test_command: String,
    /// Test command arguments
    pub test_args: Vec<String>,
    /// Optional git repo to clone for the workspace
    pub git_repo: Option<String>,
    /// Setup commands to run before the agent
    pub setup_commands: Vec<String>,
}

impl AgentPodConfig {
    /// Generate the pod name
    pub fn pod_name(&self) -> String {
        format!(
            "anode-eval-{}-{}",
            self.agent.id().replace('.', "-").to_lowercase(),
            &self.run_id[..8]
        )
    }
}

/// Build a Kubernetes Pod specification for running an agent
pub fn build_agent_pod(config: &AgentPodConfig) -> Pod {
    let pod_name = config.pod_name();

    // Build environment variables for API keys
    let mut env_vars: Vec<EnvVar> = config
        .api_keys
        .iter()
        .map(|(key, value)| EnvVar {
            name: key.clone(),
            value: Some(value.clone()),
            value_from: None,
        })
        .collect();

    // Add run configuration as env vars
    env_vars.push(EnvVar {
        name: "ANODE_RUN_ID".to_string(),
        value: Some(config.run_id.clone()),
        value_from: None,
    });
    env_vars.push(EnvVar {
        name: "ANODE_AGENT_TOOL".to_string(),
        value: Some(config.agent.tool.to_string()),
        value_from: None,
    });
    env_vars.push(EnvVar {
        name: "ANODE_MODEL".to_string(),
        value: Some(config.agent.model.to_string()),
        value_from: None,
    });
    env_vars.push(EnvVar {
        name: "ANODE_ITERATIONS".to_string(),
        value: Some(config.agent.iterations.to_string()),
        value_from: None,
    });
    env_vars.push(EnvVar {
        name: "ANODE_TIMEOUT_HOURS".to_string(),
        value: Some(config.timeout_hours.to_string()),
        value_from: None,
    });

    // Build the entrypoint script that will:
    // 1. Install the agent CLI
    // 2. Clone/setup the workspace
    // 3. Run the agent with the prompt
    // 4. Signal completion
    let entrypoint_script = build_entrypoint_script(config);

    let container = Container {
        name: "agent".to_string(),
        image: Some("anode-eval-agent:latest".to_string()),
        image_pull_policy: Some("IfNotPresent".to_string()),
        command: Some(vec!["/bin/bash".to_string(), "-c".to_string()]),
        args: Some(vec![entrypoint_script]),
        env: Some(env_vars),
        resources: Some(ResourceRequirements {
            limits: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity("1".to_string())),
                ("memory".to_string(), Quantity("1Gi".to_string())),
            ])),
            requests: Some(BTreeMap::from([
                ("cpu".to_string(), Quantity("500m".to_string())),
                ("memory".to_string(), Quantity("512Mi".to_string())),
            ])),
            ..Default::default()
        }),
        security_context: Some(SecurityContext {
            run_as_non_root: Some(true),
            run_as_user: Some(1000),
            ..Default::default()
        }),
        volume_mounts: Some(vec![
            VolumeMount {
                name: "workspace".to_string(),
                mount_path: "/workspace".to_string(),
                ..Default::default()
            },
            VolumeMount {
                name: "results".to_string(),
                mount_path: "/results".to_string(),
                ..Default::default()
            },
        ]),
        working_dir: Some("/workspace".to_string()),
        ..Default::default()
    };

    let mut labels = BTreeMap::new();
    labels.insert("app".to_string(), "anode-eval".to_string());
    labels.insert("run-id".to_string(), config.run_id.clone());
    labels.insert(
        "agent".to_string(),
        config.agent.id().replace('.', "-").to_lowercase(),
    );

    let mut annotations = BTreeMap::new();
    annotations.insert("anode-eval/prompt".to_string(), config.prompt.clone());
    annotations.insert("anode-eval/eval-path".to_string(), config.eval_path.clone());

    Pod {
        metadata: ObjectMeta {
            name: Some(pod_name),
            namespace: Some(config.namespace.clone()),
            labels: Some(labels),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![container],
            restart_policy: Some("Never".to_string()),
            // 6 hour timeout by default
            active_deadline_seconds: Some((config.timeout_hours * 3600) as i64),
            volumes: Some(vec![
                k8s_openapi::api::core::v1::Volume {
                    name: "workspace".to_string(),
                    empty_dir: Some(Default::default()),
                    ..Default::default()
                },
                k8s_openapi::api::core::v1::Volume {
                    name: "results".to_string(),
                    empty_dir: Some(Default::default()),
                    ..Default::default()
                },
            ]),
            ..Default::default()
        }),
        status: None,
    }
}

/// Build the entrypoint script for the agent container
fn build_entrypoint_script(config: &AgentPodConfig) -> String {
    let install_cmd = config.agent.tool.install_command();
    let cli_cmd = config.agent.tool.cli_command();
    let model = config.agent.model.to_string();
    let iterations = config.agent.iterations;

    // Escape the prompt for shell
    let escaped_prompt = config.prompt.replace('\'', "'\\''");

    let run_command = match config.agent.tool {
        AgentTool::ClaudeCode => {
            // Claude Code: --max-turns for iterations, --dangerously-skip-permissions for non-interactive
            // -p for prompt (non-interactive mode)
            format!(
                r#"{cli_cmd} --model {model} --max-turns {iterations} --dangerously-skip-permissions -p '{escaped_prompt}'"#,
            )
        }
        AgentTool::Codex => {
            // Codex: --full-auto for non-interactive mode with iterations
            format!(
                r#"{cli_cmd} --model {model} --full-auto --iterations {iterations} '{escaped_prompt}'"#,
            )
        }
        AgentTool::OpenCode => {
            // OpenCode: --auto-edit for non-interactive mode
            format!(
                r#"{cli_cmd} --model {model} --auto-edit --max-iterations {iterations} '{escaped_prompt}'"#,
            )
        }
    };

    // Build the test command
    let test_cmd = if config.test_args.is_empty() {
        config.test_command.clone()
    } else {
        format!("{} {}", config.test_command, config.test_args.join(" "))
    };

    // Build git clone command if repo is specified
    let git_clone_cmd = if let Some(ref repo) = config.git_repo {
        format!("git clone {} /workspace", repo)
    } else {
        "echo 'No git repo specified'".to_string()
    };

    // Build setup commands
    let setup_cmds = if config.setup_commands.is_empty() {
        "echo 'No setup commands'".to_string()
    } else {
        config.setup_commands.join("\n")
    };

    format!(
        r#"#!/bin/bash
set -e

echo "=== ANODE-EVAL Agent Runner ==="
echo "Run ID: $ANODE_RUN_ID"
echo "Agent: $ANODE_AGENT_TOOL"
echo "Model: $ANODE_MODEL"
echo "Iterations: $ANODE_ITERATIONS"
echo "Timeout: $ANODE_TIMEOUT_HOURS hours"
echo ""

# Create status file
echo "starting" > /results/status

# Install agent CLI
echo "Installing agent CLI..."
{install_cmd} || {{ echo "failed" > /results/status; exit 1; }}
echo "Agent CLI installed successfully"

# Clone repo if specified
echo "Setting up workspace..."
{git_clone_cmd}

# Setup workspace
cd /workspace

# Run setup commands
echo "Running setup commands..."
{setup_cmds}

# Create a marker file to track agent activity
touch /results/heartbeat

# Run the agent in background while monitoring
echo "Starting agent..."
echo "running" > /results/status

# Start heartbeat monitor in background
(while true; do touch /results/heartbeat; sleep 30; done) &
HEARTBEAT_PID=$!

# Run the agent
{run_command} 2>&1 | tee /results/agent_output.log
AGENT_EXIT_CODE=${{PIPESTATUS[0]}}

# Stop heartbeat
kill $HEARTBEAT_PID 2>/dev/null || true

if [ $AGENT_EXIT_CODE -eq 0 ]; then
    echo "agent_completed" > /results/status
    echo "Agent completed successfully"
else
    echo "agent_failed" > /results/status
    echo "Agent failed with exit code $AGENT_EXIT_CODE"
fi

# Store exit code
echo $AGENT_EXIT_CODE > /results/agent_exit_code

echo "=== Agent run complete ==="

# Run eval tests
echo ""
echo "=== ANODE-EVAL Test Runner ==="
echo "Running: {test_cmd}"
echo "TEST_OUTPUT_START"
{test_cmd} 2>&1 || true
echo "TEST_OUTPUT_END"
echo "=== Test run complete ==="
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::presets;

    #[test]
    fn test_pod_name_generation() {
        let config = AgentPodConfig {
            agent: presets::claude_opus_45(),
            prompt: "Test prompt".to_string(),
            eval_path: "/evals/test".to_string(),
            run_id: "12345678-abcd-1234-abcd-123456789abc".to_string(),
            namespace: "default".to_string(),
            timeout_hours: 6,
            api_keys: BTreeMap::new(),
            test_command: "cargo".to_string(),
            test_args: vec!["test".to_string()],
            git_repo: None,
            setup_commands: vec![],
        };

        let pod_name = config.pod_name();
        assert!(pod_name.starts_with("anode-eval-"));
        assert!(pod_name.contains("12345678"));
    }

    #[test]
    fn test_build_agent_pod() {
        let mut api_keys = BTreeMap::new();
        api_keys.insert("ANTHROPIC_API_KEY".to_string(), "test-key".to_string());

        let config = AgentPodConfig {
            agent: presets::claude_opus_45(),
            prompt: "Write a hello world".to_string(),
            eval_path: "/evals/hello".to_string(),
            run_id: "12345678-abcd-1234-abcd-123456789abc".to_string(),
            namespace: "default".to_string(),
            timeout_hours: 6,
            api_keys,
            test_command: "cargo".to_string(),
            test_args: vec!["test".to_string()],
            git_repo: None,
            setup_commands: vec![],
        };

        let pod = build_agent_pod(&config);

        assert!(pod.metadata.name.is_some());
        assert_eq!(pod.metadata.namespace, Some("default".to_string()));
        assert!(pod.spec.is_some());

        let spec = pod.spec.unwrap();
        assert_eq!(spec.containers.len(), 1);
        assert_eq!(spec.active_deadline_seconds, Some(21600)); // 6 hours
    }
}
