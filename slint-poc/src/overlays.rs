#![cfg_attr(not(test), allow(dead_code))]

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayKind {
    Modal,
    Menu,
    Popover,
    Toast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EscapeBehavior {
    Dismiss,
    CloseDropdownThenDismiss,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnterBehavior {
    Submit,
    PlatformSubmit,
    ActivateItem,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayContract {
    pub id: &'static str,
    pub kind: OverlayKind,
    pub gpui_source: &'static str,
    pub gpui_symbol: &'static str,
    pub slint_component: &'static str,
    pub dismisses_on_outside_click: bool,
    pub escape_behavior: EscapeBehavior,
    pub enter_behavior: EnterBehavior,
    pub routes_notifications_through_toast: bool,
}

pub const OVERLAY_CONTRACTS: &[OverlayContract] = &[
    OverlayContract {
        id: "new-task-modal",
        kind: OverlayKind::Modal,
        gpui_source: "desktop/src/new_task_modal.rs",
        gpui_symbol: "new_task_modal_overlay",
        slint_component: "AoNewTaskModal",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::Submit,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "custom-action-modal",
        kind: OverlayKind::Modal,
        gpui_source: "desktop/src/custom_actions_modal.rs",
        gpui_symbol: "custom_action_modal_overlay",
        slint_component: "AoActionModal",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::CloseDropdownThenDismiss,
        enter_behavior: EnterBehavior::PlatformSubmit,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "resource-indicator-popover",
        kind: OverlayKind::Popover,
        gpui_source: "desktop/src/resource_indicator.rs",
        gpui_symbol: "resource_indicator_overlay",
        slint_component: "AoResourcePopover",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::None,
        enter_behavior: EnterBehavior::None,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "titlebar-actions-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/titlebar.rs",
        gpui_symbol: "titlebar_custom_actions_overlay",
        slint_component: "AoMenuPopover",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "titlebar-open-in-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/titlebar.rs",
        gpui_symbol: "titlebar_open_in_overlay",
        slint_component: "AoMenuPopover",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "sidebar-project-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/left_sidebar.rs",
        gpui_symbol: "project_menu_overlay",
        slint_component: "AoMenuPopover",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "sidebar-task-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/left_sidebar.rs",
        gpui_symbol: "sidebar_task_menu_overlay",
        slint_component: "AoMenuPopover",
        dismisses_on_outside_click: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        routes_notifications_through_toast: true,
    },
    OverlayContract {
        id: "toast-layer",
        kind: OverlayKind::Toast,
        gpui_source: "desktop/src/app.rs",
        gpui_symbol: "toast_layer",
        slint_component: "AoToast",
        dismisses_on_outside_click: false,
        escape_behavior: EscapeBehavior::None,
        enter_behavior: EnterBehavior::None,
        routes_notifications_through_toast: true,
    },
];

pub fn overlay_contract(id: &str) -> Option<&'static OverlayContract> {
    OVERLAY_CONTRACTS.iter().find(|contract| contract.id == id)
}

pub fn overlay_review_sources(repo_root: &Path) -> Vec<PathBuf> {
    let mut sources = OVERLAY_CONTRACTS
        .iter()
        .map(|contract| repo_root.join(contract.gpui_source))
        .collect::<Vec<_>>();
    sources.sort();
    sources.dedup();
    sources
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("slint-poc has a workspace parent")
            .to_path_buf()
    }

    #[test]
    fn overlay_contracts_cover_required_gpui_flows() {
        for id in [
            "new-task-modal",
            "custom-action-modal",
            "resource-indicator-popover",
            "titlebar-actions-menu",
            "titlebar-open-in-menu",
            "sidebar-project-menu",
            "sidebar-task-menu",
            "toast-layer",
        ] {
            assert!(overlay_contract(id).is_some(), "missing {id}");
        }
    }

    #[test]
    fn transient_overlays_dismiss_on_outside_click_except_toasts() {
        for contract in OVERLAY_CONTRACTS {
            match contract.kind {
                OverlayKind::Modal | OverlayKind::Menu | OverlayKind::Popover => {
                    assert!(
                        contract.dismisses_on_outside_click,
                        "{} must preserve GPUI outside-click dismissal",
                        contract.id
                    );
                }
                OverlayKind::Toast => {
                    assert!(!contract.dismisses_on_outside_click);
                }
            }
        }
    }

    #[test]
    fn modal_and_menu_keyboard_contracts_match_gpui_baseline() {
        let new_task = overlay_contract("new-task-modal").unwrap();
        assert_eq!(new_task.escape_behavior, EscapeBehavior::Dismiss);
        assert_eq!(new_task.enter_behavior, EnterBehavior::Submit);

        let action = overlay_contract("custom-action-modal").unwrap();
        assert_eq!(
            action.escape_behavior,
            EscapeBehavior::CloseDropdownThenDismiss
        );
        assert_eq!(action.enter_behavior, EnterBehavior::PlatformSubmit);

        for contract in OVERLAY_CONTRACTS
            .iter()
            .filter(|contract| contract.kind == OverlayKind::Menu)
        {
            assert_eq!(contract.escape_behavior, EscapeBehavior::Dismiss);
            assert_eq!(contract.enter_behavior, EnterBehavior::ActivateItem);
        }
    }

    #[test]
    fn gpui_source_symbols_stay_source_backed() {
        let repo_root = repo_root();
        for contract in OVERLAY_CONTRACTS {
            let path = repo_root.join(contract.gpui_source);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
            assert!(
                source.contains(contract.gpui_symbol),
                "{} should contain {}",
                contract.gpui_source,
                contract.gpui_symbol
            );
        }
    }

    #[test]
    fn review_source_inventory_is_deduplicated() {
        let sources = overlay_review_sources(&repo_root());
        assert_eq!(sources.len(), 6);
        assert!(sources
            .iter()
            .any(|path| path.ends_with("desktop/src/app.rs")));
        assert!(sources
            .iter()
            .any(|path| path.ends_with("desktop/src/titlebar.rs")));
    }
}
