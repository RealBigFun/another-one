use daemon_proto::{Control, ErrKind, WorkerReply};
use crate::registry::DaemonRegistry;

pub(crate) fn handle(ctrl: Control, registry: &dyn DaemonRegistry) -> Result<WorkerReply, Control> {
    match ctrl {
        Control::ReadEnabledAgents => Ok(WorkerReply::EnabledAgentsAck {
            view: registry.read_enabled_agents(),
        }),
        Control::ReadAgentSettings => Ok(WorkerReply::AgentSettingsAck {
            view: registry.read_agent_settings(),
        }),
        Control::SetAgentEnabled { agent_id, enabled } => {
            Ok(match registry.set_agent_enabled(&agent_id, enabled) {
                Ok(changed) => WorkerReply::SetAgentEnabledAck { changed },
                Err(message) => WorkerReply::Err {
                    kind: agent_settings_error_kind(&message),
                    message,
                },
            })
        }
        Control::SetDefaultAgent { agent_id } => Ok(match registry.set_default_agent(&agent_id) {
            Ok(changed) => WorkerReply::SetDefaultAgentAck { changed },
            Err(message) => WorkerReply::Err {
                kind: agent_settings_error_kind(&message),
                message,
            },
        }),
        Control::SetAgentLaunchArgs { agent_id, args } => {
            Ok(match registry.set_agent_launch_args(&agent_id, args) {
                Ok(changed) => WorkerReply::SetAgentLaunchArgsAck { changed },
                Err(message) => WorkerReply::Err {
                    kind: agent_settings_error_kind(&message),
                    message,
                },
            })
        }
        other => Err(other),
    }
}

fn agent_settings_error_kind(message: &str) -> ErrKind {
    if message.contains("unknown agent") {
        ErrKind::UnknownId
    } else {
        ErrKind::Internal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_unknown_agent_errors_as_unknown_id() {
        assert!(matches!(
            agent_settings_error_kind("unknown agent codex"),
            ErrKind::UnknownId
        ));
    }

    #[test]
    fn classifies_other_agent_errors_as_internal() {
        assert!(matches!(
            agent_settings_error_kind("settings file unavailable"),
            ErrKind::Internal
        ));
    }
}
