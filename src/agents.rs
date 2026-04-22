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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TerminalLaunchMode {
    #[default]
    RawShell,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TerminalSessionKind {
    ClaudeSession,
    CursorChat,
    CodexSession,
    PiSession,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum TerminalRestoreStatus {
    #[default]
    NotStarted,
    Launching,
    Ready,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub(crate) struct TerminalSessionRef {
    pub kind: TerminalSessionKind,
    pub id: String,
}

pub(crate) const AGENTS: &[AgentDef] = &[
    AgentDef {
        id: "claude-code",
        label: "Claude Code",
        icon: "assets/agent-icons/claude.png",
        provider: Some(AgentProviderKind::ClaudeCode),
    },
    AgentDef {
        id: "codex",
        label: "Codex",
        icon: "assets/agent-icons/openai.svg",
        provider: Some(AgentProviderKind::Codex),
    },
    AgentDef {
        id: "cursor",
        label: "Cursor Agent",
        icon: "assets/agent-icons/cursor.svg",
        provider: Some(AgentProviderKind::CursorAgent),
    },
    AgentDef {
        id: "gemini",
        label: "Gemini",
        icon: "assets/agent-icons/gemini.png",
        provider: Some(AgentProviderKind::Gemini),
    },
    AgentDef {
        id: "pi",
        label: "Pi",
        icon: "assets/agent-icons/pi.png",
        provider: Some(AgentProviderKind::Pi),
    },
    AgentDef {
        id: "opencode",
        label: "OpenCode",
        icon: "assets/agent-icons/opencode.png",
        provider: Some(AgentProviderKind::OpenCode),
    },
    AgentDef {
        id: "amp",
        label: "Amp",
        icon: "assets/agent-icons/ampcode.png",
        provider: Some(AgentProviderKind::Amp),
    },
    AgentDef {
        id: "rovo-dev",
        label: "Rovo Dev",
        icon: "assets/agent-icons/atlassian.png",
        provider: Some(AgentProviderKind::RovoDev),
    },
    AgentDef {
        id: "forge",
        label: "Forge",
        icon: "assets/agent-icons/forge.svg",
        provider: Some(AgentProviderKind::Forge),
    },
];

pub(crate) const DEFAULT_AGENT_ID: &str = "pi";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TerminalLaunchConfig {
    pub mode: TerminalLaunchMode,
    pub provider: Option<AgentProviderKind>,
    pub session: Option<TerminalSessionRef>,
}

impl TerminalLaunchConfig {
    pub fn for_provider(provider: AgentProviderKind) -> Self {
        Self {
            mode: TerminalLaunchMode::Agent,
            provider: Some(provider),
            session: None,
        }
    }

    pub fn with_session(mut self, session: Option<TerminalSessionRef>) -> Self {
        self.session = session;
        self
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

pub(crate) fn terminal_launch_config_for_selected_agent(
    selected_agent_id: Option<&str>,
) -> Option<TerminalLaunchConfig> {
    match selected_agent_id {
        Some(selected_agent_id) => AGENTS
            .iter()
            .find(|agent| agent.id == selected_agent_id)
            .and_then(|agent| agent.provider)
            .map(TerminalLaunchConfig::for_provider),
        None => Some(TerminalLaunchConfig::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        terminal_launch_config_for_selected_agent, terminal_launch_config_for_selected_agents,
        AgentProviderKind, TerminalLaunchConfig, TerminalLaunchMode,
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
        assert_eq!(config.mode, TerminalLaunchMode::RawShell);
        assert_eq!(config.default_title(), "Terminal");
    }

    #[test]
    fn cursor_preset_uses_cursor_agent_provider() {
        let config =
            terminal_launch_config_for_selected_agents(&HashSet::from(["cursor".to_string()]));

        assert_eq!(config.provider, Some(AgentProviderKind::CursorAgent));
        assert_eq!(config.mode, TerminalLaunchMode::Agent);
    }

    #[test]
    fn selected_agent_helper_returns_raw_shell_for_cli_only() {
        let config = terminal_launch_config_for_selected_agent(None);

        assert_eq!(config, Some(TerminalLaunchConfig::default()));
    }

    #[test]
    fn selected_agent_helper_rejects_unknown_agent() {
        let config = terminal_launch_config_for_selected_agent(Some("missing"));

        assert_eq!(config, None);
    }
}
