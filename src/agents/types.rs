use serde::{Deserialize, Serialize};
use std::fmt;

/// Supported agent CLI tools
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentTool {
    /// Claude Code CLI
    ClaudeCode,
    /// OpenAI Codex CLI
    Codex,
    /// OpenCode CLI
    OpenCode,
}

impl fmt::Display for AgentTool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentTool::ClaudeCode => write!(f, "claude-code"),
            AgentTool::Codex => write!(f, "codex"),
            AgentTool::OpenCode => write!(f, "opencode"),
        }
    }
}

impl AgentTool {
    /// Get the CLI command name for this agent
    pub fn cli_command(&self) -> &'static str {
        match self {
            AgentTool::ClaudeCode => "claude",
            AgentTool::Codex => "codex",
            AgentTool::OpenCode => "opencode",
        }
    }

    /// Get the required environment variable for API key
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            AgentTool::ClaudeCode => "ANTHROPIC_API_KEY",
            AgentTool::Codex => "OPENAI_API_KEY",
            AgentTool::OpenCode => "OPENAI_API_KEY",
        }
    }

    /// Get the install command for this agent CLI
    pub fn install_command(&self) -> &'static str {
        match self {
            AgentTool::ClaudeCode => "npm install -g @anthropic-ai/claude-code",
            AgentTool::Codex => "npm install -g @openai/codex",
            AgentTool::OpenCode => "npm install -g opencode",
        }
    }
}

/// Supported model versions
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelVersion {
    /// Claude Opus 4.5 (latest)
    #[serde(rename = "claude_opus_4_5")]
    ClaudeOpus45,
    /// Claude Sonnet 4
    #[serde(rename = "claude_sonnet_4")]
    ClaudeSonnet4,
    /// GPT-5.2 Extra High
    #[serde(rename = "gpt_5_2_xhigh")]
    Gpt52XHigh,
    /// GPT-5.2 High
    #[serde(rename = "gpt_5_2_high")]
    Gpt52High,
    /// GPT-5
    #[serde(rename = "gpt_5")]
    Gpt5,
    /// o3 reasoning model
    #[serde(rename = "o3")]
    O3,
    /// Qwen Coder 8B
    #[serde(rename = "qwen_coder_8b")]
    QwenCoder8b,
    /// Custom model string
    Custom(String),
}

impl fmt::Display for ModelVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelVersion::ClaudeOpus45 => write!(f, "claude-opus-4-5-20251101"),
            ModelVersion::ClaudeSonnet4 => write!(f, "claude-sonnet-4-20250514"),
            ModelVersion::Gpt52XHigh => write!(f, "gpt-5.2-xhigh"),
            ModelVersion::Gpt52High => write!(f, "gpt-5.2-high"),
            ModelVersion::Gpt5 => write!(f, "gpt-5"),
            ModelVersion::O3 => write!(f, "o3"),
            ModelVersion::QwenCoder8b => write!(f, "qwen2.5-coder:7b"),
            ModelVersion::Custom(s) => write!(f, "{}", s),
        }
    }
}

/// An agent configuration combining tool and model
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentConfig {
    pub tool: AgentTool,
    pub model: ModelVersion,
    /// Number of iterations for ralph-loop style execution
    #[serde(default = "default_iterations")]
    pub iterations: u32,
}

fn default_iterations() -> u32 {
    10
}

impl AgentConfig {
    pub fn new(tool: AgentTool, model: ModelVersion) -> Self {
        Self {
            tool,
            model,
            iterations: default_iterations(),
        }
    }

    pub fn with_iterations(mut self, iterations: u32) -> Self {
        self.iterations = iterations;
        self
    }

    /// Get a unique identifier for this agent config
    pub fn id(&self) -> String {
        format!("{}-{}", self.tool, self.model)
    }
}

/// Predefined agent configurations
pub mod presets {
    use super::*;

    pub fn claude_opus_45() -> AgentConfig {
        AgentConfig::new(AgentTool::ClaudeCode, ModelVersion::ClaudeOpus45)
    }

    pub fn claude_sonnet_4() -> AgentConfig {
        AgentConfig::new(AgentTool::ClaudeCode, ModelVersion::ClaudeSonnet4)
    }

    pub fn codex_gpt52_xhigh() -> AgentConfig {
        AgentConfig::new(AgentTool::Codex, ModelVersion::Gpt52XHigh)
    }

    pub fn codex_gpt52_high() -> AgentConfig {
        AgentConfig::new(AgentTool::Codex, ModelVersion::Gpt52High)
    }

    pub fn codex_o3() -> AgentConfig {
        AgentConfig::new(AgentTool::Codex, ModelVersion::O3)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_tool_display() {
        assert_eq!(AgentTool::ClaudeCode.to_string(), "claude-code");
        assert_eq!(AgentTool::Codex.to_string(), "codex");
    }

    #[test]
    fn test_model_version_display() {
        assert_eq!(
            ModelVersion::ClaudeOpus45.to_string(),
            "claude-opus-4-5-20251101"
        );
        assert_eq!(ModelVersion::Gpt52XHigh.to_string(), "gpt-5.2-xhigh");
    }

    #[test]
    fn test_agent_config_id() {
        let config = presets::claude_opus_45();
        assert_eq!(config.id(), "claude-code-claude-opus-4-5-20251101");
    }
}
