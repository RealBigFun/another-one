use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Output};

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

    pub fn default_launch_argv(self) -> Vec<String> {
        match self {
            Self::ClaudeCode => vec!["claude".to_string()],
            Self::CursorAgent => vec!["agent".to_string()],
            Self::Codex => vec!["codex".to_string()],
            Self::Pi => vec!["pi".to_string()],
            Self::Gemini => vec!["gemini".to_string()],
            Self::OpenCode => vec!["opencode".to_string()],
            Self::Amp => vec!["amp".to_string()],
            Self::RovoDev => vec!["rovo-dev".to_string()],
            Self::Forge => vec!["forge".to_string()],
        }
    }

    pub fn terminal_kind(self) -> TerminalLaunchKind {
        if self.supports_resume() {
            TerminalLaunchKind::Agent
        } else {
            TerminalLaunchKind::Shell
        }
    }

    pub fn supports_resume(self) -> bool {
        matches!(
            self,
            Self::ClaudeCode | Self::CursorAgent | Self::Codex | Self::Pi
        )
    }

    pub fn is_discovery_based(self) -> bool {
        matches!(self, Self::Codex | Self::Pi)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum TerminalLaunchKind {
    #[default]
    Shell,
    Agent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ResumeMode {
    #[default]
    Fresh,
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ResumeTargetKind {
    Id,
    Path,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ResumeTarget {
    pub kind: ResumeTargetKind,
    pub value: String,
}

impl ResumeTarget {
    pub fn id(value: impl Into<String>) -> Self {
        Self {
            kind: ResumeTargetKind::Id,
            value: value.into(),
        }
    }

    pub fn path(value: impl Into<String>) -> Self {
        Self {
            kind: ResumeTargetKind::Path,
            value: value.into(),
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalLaunchConfig {
    pub kind: TerminalLaunchKind,
    pub provider: Option<AgentProviderKind>,
    pub launch_argv: Vec<String>,
    pub resume_target: Option<ResumeTarget>,
    pub resume_mode: ResumeMode,
}

impl Default for TerminalLaunchConfig {
    fn default() -> Self {
        Self {
            kind: TerminalLaunchKind::Shell,
            provider: None,
            launch_argv: Vec::new(),
            resume_target: None,
            resume_mode: ResumeMode::Fresh,
        }
    }
}

impl TerminalLaunchConfig {
    pub fn for_provider(provider: AgentProviderKind) -> Self {
        Self {
            kind: provider.terminal_kind(),
            provider: Some(provider),
            launch_argv: provider.default_launch_argv(),
            resume_target: None,
            resume_mode: ResumeMode::Fresh,
        }
    }

    pub fn default_title(&self) -> String {
        self.provider
            .map(AgentProviderKind::label)
            .unwrap_or("Terminal")
            .to_string()
    }

    pub fn with_resume_target(&self, resume_target: ResumeTarget) -> Self {
        let mut next = self.clone();
        next.resume_target = Some(resume_target);
        next.resume_mode = ResumeMode::Resume;
        next
    }

    pub fn fresh_clone_for_new_tab(&self) -> Self {
        let mut next = self.clone();
        next.resume_target = None;
        next.resume_mode = ResumeMode::Fresh;
        next.launch_argv = next
            .provider
            .map(AgentProviderKind::default_launch_argv)
            .unwrap_or_else(|| self.launch_argv.clone());
        next
    }

    pub fn shell_startup_command_line(&self) -> Option<String> {
        if self.kind != TerminalLaunchKind::Shell || self.launch_argv.is_empty() {
            return None;
        }

        Some(shell_join(&self.launch_argv))
    }

    pub fn persisted_launch_argv(&self) -> Vec<String> {
        if self.kind == TerminalLaunchKind::Agent {
            self.launch_argv.clone()
        } else {
            Vec::new()
        }
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

pub(crate) fn prepare_launch_config_for_spawn(
    launch_config: &TerminalLaunchConfig,
    cwd: Option<&Path>,
) -> Result<TerminalLaunchConfig, String> {
    let mut prepared = launch_config.clone();

    match (prepared.kind, prepared.provider, prepared.resume_mode) {
        (TerminalLaunchKind::Shell, Some(provider), ResumeMode::Fresh) => {
            if prepared.launch_argv.is_empty() {
                prepared.launch_argv = provider.default_launch_argv();
            }
            Ok(prepared)
        }
        (TerminalLaunchKind::Shell, _, _) => Ok(prepared),
        (TerminalLaunchKind::Agent, Some(AgentProviderKind::ClaudeCode), ResumeMode::Fresh) => {
            let resume_target = prepared
                .resume_target
                .clone()
                .unwrap_or_else(|| ResumeTarget::id(uuid::Uuid::new_v4().to_string()));
            prepared.resume_target = Some(resume_target.clone());
            prepared.launch_argv = vec![
                "claude".to_string(),
                "--session-id".to_string(),
                resume_target.value,
            ];
            Ok(prepared)
        }
        (TerminalLaunchKind::Agent, Some(AgentProviderKind::CursorAgent), ResumeMode::Fresh) => {
            let chat_id = create_cursor_chat(cwd)?;
            prepared.resume_target = Some(ResumeTarget::id(chat_id.clone()));
            prepared.resume_mode = ResumeMode::Resume;
            prepared.launch_argv = vec!["agent".to_string(), "--resume".to_string(), chat_id];
            Ok(prepared)
        }
        (TerminalLaunchKind::Agent, Some(provider), ResumeMode::Fresh) => {
            if prepared.launch_argv.is_empty() {
                prepared.launch_argv = provider.default_launch_argv();
            }
            Ok(prepared)
        }
        (TerminalLaunchKind::Agent, Some(provider), ResumeMode::Resume) => {
            let resume_target = prepared.resume_target.as_ref().ok_or_else(|| {
                format!(
                    "{} could not resume because no saved target was found.",
                    provider.label()
                )
            })?;
            prepared.launch_argv = match provider {
                AgentProviderKind::ClaudeCode => {
                    vec![
                        "claude".to_string(),
                        "--resume".to_string(),
                        resume_target.value.clone(),
                    ]
                }
                AgentProviderKind::CursorAgent => {
                    vec![
                        "agent".to_string(),
                        "--resume".to_string(),
                        resume_target.value.clone(),
                    ]
                }
                AgentProviderKind::Codex => {
                    vec![
                        "codex".to_string(),
                        "resume".to_string(),
                        resume_target.value.clone(),
                    ]
                }
                AgentProviderKind::Pi => {
                    vec![
                        "pi".to_string(),
                        "--session".to_string(),
                        resume_target.value.clone(),
                    ]
                }
                unsupported => {
                    return Err(format!(
                        "{} does not support resume launches yet.",
                        unsupported.label()
                    ));
                }
            };
            Ok(prepared)
        }
        (TerminalLaunchKind::Agent, None, _) => {
            Err("Could not start the tab because its provider is missing.".to_string())
        }
    }
}

fn create_cursor_chat(cwd: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new("agent");
    command.arg("create-chat");
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let output = command
        .output()
        .map_err(|error| format!("Could not start Cursor Agent: {error}"))?;

    if !output.status.success() {
        return Err(command_failure("Could not start Cursor Agent", &output));
    }

    let chat_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if chat_id.is_empty() {
        return Err("Cursor Agent did not return a resumable chat ID for the new tab.".to_string());
    }

    Ok(chat_id)
}

fn command_failure(prefix: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "No additional details were reported.".to_string()
    };
    format!("{prefix}. {detail}")
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
    use super::{
        prepare_launch_config_for_spawn, shell_quote, terminal_launch_config_for_selected_agents,
        AgentProviderKind, ResumeMode, ResumeTarget, TerminalLaunchConfig, TerminalLaunchKind,
    };
    use std::collections::HashSet;

    #[test]
    fn selected_agents_use_configured_display_order() {
        let selected = HashSet::from(["codex".to_string(), "claude-code".to_string()]);

        let config = terminal_launch_config_for_selected_agents(&selected);

        assert_eq!(config.provider, Some(AgentProviderKind::ClaudeCode));
        assert_eq!(config.kind, TerminalLaunchKind::Agent);
        assert_eq!(config.launch_argv, vec!["claude".to_string()]);
    }

    #[test]
    fn empty_selection_produces_plain_shell_launch() {
        let config = terminal_launch_config_for_selected_agents(&HashSet::new());

        assert_eq!(config, TerminalLaunchConfig::default());
        assert_eq!(config.shell_startup_command_line(), None);
    }

    #[test]
    fn shell_command_line_shell_quotes_arguments() {
        let config = TerminalLaunchConfig {
            kind: TerminalLaunchKind::Shell,
            provider: Some(AgentProviderKind::Gemini),
            launch_argv: vec![
                "gemini".to_string(),
                "--prompt".to_string(),
                "agent's choice".to_string(),
            ],
            resume_target: None,
            resume_mode: ResumeMode::Fresh,
        };

        assert_eq!(
            config.shell_startup_command_line().as_deref(),
            Some("gemini --prompt 'agent'\"'\"'s choice'")
        );
    }

    #[test]
    fn shell_quote_leaves_safe_tokens_unquoted() {
        assert_eq!(shell_quote("pi"), "pi");
        assert_eq!(shell_quote("foo/bar-baz"), "foo/bar-baz");
    }

    #[test]
    fn claude_fresh_launch_prepares_session_id() {
        let prepared = prepare_launch_config_for_spawn(
            &TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode),
            None,
        )
        .expect("claude launch should prepare");

        assert!(prepared.resume_target.is_some());
        assert_eq!(prepared.launch_argv[0], "claude");
        assert_eq!(prepared.launch_argv[1], "--session-id");
    }

    #[test]
    fn codex_resume_launch_uses_resume_subcommand() {
        let prepared = prepare_launch_config_for_spawn(
            &TerminalLaunchConfig {
                kind: TerminalLaunchKind::Agent,
                provider: Some(AgentProviderKind::Codex),
                launch_argv: vec!["codex".to_string()],
                resume_target: Some(ResumeTarget::id("session-123")),
                resume_mode: ResumeMode::Resume,
            },
            None,
        )
        .expect("codex resume should prepare");

        assert_eq!(
            prepared.launch_argv,
            vec![
                "codex".to_string(),
                "resume".to_string(),
                "session-123".to_string(),
            ]
        );
    }

    #[test]
    fn claude_resume_launch_uses_resume_flag() {
        let prepared = prepare_launch_config_for_spawn(
            &TerminalLaunchConfig {
                kind: TerminalLaunchKind::Agent,
                provider: Some(AgentProviderKind::ClaudeCode),
                launch_argv: vec!["claude".to_string()],
                resume_target: Some(ResumeTarget::id("session-123")),
                resume_mode: ResumeMode::Resume,
            },
            None,
        )
        .expect("claude resume should prepare");

        assert_eq!(
            prepared.launch_argv,
            vec![
                "claude".to_string(),
                "--resume".to_string(),
                "session-123".to_string(),
            ]
        );
    }

    #[test]
    fn cursor_resume_launch_uses_resume_flag() {
        let prepared = prepare_launch_config_for_spawn(
            &TerminalLaunchConfig {
                kind: TerminalLaunchKind::Agent,
                provider: Some(AgentProviderKind::CursorAgent),
                launch_argv: vec!["agent".to_string()],
                resume_target: Some(ResumeTarget::id("chat-123")),
                resume_mode: ResumeMode::Resume,
            },
            None,
        )
        .expect("cursor resume should prepare");

        assert_eq!(
            prepared.launch_argv,
            vec![
                "agent".to_string(),
                "--resume".to_string(),
                "chat-123".to_string(),
            ]
        );
    }

    #[test]
    fn pi_resume_launch_uses_session_path() {
        let prepared = prepare_launch_config_for_spawn(
            &TerminalLaunchConfig {
                kind: TerminalLaunchKind::Agent,
                provider: Some(AgentProviderKind::Pi),
                launch_argv: vec!["pi".to_string()],
                resume_target: Some(ResumeTarget::path("/tmp/pi-session.jsonl")),
                resume_mode: ResumeMode::Resume,
            },
            None,
        )
        .expect("pi resume should prepare");

        assert_eq!(
            prepared.launch_argv,
            vec![
                "pi".to_string(),
                "--session".to_string(),
                "/tmp/pi-session.jsonl".to_string(),
            ]
        );
    }

    #[test]
    fn cursor_preset_uses_cursor_agent_cli() {
        let config =
            terminal_launch_config_for_selected_agents(&HashSet::from(["cursor".to_string()]));

        assert_eq!(config.provider, Some(AgentProviderKind::CursorAgent));
        assert_eq!(config.kind, TerminalLaunchKind::Agent);
        assert_eq!(config.launch_argv, vec!["agent".to_string()]);
    }
}
