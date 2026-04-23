use anyhow::Context;
use portable_pty::CommandBuilder;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use crate::terminal_launch::{
    create_cursor_chat, pi_session_capture_extension_path, prepare_pi_session_capture,
    resolve_claude_session, resolve_codex_home_override, resolve_pi_session, DiscoveryKind,
    CODEX_HOME_ENV,
};

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

#[cfg(test)]
pub(crate) const ALL_PROVIDERS: &[AgentProviderKind] = &[
    AgentProviderKind::ClaudeCode,
    AgentProviderKind::CursorAgent,
    AgentProviderKind::Codex,
    AgentProviderKind::Pi,
    AgentProviderKind::Gemini,
    AgentProviderKind::OpenCode,
    AgentProviderKind::Amp,
    AgentProviderKind::RovoDev,
    AgentProviderKind::Forge,
];

impl AgentProviderKind {
    pub fn label(self) -> &'static str {
        harness(self).label()
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
    #[serde(default)]
    pub home_override: Option<PathBuf>,
}

impl TerminalLaunchConfig {
    pub fn for_provider(provider: AgentProviderKind) -> Self {
        Self {
            mode: TerminalLaunchMode::Agent,
            provider: Some(provider),
            session: None,
            home_override: None,
        }
    }

    pub fn with_session(mut self, session: Option<TerminalSessionRef>) -> Self {
        self.session = session;
        self
    }

    pub fn with_home_override(mut self, home_override: Option<PathBuf>) -> Self {
        self.home_override = home_override;
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

pub(crate) struct CommandOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

pub(crate) trait CommandRunner: Send + Sync {
    fn run(&self, program: &str, args: &[&str], cwd: &Path) -> anyhow::Result<CommandOutput>;
}

pub(crate) struct OsCommandRunner;

impl CommandRunner for OsCommandRunner {
    fn run(&self, program: &str, args: &[&str], cwd: &Path) -> anyhow::Result<CommandOutput> {
        let output = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .with_context(|| format!("failed to execute {program}"))?;
        Ok(CommandOutput {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub(crate) struct HarnessEnv {
    pub claude_projects_root: Option<PathBuf>,
    pub pi_sessions_root: Option<PathBuf>,
    pub codex_root: Option<PathBuf>,
    pub codex_isolated_homes_root: Option<PathBuf>,
    pub pi_session_captures_root: Option<PathBuf>,
    pub command_runner: Arc<dyn CommandRunner>,
}

impl HarnessEnv {
    pub(crate) fn from_os() -> Self {
        let home = dirs::home_dir();
        let app_state = dirs::config_dir()
            .or_else(|| Some(std::env::temp_dir()))
            .map(|dir| dir.join("another-one"));
        Self {
            claude_projects_root: home.as_ref().map(|h| h.join(".claude/projects")),
            pi_sessions_root: home.as_ref().map(|h| h.join(".pi/agent/sessions")),
            codex_root: std::env::var_os(CODEX_HOME_ENV)
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
                .or_else(|| home.as_ref().map(|h| h.join(".codex"))),
            codex_isolated_homes_root: app_state.as_ref().map(|d| d.join("codex-homes")),
            pi_session_captures_root: app_state.as_ref().map(|d| d.join("pi-session-captures")),
            command_runner: Arc::new(OsCommandRunner),
        }
    }
}

pub(crate) trait AgentHarness: Send + Sync {
    /// Self-identification used by the registry exhaustiveness test to confirm
    /// that the right harness sits behind each enum variant.
    #[allow(dead_code)]
    fn provider_kind(&self) -> AgentProviderKind;
    fn label(&self) -> &'static str;
    fn command(&self) -> &'static str;

    /// Build the PTY command for this harness. The default impl covers
    /// providers whose entire CLI surface is `[command, session.id?]`
    /// (Gemini, OpenCode, Amp, RovoDev, Forge today).
    fn build_launch(
        &self,
        _env: &HarnessEnv,
        _cwd: &Path,
        launch_config: TerminalLaunchConfig,
    ) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
        let mut builder = CommandBuilder::new(self.command());
        if let Some(session) = launch_config.session.clone() {
            builder.arg(session.id);
        }
        Ok((builder, launch_config, None))
    }

    /// Did the PTY recently print something that means the session on disk is
    /// gone and we should restart fresh? Defaults to `false`; the only harness
    /// that overrides this today is Claude Code.
    fn output_indicates_missing_session(&self, _recent_output: &str) -> bool {
        false
    }
}

pub(crate) struct ClaudeCodeHarness;
pub(crate) struct CursorAgentHarness;
pub(crate) struct CodexHarness;
pub(crate) struct PiHarness;
pub(crate) struct GeminiHarness;
pub(crate) struct OpenCodeHarness;
pub(crate) struct AmpHarness;
pub(crate) struct RovoDevHarness;
pub(crate) struct ForgeHarness;

impl AgentHarness for ClaudeCodeHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::ClaudeCode
    }
    fn label(&self) -> &'static str {
        "Claude Code"
    }
    fn command(&self) -> &'static str {
        "claude"
    }
    fn build_launch(
        &self,
        env: &HarnessEnv,
        cwd: &Path,
        launch_config: TerminalLaunchConfig,
    ) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
        let (session, should_resume) = resolve_claude_session(
            cwd,
            launch_config.session.as_ref(),
            env.claude_projects_root.as_deref(),
        );
        let mut builder = CommandBuilder::new(self.command());
        if should_resume {
            builder.args(["--resume", session.id.as_str()]);
        } else {
            builder.args(["--session-id", session.id.as_str()]);
        }
        Ok((builder, launch_config.with_session(Some(session)), None))
    }
    fn output_indicates_missing_session(&self, recent_output: &str) -> bool {
        recent_output
            .to_ascii_lowercase()
            .contains("no conversation found")
    }
}

impl AgentHarness for CursorAgentHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::CursorAgent
    }
    fn label(&self) -> &'static str {
        "Cursor Agent"
    }
    fn command(&self) -> &'static str {
        "agent"
    }
    fn build_launch(
        &self,
        env: &HarnessEnv,
        cwd: &Path,
        launch_config: TerminalLaunchConfig,
    ) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
        let session = if let Some(session) = launch_config.session.clone() {
            session
        } else {
            TerminalSessionRef {
                kind: TerminalSessionKind::CursorChat,
                id: create_cursor_chat(env, cwd)?,
            }
        };
        let mut builder = CommandBuilder::new(self.command());
        builder.args(["--resume", session.id.as_str()]);
        Ok((builder, launch_config.with_session(Some(session)), None))
    }
}

impl AgentHarness for CodexHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::Codex
    }
    fn label(&self) -> &'static str {
        "Codex"
    }
    fn command(&self) -> &'static str {
        "codex"
    }
    fn build_launch(
        &self,
        env: &HarnessEnv,
        _cwd: &Path,
        launch_config: TerminalLaunchConfig,
    ) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
        let mut builder = CommandBuilder::new(self.command());
        let (launch_config, codex_home_override) = resolve_codex_home_override(env, launch_config)?;
        if let Some(codex_home_override) = codex_home_override.as_ref() {
            builder.env(
                CODEX_HOME_ENV,
                codex_home_override.to_string_lossy().into_owned(),
            );
        }
        let discovery = if let Some(session) = launch_config.session.clone() {
            builder.args(["resume", session.id.as_str()]);
            None
        } else {
            Some(DiscoveryKind::Codex {
                root: codex_home_override
                    .expect("fresh codex launches should always have an isolated home"),
            })
        };
        Ok((builder, launch_config, discovery))
    }
}

impl AgentHarness for PiHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::Pi
    }
    fn label(&self) -> &'static str {
        "Pi"
    }
    fn command(&self) -> &'static str {
        "pi"
    }
    fn build_launch(
        &self,
        env: &HarnessEnv,
        cwd: &Path,
        launch_config: TerminalLaunchConfig,
    ) -> anyhow::Result<(CommandBuilder, TerminalLaunchConfig, Option<DiscoveryKind>)> {
        let mut builder = CommandBuilder::new(self.command());
        let discovery = if let Some(session) = resolve_pi_session(
            cwd,
            launch_config.session.as_ref(),
            env.pi_sessions_root.as_deref(),
        ) {
            builder.args(["--session", session.id.as_str()]);
            None
        } else {
            let capture = prepare_pi_session_capture(env)?;
            capture.attach_to_command(&mut builder);
            let extension_path = pi_session_capture_extension_path();
            let extension_path = extension_path.to_string_lossy().into_owned();
            builder.args(["-e", extension_path.as_str()]);
            Some(DiscoveryKind::Pi { capture })
        };
        Ok((builder, launch_config, discovery))
    }
}

impl AgentHarness for GeminiHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::Gemini
    }
    fn label(&self) -> &'static str {
        "Gemini"
    }
    fn command(&self) -> &'static str {
        "gemini"
    }
}

impl AgentHarness for OpenCodeHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::OpenCode
    }
    fn label(&self) -> &'static str {
        "OpenCode"
    }
    fn command(&self) -> &'static str {
        "opencode"
    }
}

impl AgentHarness for AmpHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::Amp
    }
    fn label(&self) -> &'static str {
        "Amp"
    }
    fn command(&self) -> &'static str {
        "amp"
    }
}

impl AgentHarness for RovoDevHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::RovoDev
    }
    fn label(&self) -> &'static str {
        "Rovo Dev"
    }
    fn command(&self) -> &'static str {
        "rovo-dev"
    }
}

impl AgentHarness for ForgeHarness {
    fn provider_kind(&self) -> AgentProviderKind {
        AgentProviderKind::Forge
    }
    fn label(&self) -> &'static str {
        "Forge"
    }
    fn command(&self) -> &'static str {
        "forge"
    }
}

pub(crate) fn harness(kind: AgentProviderKind) -> &'static dyn AgentHarness {
    match kind {
        AgentProviderKind::ClaudeCode => &ClaudeCodeHarness,
        AgentProviderKind::CursorAgent => &CursorAgentHarness,
        AgentProviderKind::Codex => &CodexHarness,
        AgentProviderKind::Pi => &PiHarness,
        AgentProviderKind::Gemini => &GeminiHarness,
        AgentProviderKind::OpenCode => &OpenCodeHarness,
        AgentProviderKind::Amp => &AmpHarness,
        AgentProviderKind::RovoDev => &RovoDevHarness,
        AgentProviderKind::Forge => &ForgeHarness,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        harness, terminal_launch_config_for_selected_agent,
        terminal_launch_config_for_selected_agents, AgentProviderKind, TerminalLaunchConfig,
        TerminalLaunchMode, AGENTS, ALL_PROVIDERS,
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

    #[test]
    fn harness_registry_round_trips_every_provider() {
        for &kind in ALL_PROVIDERS {
            assert_eq!(harness(kind).provider_kind(), kind);
        }
    }

    #[test]
    fn agent_def_label_matches_harness_label() {
        for agent in AGENTS {
            let Some(provider) = agent.provider else {
                continue;
            };
            assert_eq!(
                agent.label,
                harness(provider).label(),
                "AGENTS[{}].label drifts from harness label",
                agent.id
            );
        }
    }
}
