use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub(crate) struct AgentDef {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub provider: Option<AgentProviderKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub(crate) enum AgentProviderKind {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "cursor-agent")]
    CursorAgent,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "pi")]
    Pi,
    #[serde(rename = "gemini")]
    Gemini,
    #[serde(rename = "opencode")]
    OpenCode,
    #[serde(rename = "amp")]
    Amp,
    #[serde(rename = "rovo-dev")]
    RovoDev,
    #[serde(rename = "forge")]
    Forge,
}

impl AgentProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::CursorAgent => "Cursor Agent",
            Self::Codex => "Codex",
            Self::Pi => "Pi",
            Self::Gemini => "Gemini",
            Self::OpenCode => "OpenCode",
            Self::Amp => "Amp",
            Self::RovoDev => "Rovo Dev",
            Self::Forge => "Forge",
        }
    }
}

pub(crate) const AGENTS: &[AgentDef] = &[
    AgentDef {
        id: "claude-code",
        label: "Claude Code",
        icon: "assets/icons/icons__claude-ai.svg",
        provider: Some(AgentProviderKind::ClaudeCode),
    },
    AgentDef {
        id: "codex",
        label: "Codex",
        icon: "assets/icons/icons__codex-ai.svg",
        provider: Some(AgentProviderKind::Codex),
    },
    AgentDef {
        id: "cursor",
        label: "Cursor Agent",
        icon: "assets/icons/icons__cursor-ai.svg",
        provider: Some(AgentProviderKind::CursorAgent),
    },
    AgentDef {
        id: "gemini",
        label: "Gemini",
        icon: "assets/icons/icons__gemini-ai.svg",
        provider: Some(AgentProviderKind::Gemini),
    },
    AgentDef {
        id: "pi",
        label: "Pi",
        icon: "assets/icons/icons__kiro-ai.svg",
        provider: Some(AgentProviderKind::Pi),
    },
    AgentDef {
        id: "opencode",
        label: "OpenCode",
        icon: "assets/icons/icons__brain.svg",
        provider: Some(AgentProviderKind::OpenCode),
    },
    AgentDef {
        id: "amp",
        label: "Amp",
        icon: "assets/icons/icons__brain.svg",
        provider: Some(AgentProviderKind::Amp),
    },
    AgentDef {
        id: "rovo-dev",
        label: "Rovo Dev",
        icon: "assets/icons/icons__brain.svg",
        provider: Some(AgentProviderKind::RovoDev),
    },
    AgentDef {
        id: "forge",
        label: "Forge",
        icon: "assets/icons/icons__brain.svg",
        provider: Some(AgentProviderKind::Forge),
    },
];

pub(crate) const DEFAULT_AGENT_ID: &str = "pi";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalLaunchConfig {
    pub provider: Option<AgentProviderKind>,
}

impl TerminalLaunchConfig {
    pub fn for_provider(provider: AgentProviderKind) -> Self {
        Self {
            provider: Some(provider),
        }
    }

    pub fn default_title(&self) -> String {
        self.provider
            .map(AgentProviderKind::label)
            .unwrap_or("Terminal")
            .to_string()
    }
}

pub(crate) fn terminal_launch_config_for_selected_agents(
    selected_agents: &HashSet<String>,
) -> TerminalLaunchConfig {
    AGENTS
        .iter()
        .find(|agent| selected_agents.contains(agent.id))
        .and_then(|agent| agent.provider)
        .map(TerminalLaunchConfig::for_provider)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        terminal_launch_config_for_selected_agents, AgentProviderKind, TerminalLaunchConfig,
    };
    use std::collections::HashSet;

    #[test]
    fn selected_agents_use_configured_display_order() {
        let selected = HashSet::from(["codex".to_string(), "claude-code".to_string()]);

        let config = terminal_launch_config_for_selected_agents(&selected);

        assert_eq!(config.provider, Some(AgentProviderKind::ClaudeCode));
    }

    #[test]
    fn empty_selection_produces_default_placeholder_tab() {
        let config = terminal_launch_config_for_selected_agents(&HashSet::new());

        assert_eq!(config, TerminalLaunchConfig::default());
        assert_eq!(config.default_title(), "Terminal");
    }

    #[test]
    fn cursor_preset_uses_cursor_agent_provider() {
        let config =
            terminal_launch_config_for_selected_agents(&HashSet::from(["cursor".to_string()]));

        assert_eq!(config.provider, Some(AgentProviderKind::CursorAgent));
    }
}
