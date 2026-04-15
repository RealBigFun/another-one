use std::collections::HashSet;

pub(crate) struct AgentDef {
    pub id: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub startup_argv: &'static [&'static str],
}

pub(crate) const AGENTS: &[AgentDef] = &[
    AgentDef {
        id: "claude-code",
        label: "Claude Code",
        icon: "assets/icons/icons__claude-ai.svg",
        startup_argv: &["claude"],
    },
    AgentDef {
        id: "codex",
        label: "Codex",
        icon: "assets/icons/icons__codex-ai.svg",
        startup_argv: &["codex"],
    },
    AgentDef {
        id: "cursor",
        label: "Cursor",
        icon: "assets/icons/icons__cursor-ai.svg",
        startup_argv: &["cursor"],
    },
    AgentDef {
        id: "gemini",
        label: "Gemini",
        icon: "assets/icons/icons__gemini-ai.svg",
        startup_argv: &["gemini"],
    },
    AgentDef {
        id: "pi",
        label: "Pi",
        icon: "assets/icons/icons__kiro-ai.svg",
        startup_argv: &["pi"],
    },
    AgentDef {
        id: "opencode",
        label: "OpenCode",
        icon: "assets/icons/icons__brain.svg",
        startup_argv: &["opencode"],
    },
    AgentDef {
        id: "amp",
        label: "Amp",
        icon: "assets/icons/icons__brain.svg",
        startup_argv: &["amp"],
    },
    AgentDef {
        id: "rovo-dev",
        label: "Rovo Dev",
        icon: "assets/icons/icons__brain.svg",
        startup_argv: &["rovo-dev"],
    },
    AgentDef {
        id: "forge",
        label: "Forge",
        icon: "assets/icons/icons__brain.svg",
        startup_argv: &["forge"],
    },
];

pub(crate) const DEFAULT_AGENT_ID: &str = "pi";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalLaunchConfig {
    pub startup_argv: Option<Vec<String>>,
}

impl TerminalLaunchConfig {
    pub fn startup_command_line(&self) -> Option<String> {
        self.startup_argv
            .as_ref()
            .filter(|argv| !argv.is_empty())
            .map(|argv| shell_join(argv))
    }
}

pub(crate) fn terminal_launch_config_for_selected_agents(
    selected_agents: &HashSet<String>,
) -> TerminalLaunchConfig {
    let startup_argv = AGENTS
        .iter()
        .find(|agent| selected_agents.contains(agent.id))
        .filter(|agent| !agent.startup_argv.is_empty())
        .map(|agent| {
            agent
                .startup_argv
                .iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>()
        });

    TerminalLaunchConfig { startup_argv }
}

fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| shell_quote(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }

    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
    {
        return arg.to_string();
    }

    format!("'{}'", arg.replace('\'', r#"'"'"'"#))
}

#[cfg(test)]
mod tests {
    use super::{shell_quote, terminal_launch_config_for_selected_agents, TerminalLaunchConfig};
    use std::collections::HashSet;

    #[test]
    fn selected_agents_use_configured_display_order() {
        let selected = HashSet::from(["codex".to_string(), "claude-code".to_string()]);

        let config = terminal_launch_config_for_selected_agents(&selected);

        assert_eq!(config.startup_argv, Some(vec!["claude".to_string()]));
    }

    #[test]
    fn empty_selection_produces_plain_shell_launch() {
        let config = terminal_launch_config_for_selected_agents(&HashSet::new());

        assert_eq!(config, TerminalLaunchConfig::default());
        assert_eq!(config.startup_command_line(), None);
    }

    #[test]
    fn startup_command_line_shell_quotes_arguments() {
        let config = TerminalLaunchConfig {
            startup_argv: Some(vec![
                "codex".to_string(),
                "--prompt".to_string(),
                "agent's choice".to_string(),
            ]),
        };

        assert_eq!(
            config.startup_command_line().as_deref(),
            Some("codex --prompt 'agent'\"'\"'s choice'")
        );
    }

    #[test]
    fn shell_quote_leaves_safe_tokens_unquoted() {
        assert_eq!(shell_quote("pi"), "pi");
        assert_eq!(shell_quote("foo/bar-baz"), "foo/bar-baz");
    }
}
