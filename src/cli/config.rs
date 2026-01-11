use crate::agents::{AgentConfig, AgentTool, ModelVersion};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Configuration for an evaluation run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalConfig {
    /// Name of this evaluation
    pub name: String,

    /// Description of the evaluation
    #[serde(default)]
    pub description: String,

    /// List of prompts to evaluate
    pub prompts: Vec<PromptConfig>,

    /// List of agents to use
    pub agents: Vec<AgentConfig>,

    /// Global settings
    #[serde(default)]
    pub settings: EvalSettings,
}

/// Configuration for a single prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptConfig {
    /// Unique identifier for this prompt
    pub id: String,

    /// The prompt text
    pub prompt: String,

    /// Path to the evaluation tests directory
    pub eval_path: PathBuf,

    /// Test harness to use
    pub test_harness: TestHarness,

    /// Optional setup commands to run before the agent
    #[serde(default)]
    pub setup_commands: Vec<String>,

    /// Optional timeout override in hours
    pub timeout_hours: Option<u32>,
}

/// Supported test harnesses
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestHarness {
    /// Rust cargo test
    Cargo {
        /// Cargo features to enable
        #[serde(default)]
        features: Vec<String>,
        /// Run with release mode
        #[serde(default)]
        release: bool,
    },
    /// Node.js npm test
    Npm {
        /// Test script name (default: "test")
        #[serde(default = "default_npm_script")]
        script: String,
    },
    /// Python pytest
    Pytest {
        /// Extra pytest arguments
        #[serde(default)]
        args: Vec<String>,
    },
    /// Go test
    Go {
        /// Package path
        #[serde(default = "default_go_package")]
        package: String,
    },
    /// Custom command
    Custom {
        /// Command to run
        command: String,
        /// Arguments
        #[serde(default)]
        args: Vec<String>,
    },
}

fn default_npm_script() -> String {
    "test".to_string()
}

fn default_go_package() -> String {
    "./...".to_string()
}

impl TestHarness {
    /// Get the command to run tests
    pub fn test_command(&self) -> (String, Vec<String>) {
        match self {
            TestHarness::Cargo { features, release } => {
                let mut args = vec!["test".to_string()];
                if !features.is_empty() {
                    args.push("--features".to_string());
                    args.push(features.join(","));
                }
                if *release {
                    args.push("--release".to_string());
                }
                // Don't use --format=json as it requires nightly
                ("cargo".to_string(), args)
            }
            TestHarness::Npm { script } => {
                ("npm".to_string(), vec!["run".to_string(), script.clone()])
            }
            TestHarness::Pytest { args } => {
                let mut full_args = vec!["-v".to_string(), "--tb=short".to_string()];
                full_args.extend(args.clone());
                ("pytest".to_string(), full_args)
            }
            TestHarness::Go { package } => (
                "go".to_string(),
                vec!["test".to_string(), "-v".to_string(), package.clone()],
            ),
            TestHarness::Custom { command, args } => (command.clone(), args.clone()),
        }
    }
}

/// Global evaluation settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalSettings {
    /// Default timeout in hours
    #[serde(default = "default_timeout")]
    pub default_timeout_hours: u32,

    /// Output directory for results
    #[serde(default = "default_output_dir")]
    pub output_dir: PathBuf,

    /// Number of iterations for ralph-loop style execution
    #[serde(default = "default_iterations")]
    pub default_iterations: u32,

    /// Whether to clean up pods after completion
    #[serde(default = "default_cleanup")]
    pub cleanup_on_complete: bool,

    /// API keys configuration
    #[serde(default)]
    pub api_keys: ApiKeysConfig,
}

impl Default for EvalSettings {
    fn default() -> Self {
        Self {
            default_timeout_hours: default_timeout(),
            output_dir: default_output_dir(),
            default_iterations: default_iterations(),
            cleanup_on_complete: default_cleanup(),
            api_keys: ApiKeysConfig::default(),
        }
    }
}

fn default_timeout() -> u32 {
    6
}

fn default_output_dir() -> PathBuf {
    PathBuf::from("./eval-results")
}

fn default_iterations() -> u32 {
    10
}

fn default_cleanup() -> bool {
    true
}

/// API keys configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiKeysConfig {
    /// Environment variable names for API keys
    /// The values will be read from the environment
    #[serde(default)]
    pub env_vars: Vec<String>,

    /// Direct key-value pairs (not recommended, prefer env_vars)
    #[serde(default)]
    pub direct: BTreeMap<String, String>,
}

impl ApiKeysConfig {
    /// Resolve all API keys from environment and direct config
    pub fn resolve(&self) -> Result<BTreeMap<String, String>> {
        let mut keys = self.direct.clone();

        for var_name in &self.env_vars {
            if let Ok(value) = std::env::var(var_name) {
                keys.insert(var_name.clone(), value);
            } else {
                tracing::warn!("Environment variable {} not set", var_name);
            }
        }

        Ok(keys)
    }
}

impl EvalConfig {
    /// Load configuration from a YAML file
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .context(format!("Failed to read config file: {:?}", path.as_ref()))?;

        let config: EvalConfig =
            serde_yaml::from_str(&content).context("Failed to parse config file")?;

        Ok(config)
    }

    /// Save configuration to a YAML file
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_yaml::to_string(self).context("Failed to serialize config")?;
        std::fs::write(path.as_ref(), content)
            .context(format!("Failed to write config file: {:?}", path.as_ref()))?;
        Ok(())
    }

    /// Generate all (prompt, agent) combinations for this evaluation
    pub fn combinations(&self) -> Vec<(PromptConfig, AgentConfig)> {
        let mut result = Vec::new();
        for prompt in &self.prompts {
            for agent in &self.agents {
                result.push((prompt.clone(), agent.clone()));
            }
        }
        result
    }

    /// Generate a sample configuration
    pub fn sample() -> Self {
        Self {
            name: "Sample Evaluation".to_string(),
            description: "A sample evaluation configuration".to_string(),
            prompts: vec![PromptConfig {
                id: "hello-world".to_string(),
                prompt: "Create a function that returns 'Hello, World!' and write tests for it."
                    .to_string(),
                eval_path: PathBuf::from("./evals/hello-world"),
                test_harness: TestHarness::Cargo {
                    features: vec![],
                    release: false,
                },
                setup_commands: vec![],
                timeout_hours: None,
            }],
            agents: vec![
                AgentConfig {
                    tool: AgentTool::ClaudeCode,
                    model: ModelVersion::ClaudeOpus45,
                    iterations: 10,
                },
                AgentConfig {
                    tool: AgentTool::Codex,
                    model: ModelVersion::Gpt52XHigh,
                    iterations: 10,
                },
            ],
            settings: EvalSettings {
                api_keys: ApiKeysConfig {
                    env_vars: vec![
                        "ANTHROPIC_API_KEY".to_string(),
                        "OPENAI_API_KEY".to_string(),
                    ],
                    ..Default::default()
                },
                ..Default::default()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_config() {
        let config = EvalConfig::sample();
        assert_eq!(config.prompts.len(), 1);
        assert_eq!(config.agents.len(), 2);
    }

    #[test]
    fn test_combinations() {
        let config = EvalConfig::sample();
        let combos = config.combinations();
        assert_eq!(combos.len(), 2); // 1 prompt * 2 agents
    }

    #[test]
    fn test_cargo_test_command() {
        let harness = TestHarness::Cargo {
            features: vec!["feature1".to_string()],
            release: true,
        };
        let (cmd, args) = harness.test_command();
        assert_eq!(cmd, "cargo");
        assert!(args.contains(&"test".to_string()));
        assert!(args.contains(&"--release".to_string()));
    }

    #[test]
    fn test_serialize_deserialize() {
        let config = EvalConfig::sample();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let parsed: EvalConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.name, config.name);
    }
}
