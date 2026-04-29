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
pub enum ToastRoute {
    AppToastLayer,
    EmitsToastRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuGroup {
    None,
    Titlebar,
    Sidebar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValidationAffordance {
    None,
    RequiredTaskName,
    RequiredActionNameAndBody,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceSemantics {
    None,
    AppStatsAndSessionTree,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlayContract {
    pub id: &'static str,
    pub kind: OverlayKind,
    pub gpui_source: &'static str,
    pub gpui_symbol: &'static str,
    pub slint_component: &'static str,
    pub menu_group: MenuGroup,
    pub dismisses_on_outside_click: bool,
    pub stops_inner_click_propagation: bool,
    pub escape_behavior: EscapeBehavior,
    pub enter_behavior: EnterBehavior,
    pub toast_route: ToastRoute,
    pub validation_affordance: ValidationAffordance,
    pub resource_semantics: ResourceSemantics,
    pub has_refresh_action: bool,
}

pub const OVERLAY_CONTRACTS: &[OverlayContract] = &[
    OverlayContract {
        id: "new-task-modal",
        kind: OverlayKind::Modal,
        gpui_source: "desktop/src/new_task_modal.rs",
        gpui_symbol: "new_task_modal_overlay",
        slint_component: "AoNewTaskModal",
        menu_group: MenuGroup::None,
        dismisses_on_outside_click: true,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::Submit,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::RequiredTaskName,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
    },
    OverlayContract {
        id: "custom-action-modal",
        kind: OverlayKind::Modal,
        gpui_source: "desktop/src/custom_actions_modal.rs",
        gpui_symbol: "custom_action_modal_overlay",
        slint_component: "AoActionModal",
        menu_group: MenuGroup::None,
        dismisses_on_outside_click: true,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::CloseDropdownThenDismiss,
        enter_behavior: EnterBehavior::PlatformSubmit,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::RequiredActionNameAndBody,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
    },
    OverlayContract {
        id: "resource-indicator-popover",
        kind: OverlayKind::Popover,
        gpui_source: "desktop/src/resource_indicator.rs",
        gpui_symbol: "resource_indicator_overlay",
        slint_component: "AoResourcePopover",
        menu_group: MenuGroup::None,
        dismisses_on_outside_click: false,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::None,
        enter_behavior: EnterBehavior::None,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::None,
        resource_semantics: ResourceSemantics::AppStatsAndSessionTree,
        has_refresh_action: true,
    },
    OverlayContract {
        id: "titlebar-actions-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/titlebar.rs",
        gpui_symbol: "titlebar_custom_actions_overlay",
        slint_component: "AoMenuPopover",
        menu_group: MenuGroup::Titlebar,
        dismisses_on_outside_click: true,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::None,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
    },
    OverlayContract {
        id: "titlebar-open-in-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/titlebar.rs",
        gpui_symbol: "titlebar_open_in_overlay",
        slint_component: "AoMenuPopover",
        menu_group: MenuGroup::Titlebar,
        dismisses_on_outside_click: true,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::None,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
    },
    OverlayContract {
        id: "sidebar-project-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/left_sidebar.rs",
        gpui_symbol: "project_menu_overlay",
        slint_component: "AoMenuPopover",
        menu_group: MenuGroup::Sidebar,
        dismisses_on_outside_click: true,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::None,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
    },
    OverlayContract {
        id: "sidebar-task-menu",
        kind: OverlayKind::Menu,
        gpui_source: "desktop/src/left_sidebar.rs",
        gpui_symbol: "sidebar_task_menu_overlay",
        slint_component: "AoMenuPopover",
        menu_group: MenuGroup::Sidebar,
        dismisses_on_outside_click: true,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::Dismiss,
        enter_behavior: EnterBehavior::ActivateItem,
        toast_route: ToastRoute::EmitsToastRequest,
        validation_affordance: ValidationAffordance::None,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
    },
    OverlayContract {
        id: "toast-layer",
        kind: OverlayKind::Toast,
        gpui_source: "desktop/src/app.rs",
        gpui_symbol: "toast_layer",
        slint_component: "AoToast",
        menu_group: MenuGroup::None,
        dismisses_on_outside_click: false,
        stops_inner_click_propagation: true,
        escape_behavior: EscapeBehavior::None,
        enter_behavior: EnterBehavior::None,
        toast_route: ToastRoute::AppToastLayer,
        validation_affordance: ValidationAffordance::None,
        resource_semantics: ResourceSemantics::None,
        has_refresh_action: false,
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
    fn only_gpui_scrims_and_menus_dismiss_on_outside_click() {
        for contract in OVERLAY_CONTRACTS {
            match contract.kind {
                OverlayKind::Modal | OverlayKind::Menu => {
                    assert!(
                        contract.dismisses_on_outside_click,
                        "{} must preserve GPUI outside-click dismissal",
                        contract.id
                    );
                }
                OverlayKind::Popover | OverlayKind::Toast => {
                    assert!(!contract.dismisses_on_outside_click);
                }
            }
        }
    }

    #[test]
    fn transient_overlays_stop_inner_clicks_from_reaching_scrims() {
        for contract in OVERLAY_CONTRACTS.iter().filter(|contract| {
            matches!(
                contract.kind,
                OverlayKind::Modal | OverlayKind::Menu | OverlayKind::Popover | OverlayKind::Toast
            )
        }) {
            assert!(
                contract.stops_inner_click_propagation,
                "{} must keep panel/card clicks from dismissing parent overlays",
                contract.id
            );
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
    fn modal_validation_and_resource_semantics_are_explicit() {
        assert_eq!(
            overlay_contract("new-task-modal")
                .unwrap()
                .validation_affordance,
            ValidationAffordance::RequiredTaskName
        );
        assert_eq!(
            overlay_contract("custom-action-modal")
                .unwrap()
                .validation_affordance,
            ValidationAffordance::RequiredActionNameAndBody
        );
        let resources = overlay_contract("resource-indicator-popover").unwrap();
        assert_eq!(
            resources.resource_semantics,
            ResourceSemantics::AppStatsAndSessionTree
        );
        assert!(resources.has_refresh_action);
    }

    #[test]
    fn menu_groups_record_mutual_exclusion_contracts() {
        let titlebar_menus = OVERLAY_CONTRACTS
            .iter()
            .filter(|contract| contract.menu_group == MenuGroup::Titlebar)
            .count();
        let sidebar_menus = OVERLAY_CONTRACTS
            .iter()
            .filter(|contract| contract.menu_group == MenuGroup::Sidebar)
            .count();

        assert_eq!(titlebar_menus, 2);
        assert_eq!(sidebar_menus, 2);
        for contract in OVERLAY_CONTRACTS
            .iter()
            .filter(|contract| contract.kind == OverlayKind::Menu)
        {
            assert_ne!(contract.menu_group, MenuGroup::None);
        }
    }

    #[test]
    fn notifications_have_a_single_toast_routing_contract() {
        let toast_layer = overlay_contract("toast-layer").unwrap();
        assert_eq!(toast_layer.toast_route, ToastRoute::AppToastLayer);

        for contract in OVERLAY_CONTRACTS
            .iter()
            .filter(|contract| contract.kind != OverlayKind::Toast)
        {
            assert_eq!(
                contract.toast_route,
                ToastRoute::EmitsToastRequest,
                "{} should not render user-facing notifications inline",
                contract.id
            );
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
