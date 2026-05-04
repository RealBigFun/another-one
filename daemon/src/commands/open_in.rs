use daemon_proto::{Control, ErrKind, WorkerReply};
use crate::registry::DaemonRegistry;

pub(crate) fn handle(ctrl: Control, registry: &dyn DaemonRegistry) -> Result<WorkerReply, Control> {
    match ctrl {
        Control::OpenInState => Ok(match registry.open_in_state() {
            Some(state) => WorkerReply::OpenInStateAck { state },
            None => unsupported("open_in_state"),
        }),
        Control::ReadOpenInSettings => Ok(match registry.read_open_in_settings() {
            Some(view) => WorkerReply::OpenInSettingsAck { view },
            None => unsupported("read_open_in_settings"),
        }),
        Control::SetOpenInAppEnabled { app_id, enabled } => {
            Ok(match registry.set_open_in_app_enabled(&app_id, enabled) {
                Ok(()) => WorkerReply::SetOpenInAppEnabledAck,
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            })
        }
        Control::OpenProjectInApp { project_id, app_id } => {
            Ok(match registry.open_project_in_app(&project_id, &app_id) {
                Ok(()) => WorkerReply::OpenProjectInAppAck,
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            })
        }
        other => Err(other),
    }
}

fn unsupported(verb: &str) -> WorkerReply {
    WorkerReply::Err {
        message: format!("{verb}: registry does not surface Open-In on this host"),
        kind: ErrKind::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_open_in_reply_preserves_wire_error_kind() {
        match unsupported("open_in_state") {
            WorkerReply::Err { kind, message } => {
                assert!(matches!(kind, ErrKind::Unsupported));
                assert!(message.contains("open_in_state"));
            }
            other => panic!("unexpected reply: {other:?}"),
        }
    }
}
