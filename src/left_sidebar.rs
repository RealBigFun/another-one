//! Left sidebar content: project groups, default branches, and worktrees.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, AnyElement, ClipboardItem, Context, KeyDownEvent,
    MouseButton, MouseDownEvent, PathPromptOptions, SharedString, Window,
};

use crate::app::{
    AnotherOneApp, SectionId, SidebarTaskDeleteConfirmState, SidebarTaskDeleteRequest,
    SidebarTaskMenuState, SidebarTaskRenameState,
};
use crate::project_store::{Branch, Project, TaskKind};
use crate::shortcuts::{shortcut_matches_event, ShortcutAction};
use crate::theme;

const PROJECT_ROW_H: f32 = 34.;
const BRANCH_ROW_H: f32 = 44.;
const LIST_TOP_PAD: f32 = 4.;
const LIST_GAP: f32 = 2.;
const MENU_W: f32 = 316.;
const TASK_MENU_W: f32 = 248.;
const TASK_MENU_H: f32 = 152.;

#[derive(Clone)]
struct SidebarTaskEntry {
    project_id: String,
    project_path: PathBuf,
    task_id: String,
    task_name: String,
    kind: TaskKind,
    branch: Branch,
}

#[derive(Clone)]
struct SidebarGroup {
    root_project: Project,
    child_entries: Vec<SidebarTaskEntry>,
}

struct SidebarTaskMenuRequest {
    project_id: String,
    task_id: String,
    task_name: String,
    branch_name: String,
    kind: TaskKind,
}

struct ProjectRowState {
    github_url: Option<String>,
    active: bool,
    has_children: bool,
    expanded: bool,
}

struct SidebarTaskMenuItemStyle {
    tooltip_label: &'static str,
    text_color: gpui::Hsla,
    hover_bg: gpui::Hsla,
}

impl AnotherOneApp {
    fn sidebar_group_key(project: &Project) -> String {
        project
            .repo_common_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| format!("project:{}", project.id))
    }

    fn sidebar_branch_for_project(
        &self,
        project: &Project,
        prefer_default: bool,
    ) -> Option<Branch> {
        self.project_store
            .primary_branch_for_project(&project.id, prefer_default)
    }

    fn sidebar_branch_named(&self, project: &Project, branch_name: &str) -> Branch {
        self.project_store
            .branch_view(&project.id, branch_name)
            .or_else(|| self.sidebar_branch_for_project(project, false))
            .unwrap_or_else(|| Branch {
                name: branch_name.to_string(),
                lines_added: 0,
                lines_removed: 0,
                ahead_count: 0,
                behind_count: 0,
                last_commit_relative: String::new(),
                is_default: false,
                is_current: false,
            })
    }

    fn sidebar_root_project_for_project(&self, project_id: &str) -> Option<Project> {
        let project = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)?;
        let group_key = Self::sidebar_group_key(project);

        self.project_store
            .projects
            .iter()
            .find(|candidate| {
                Self::sidebar_group_key(candidate) == group_key && candidate.worktree_name.is_none()
            })
            .cloned()
            .or_else(|| Some(project.clone()))
    }

    fn sidebar_task_is_pinned(&self, task_id: &str) -> bool {
        self.project_store.ui.pinned_task_ids.contains(task_id)
    }

    fn sidebar_task_entry_is_pinned(&self, entry: &SidebarTaskEntry) -> bool {
        self.sidebar_task_is_pinned(&entry.task_id)
    }

    fn sidebar_groups(&self) -> Vec<SidebarGroup> {
        let mut order = Vec::new();
        let mut grouped_indices: HashMap<String, Vec<usize>> = HashMap::new();

        for (index, project) in self.project_store.projects.iter().enumerate() {
            let key = Self::sidebar_group_key(project);
            grouped_indices
                .entry(key.clone())
                .and_modify(|indices| indices.push(index))
                .or_insert_with(|| {
                    order.push(key);
                    vec![index]
                });
        }

        let mut groups = Vec::new();
        for key in order {
            let Some(indices) = grouped_indices.get(&key) else {
                continue;
            };

            let root_index = indices
                .iter()
                .copied()
                .find(|index| self.project_store.projects[*index].worktree_name.is_none())
                .unwrap_or(indices[0]);
            let root_project = self.project_store.projects[root_index].clone();

            let mut child_entries = Vec::new();
            if let Some(tasks) = self.project_store.tasks.get(&root_project.id) {
                for task in tasks {
                    let (project_id, project_path, branch) = match task.kind {
                        TaskKind::Direct => (
                            root_project.id.clone(),
                            root_project.path.clone(),
                            self.sidebar_branch_named(&root_project, &task.branch_name),
                        ),
                        TaskKind::Worktree | TaskKind::MultiWorktree => {
                            let wt_project = task.worktree_project_id.as_ref().and_then(|wt_id| {
                                self.project_store.projects.iter().find(|p| p.id == *wt_id)
                            });
                            if let Some(wt) = wt_project {
                                let branch =
                                    self.sidebar_branch_for_project(wt, false).unwrap_or_else(
                                        || self.sidebar_branch_named(wt, &task.branch_name),
                                    );
                                (wt.id.clone(), wt.path.clone(), branch)
                            } else {
                                continue;
                            }
                        }
                    };
                    child_entries.push(SidebarTaskEntry {
                        project_id,
                        project_path,
                        task_id: task.id.clone(),
                        task_name: task.name.clone(),
                        kind: task.kind,
                        branch,
                    });
                }
            }

            child_entries.sort_by_key(|entry| !self.sidebar_task_entry_is_pinned(entry));

            groups.push(SidebarGroup {
                root_project,
                child_entries,
            });
        }

        groups
    }

    fn project_group_member_ids(&self, root_project_id: &str) -> Vec<String> {
        let Some(root_project) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == root_project_id)
        else {
            return Vec::new();
        };
        let group_key = Self::sidebar_group_key(root_project);

        self.project_store
            .projects
            .iter()
            .filter(|project| Self::sidebar_group_key(project) == group_key)
            .map(|project| project.id.clone())
            .collect()
    }

    fn removed_repo_ids_without_remaining_projects(
        projects: &[Project],
        removed_project_ids: &std::collections::HashSet<String>,
    ) -> std::collections::HashSet<String> {
        let removed_repo_ids = projects
            .iter()
            .filter(|project| removed_project_ids.contains(&project.id))
            .map(|project| project.repo_id.clone())
            .collect::<std::collections::HashSet<_>>();

        removed_repo_ids
            .into_iter()
            .filter(|repo_id| {
                !projects.iter().any(|project| {
                    project.repo_id == *repo_id && !removed_project_ids.contains(&project.id)
                })
            })
            .collect()
    }

    fn project_group_remove_confirm(
        &self,
        root_project_id: &str,
    ) -> Option<crate::app::ProjectRemoveConfirmState> {
        let root_project = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == root_project_id)?;
        let project_ids = self.project_group_member_ids(root_project_id);
        let open_task_count = self
            .project_store
            .tasks
            .get(root_project_id)
            .map_or(0, Vec::len);

        Some(crate::app::ProjectRemoveConfirmState {
            project_name: root_project.name.clone(),
            project_ids,
            open_task_count,
        })
    }

    fn fallback_section_after_project_removal(&self) -> Option<(SectionId, PathBuf)> {
        let project = self.project_store.projects.first()?;
        let branch_name = self.project_store.current_branch_name(&project.id)?;
        Some((
            SectionId::new(&project.id, &branch_name),
            project.path.clone(),
        ))
    }

    fn remove_project_group_ids(&mut self, project_ids: &[String], cx: &mut Context<Self>) {
        let project_id_set: std::collections::HashSet<String> =
            project_ids.iter().cloned().collect();
        let removed_repo_ids = Self::removed_repo_ids_without_remaining_projects(
            &self.project_store.projects,
            &project_id_set,
        );

        for project_id in project_ids {
            self.project_store.remove_project(project_id);
        }
        self.changed_files
            .retain(|project_id, _| !project_id_set.contains(project_id));
        self.project_github_links
            .retain(|project_id, _| !project_id_set.contains(project_id));
        self.project_github_link_checked
            .retain(|project_id| !project_id_set.contains(project_id));
        self.expanded_projects
            .retain(|repo_id| !removed_repo_ids.contains(repo_id));
        self.project_store
            .set_expanded_projects(&self.expanded_projects);

        if self
            .project_menu_project
            .as_ref()
            .is_some_and(|project_id| project_id_set.contains(project_id))
        {
            self.project_menu_project = None;
        }
        if self
            .new_task_modal
            .as_ref()
            .is_some_and(|state| project_id_set.contains(&state.project_id))
        {
            self.new_task_modal = None;
        }
        if self
            .discard_confirm
            .as_ref()
            .is_some_and(|(project_id, _)| project_id_set.contains(project_id))
        {
            self.discard_confirm = None;
        }
        if self
            .sidebar_task_rename
            .as_ref()
            .is_some_and(|state| project_id_set.contains(&state.project_id))
        {
            self.sidebar_task_rename = None;
        }
        if self
            .sidebar_task_last_click
            .as_ref()
            .is_some_and(|(project_id, _, _)| project_id_set.contains(project_id))
        {
            self.sidebar_task_last_click = None;
        }
        if self
            .sidebar_task_menu
            .as_ref()
            .is_some_and(|menu| project_id_set.contains(&menu.project_id))
        {
            self.sidebar_task_menu = None;
        }
        if self
            .sidebar_task_delete_confirm
            .as_ref()
            .is_some_and(|confirm| project_id_set.contains(&confirm.project_id))
        {
            self.sidebar_task_delete_confirm = None;
        }
        self.project_github_link_requests
            .retain(|project_id| !project_id_set.contains(project_id));
        self.project_remove_confirm = None;
        let fallback_section = self.fallback_section_after_project_removal();
        self.project_store.save();

        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.remove_project_sections(&project_id_set, cx);
            if workspace.active_section.is_none() && workspace.active_project_page.is_none() {
                if let Some((section_id, cwd)) = fallback_section.clone() {
                    workspace.activate_section(section_id, Some(cwd), None, cx);
                }
            }
        });

        cx.notify();
    }

    pub(crate) fn request_remove_project_group(
        &mut self,
        root_project_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(confirm) = self.project_group_remove_confirm(root_project_id) else {
            return;
        };

        self.project_menu_project = None;
        self.sidebar_task_menu = None;

        if confirm.open_task_count == 0 {
            self.remove_project_group_ids(&confirm.project_ids, cx);
            self.show_success_toast(
                format!("Removed {} from the sidebar.", confirm.project_name),
                cx,
            );
            return;
        }

        self.project_remove_confirm = Some(confirm);
        cx.notify();
    }

    pub(crate) fn confirm_remove_project_group(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.project_remove_confirm.clone() else {
            return;
        };

        self.remove_project_group_ids(&confirm.project_ids, cx);
        self.show_success_toast(
            format!(
                "Removed {} and its open tasks from the sidebar.",
                confirm.project_name
            ),
            cx,
        );
    }

    fn restore_view_after_task_removal(
        &mut self,
        preferred_project_id: &str,
        cx: &mut Context<Self>,
    ) {
        let preferred_project_exists = self
            .project_store
            .projects
            .iter()
            .any(|project| project.id == preferred_project_id);
        let fallback = self.fallback_section_after_project_removal();
        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.restore_view(preferred_project_id, preferred_project_exists, fallback, cx);
        });
    }

    fn set_sidebar_task_pinned(&mut self, task_id: &str, pinned: bool) -> bool {
        let changed = self.project_store.set_task_pinned(task_id, pinned);
        if changed {
            self.project_store.save();
        }
        changed
    }

    fn open_sidebar_task_menu(
        &mut self,
        request: SidebarTaskMenuRequest,
        ev: &MouseDownEvent,
        cx: &mut Context<Self>,
    ) {
        let SidebarTaskMenuRequest {
            project_id,
            task_id,
            task_name,
            branch_name,
            kind,
        } = request;
        let root_project_id = self
            .sidebar_root_project_for_project(&project_id)
            .map(|project| project.id)
            .unwrap_or_else(|| project_id.clone());
        let is_worktree = kind == TaskKind::Worktree || kind == TaskKind::MultiWorktree;

        self.commit_sidebar_task_rename(cx);
        self.project_menu_project = None;
        self.sidebar_task_last_click = None;
        self.sidebar_task_menu = Some(SidebarTaskMenuState {
            project_id,
            root_project_id,
            row_id: task_id.clone(),
            task_id: Some(task_id),
            task_name,
            branch_name,
            is_worktree,
            anchor_x: f32::from(ev.position.x),
            anchor_y: f32::from(ev.position.y),
        });
        cx.stop_propagation();
        cx.notify();
    }

    fn build_sidebar_task_delete_confirm(
        &self,
        project_id: &str,
        task_id: Option<&str>,
        task_name: &str,
        branch_name: &str,
        is_worktree: bool,
    ) -> Option<SidebarTaskDeleteConfirmState> {
        let project = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)?
            .clone();
        let root_project = self.sidebar_root_project_for_project(project_id)?;

        Some(SidebarTaskDeleteConfirmState {
            project_id: project_id.to_string(),
            root_project_id: root_project.id,
            task_id: task_id.map(str::to_string),
            task_name: task_name.to_string(),
            branch_name: branch_name.to_string(),
            project_path: project.path,
            repo_path: root_project.path,
            is_worktree,
        })
    }

    fn delete_direct_sidebar_task(
        &mut self,
        project_id: &str,
        task_id: &str,
        task_name: &str,
        preferred_project_id: &str,
        cx: &mut Context<Self>,
    ) {
        if self
            .project_store
            .remove_task(project_id, task_id)
            .is_none()
        {
            self.show_error_toast("Could not find the selected task.", cx);
            return;
        }

        self.sidebar_task_menu = None;
        self.sidebar_task_last_click = None;
        if self
            .sidebar_task_rename
            .as_ref()
            .is_some_and(|rename| rename.project_id == project_id && rename.row_id == task_id)
        {
            self.sidebar_task_rename = None;
        }
        self.workspace_pane.update(cx, |workspace, cx| {
            workspace.remove_task_sections(task_id, cx);
        });
        self.restore_view_after_task_removal(preferred_project_id, cx);
        self.project_store.save();
        self.show_success_toast(format!("Deleted task {}.", task_name), cx);
        cx.notify();
    }

    pub(crate) fn request_sidebar_task_delete(
        &mut self,
        request: SidebarTaskDeleteRequest,
        cx: &mut Context<Self>,
    ) {
        let SidebarTaskDeleteRequest {
            project_id,
            task_id,
            task_name,
            branch_name,
            is_worktree,
            preferred_project_id,
        } = request;
        self.sidebar_task_menu = None;
        self.sidebar_task_last_click = None;

        if is_worktree {
            let Some(confirm) = self.build_sidebar_task_delete_confirm(
                &project_id,
                Some(&task_id),
                &task_name,
                &branch_name,
                is_worktree,
            ) else {
                self.show_error_toast("Could not find the selected worktree.", cx);
                return;
            };
            self.sidebar_task_delete_confirm = Some(confirm);
            cx.notify();
            return;
        }

        self.delete_direct_sidebar_task(
            &project_id,
            &task_id,
            &task_name,
            &preferred_project_id,
            cx,
        );
    }

    pub(crate) fn confirm_sidebar_task_delete(&mut self, cx: &mut Context<Self>) {
        let Some(confirm) = self.sidebar_task_delete_confirm.clone() else {
            return;
        };

        if !confirm.is_worktree {
            self.sidebar_task_delete_confirm = None;
            let Some(task_id) = confirm.task_id.as_deref() else {
                self.show_error_toast("Could not find the selected task.", cx);
                return;
            };
            self.delete_direct_sidebar_task(
                &confirm.root_project_id,
                task_id,
                &confirm.task_name,
                &confirm.root_project_id,
                cx,
            );
            return;
        }

        let was_active_project = self
            .workspace_pane
            .read(cx)
            .active_section
            .as_ref()
            .is_some_and(|section| section.project_id == confirm.project_id);

        match crate::project_store::remove_task_worktree(&confirm.repo_path, &confirm.project_path)
        {
            Ok(()) => {
                if let Err(error) = crate::project_store::delete_local_branch(
                    &confirm.repo_path,
                    &confirm.branch_name,
                ) {
                    self.show_warning_toast(error, cx);
                }

                if let Some(task_id) = confirm.task_id.as_deref() {
                    self.project_store
                        .remove_task(&confirm.root_project_id, task_id);
                    self.workspace_pane.update(cx, |workspace, cx| {
                        workspace.remove_task_sections(task_id, cx);
                    });
                }

                self.sidebar_task_delete_confirm = None;
                self.remove_project_group_ids(std::slice::from_ref(&confirm.project_id), cx);
                if was_active_project
                    && self
                        .project_store
                        .projects
                        .iter()
                        .any(|project| project.id == confirm.root_project_id)
                {
                    let root_project_id = confirm.root_project_id.clone();
                    self.workspace_pane.update(cx, |workspace, cx| {
                        workspace.activate_project_page(root_project_id, cx);
                    });
                }
                let worktree_display_name = confirm
                    .project_path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| confirm.task_name.clone());
                self.show_success_toast(format!("Deleted worktree {}.", worktree_display_name), cx);
                cx.notify();
            }
            Err(error) => {
                self.show_error_toast(error, cx);
            }
        }
    }

    pub(crate) fn handle_global_key_down(
        &mut self,
        ev: &KeyDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.add_agent_modal.is_some() {
            self.handle_add_agent_modal_key_down(ev, cx);
            return;
        }

        if self.new_task_modal.is_some() {
            self.handle_new_task_modal_key_down(ev, cx);
            return;
        }

        if self.settings_open {
            self.handle_settings_key_down(ev, cx);
            cx.stop_propagation();
            return;
        }

        if self.project_page_open_in_menu_project_id.is_some()
            && ev.keystroke.key.as_str() == "escape"
        {
            self.project_page_open_in_menu_project_id = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::CycleProjects),
            ev,
        ) {
            if self.navigate_project_shortcut(cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::NewTabInCurrentTask),
            ev,
        ) {
            if self.open_new_tab_shortcut(cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::NewTask),
            ev,
        ) {
            if self.open_new_task_shortcut(cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::CloseCurrentTab),
            ev,
        ) {
            if self.close_active_tab_shortcut(cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::NextTab),
            ev,
        ) {
            if self.navigate_tab_shortcut(crate::app::NavigationDirection::Next, cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::PreviousTab),
            ev,
        ) {
            if self.navigate_tab_shortcut(crate::app::NavigationDirection::Previous, cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::NextTask),
            ev,
        ) {
            if self.navigate_task_shortcut(crate::app::NavigationDirection::Next, cx) {
                cx.stop_propagation();
            }
            return;
        }

        if shortcut_matches_event(
            self.project_store
                .ui
                .shortcuts
                .binding_for(ShortcutAction::PreviousTask),
            ev,
        ) {
            if self.navigate_task_shortcut(crate::app::NavigationDirection::Previous, cx) {
                cx.stop_propagation();
            }
            return;
        }

        if self.sidebar_task_menu.is_some() && ev.keystroke.key.as_str() == "escape" {
            self.sidebar_task_menu = None;
            cx.stop_propagation();
            cx.notify();
            return;
        }

        if self.handle_sidebar_task_rename_key_down(ev, cx) {
            return;
        }

        let _ = self.handle_terminal_key_down(ev, cx);
    }

    fn handle_terminal_key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let modifiers = ev.keystroke.modifiers;
        let is_clipboard_combo = is_terminal_clipboard_shortcut(modifiers);

        if is_clipboard_combo && ev.keystroke.key.as_str() == "c" {
            if let Some(text) = self.selected_terminal_text(cx) {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                cx.stop_propagation();
                return true;
            }
            return false;
        }

        if is_clipboard_combo && ev.keystroke.key.as_str() == "v" {
            if self.handle_clipboard_paste(cx) {
                return true;
            }
            return false;
        }

        #[cfg(target_os = "macos")]
        if let Some(bytes) = macos_terminal_command_bytes(ev) {
            if self.write_active_terminal_input(cx, &bytes) {
                cx.stop_propagation();
                return true;
            }
            return false;
        }

        #[cfg(target_os = "macos")]
        if modifiers.platform {
            return false;
        }

        let Some(bytes) = terminal_key_bytes(ev) else {
            return false;
        };

        if self.write_active_terminal_input(cx, &bytes) {
            cx.stop_propagation();
            return true;
        }

        false
    }

    fn handle_sidebar_task_rename_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.sidebar_task_rename.is_none() {
            return false;
        }

        cx.stop_propagation();

        match ev.keystroke.key.as_str() {
            "escape" => {
                self.cancel_sidebar_task_rename(cx);
                return true;
            }
            "enter" => {
                self.commit_sidebar_task_rename(cx);
                return true;
            }
            _ => {}
        }

        let Some(state) = self.sidebar_task_rename.as_mut() else {
            return true;
        };

        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "backspace" => {
                if modifiers.platform {
                    delete_sidebar_task_name_to_start(state);
                } else if modifiers.alt {
                    delete_sidebar_task_name_word_backward(state);
                } else {
                    delete_sidebar_task_name_backward(state);
                }
                cx.notify();
                return true;
            }
            "delete" => {
                delete_sidebar_task_name_forward(state);
                cx.notify();
                return true;
            }
            "left" => {
                move_sidebar_task_name_cursor(state, SidebarCursorDirection::Left, modifiers.shift);
                cx.notify();
                return true;
            }
            "right" => {
                move_sidebar_task_name_cursor(
                    state,
                    SidebarCursorDirection::Right,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "home" => {
                move_sidebar_task_name_cursor_to_edge(state, false, modifiers.shift);
                cx.notify();
                return true;
            }
            "end" => {
                move_sidebar_task_name_cursor_to_edge(state, true, modifiers.shift);
                cx.notify();
                return true;
            }
            "up" | "down" | "tab" => return true,
            _ => {}
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "a" {
            state.task_name_cursor = state.task_name.len();
            state.task_name_selection_anchor = Some(0);
            cx.notify();
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "c" {
            if let Some(range) = selected_sidebar_task_name_range(state) {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    state.task_name[range].to_string(),
                ));
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "x" {
            if let Some(range) = selected_sidebar_task_name_range(state) {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    state.task_name[range.clone()].to_string(),
                ));
                replace_sidebar_task_name_range(state, range, "");
                cx.notify();
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "v" {
            if let Some(text) = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .map(sanitize_sidebar_task_name_input)
            {
                insert_sidebar_task_name_text(state, &text);
                cx.notify();
            }
            return true;
        }

        if modifiers.control || modifiers.platform || modifiers.function {
            return false;
        }

        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
            insert_sidebar_task_name_text(state, key_char);
            cx.notify();
            return true;
        }

        false
    }

    fn begin_sidebar_task_rename(
        &mut self,
        project_id: &str,
        row_id: &str,
        task_name: &str,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_task_last_click = None;
        self.sidebar_task_rename = Some(SidebarTaskRenameState {
            project_id: project_id.to_string(),
            row_id: row_id.to_string(),
            original_name: task_name.to_string(),
            task_name: task_name.to_string(),
            task_name_cursor: task_name.len(),
            task_name_selection_anchor: Some(0),
        });
        cx.notify();
    }

    fn commit_sidebar_task_rename(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(rename) = self.sidebar_task_rename.take() else {
            return false;
        };
        self.sidebar_task_last_click = None;

        let next_name = sanitize_sidebar_task_name_input(rename.task_name)
            .trim()
            .to_string();
        let final_name = if next_name.is_empty() {
            rename.original_name.clone()
        } else {
            next_name
        };

        if final_name != rename.original_name {
            if let Some(task) = self.project_store.find_task_mut(&rename.row_id) {
                task.name = final_name;
                self.project_store.save();
            }
        }

        cx.notify();
        true
    }

    fn cancel_sidebar_task_rename(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_task_rename.take().is_some() {
            self.sidebar_task_last_click = None;
            cx.notify();
        }
    }

    fn sidebar_task_is_being_renamed(&self, project_id: &str, row_id: &str) -> bool {
        self.sidebar_task_rename
            .as_ref()
            .is_some_and(|state| state.project_id == project_id && state.row_id == row_id)
    }

    fn project_row(
        project: &Project,
        state: ProjectRowState,
        _window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let color = theme::project_color(&project.id);
        let first_char: String = project
            .name
            .chars()
            .next()
            .unwrap_or('?')
            .to_uppercase()
            .collect();
        let name: SharedString = project.name.clone().into();
        let pid = project.id.clone();
        let expand_id = project.repo_id.clone();

        let text_col = hsla(0., 0., 0.90, 1.);
        let hover_bg = gpui::white().opacity(0.06);
        let active_bg = gpui::white().opacity(0.03);
        let active_border = gpui::white().opacity(0.18);
        let chevron_col = hsla(0., 0., 0.55, 1.);
        let pid_row = pid.clone();
        let pid_toggle = expand_id;
        let pid_menu = pid.clone();
        let pid_plus = pid.clone();
        let github_url_for_icon = state.github_url.clone();
        let github_url_for_click = state.github_url.clone();
        let row_group = SharedString::from(format!("project-row-{}", pid));

        let row = div()
            .id(SharedString::from(format!("project-{}", &pid)))
            .group(row_group.clone())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(5.))
            .rounded_md()
            .border_1()
            .border_color(if state.active {
                active_border
            } else {
                gpui::transparent_black()
            })
            .when(state.active, |d| d.bg(active_bg))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .tooltip(move |_window, cx| {
                Self::action_tooltip_view("Open this project's overview page", cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    this.commit_sidebar_task_rename(cx);
                    this.sidebar_task_menu = None;
                    this.project_menu_project = None;
                    this.workspace_pane.update(cx, |workspace, cx| {
                        workspace.activate_project_page(pid_row.clone(), cx);
                    });
                }),
            );

        row.child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .w(px(24.))
                .h(px(24.))
                .rounded(px(5.))
                .bg(rgb(color))
                .text_color(gpui::white())
                .text_size(rems(12. / 16.))
                .font_weight(gpui::FontWeight::BOLD)
                .child(first_char),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .flex_1()
                .min_w(px(0.))
                .overflow_hidden()
                .child(
                    div()
                        .flex_shrink()
                        .min_w(px(0.))
                        .text_sm()
                        .text_color(text_col)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .truncate()
                        .child(name),
                )
                .when(state.has_children, |row| {
                    row.child(
                        div()
                            .id(SharedString::from(format!("project-chevron-{}", &pid)))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(20.))
                            .h(px(20.))
                            .rounded_sm()
                            .cursor_pointer()
                            .hover(move |s| s.bg(gpui::white().opacity(0.08)))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Show or hide tasks and worktrees for this project",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    this.commit_sidebar_task_rename(cx);
                                    this.sidebar_task_menu = None;
                                    this.project_menu_project = None;
                                    this.toggle_project_expansion(&pid_toggle, window, cx);
                                }),
                            )
                            .child(
                                svg()
                                    .path(if state.expanded {
                                        "assets/icons/icons__chevron-down.svg"
                                    } else {
                                        "assets/icons/icons__chevron-right.svg"
                                    })
                                    .size(px(12.))
                                    .text_color(chevron_col),
                            ),
                    )
                }),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(2.))
                .flex_none()
                .child(
                    div()
                        .id(SharedString::from(format!("project-dots-{}", &pid)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(24.))
                        .h(px(24.))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(move |s| s.bg(gpui::white().opacity(0.08)))
                        .tooltip(move |_window, cx| {
                            Self::action_tooltip_view("Open project menu", cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                cx.stop_propagation();
                                this.commit_sidebar_task_rename(cx);
                                this.sidebar_task_menu = None;
                                this.project_menu_project = if this.project_menu_project.as_deref()
                                    == Some(pid_menu.as_str())
                                {
                                    None
                                } else {
                                    Some(pid_menu.clone())
                                };
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__ellipsis.svg")
                                .size(px(14.))
                                .text_color(chevron_col),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("project-github-{}", &pid)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(24.))
                        .h(px(24.))
                        .rounded_md()
                        .cursor_pointer()
                        .when(github_url_for_icon.is_none(), |button| button.invisible())
                        .when(github_url_for_icon.is_some(), |d| {
                            d.hover(move |s| s.bg(gpui::white().opacity(0.08)))
                                .tooltip(move |_window, cx| {
                                    Self::action_tooltip_view("Open this project's GitHub link", cx)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.sidebar_task_menu = None;
                                        if let Some(github_url) = github_url_for_click.clone() {
                                            if let Err(err) = open_external_url(&github_url) {
                                                this.show_error_toast(err, cx);
                                            }
                                        }
                                    }),
                                )
                        })
                        .child(
                            svg()
                                .path("assets/icons/icons__github.svg")
                                .size(px(14.))
                                .text_color(chevron_col),
                        ),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("project-plus-{}", &pid)))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(24.))
                        .h(px(24.))
                        .rounded_md()
                        .cursor_pointer()
                        .hover(move |s| s.bg(gpui::white().opacity(0.08)))
                        .tooltip(move |_window, cx| {
                            Self::action_tooltip_view("Add a task or worktree to this project", cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                this.focus_handle.focus(window);
                                this.commit_sidebar_task_rename(cx);
                                this.sidebar_task_menu = None;
                                this.open_new_task_modal(&pid_plus, cx);
                                cx.notify();
                            }),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__plus.svg")
                                .size(px(14.))
                                .text_color(chevron_col),
                        ),
                ),
        )
    }

    fn branch_row(
        &self,
        entry: &SidebarTaskEntry,
        is_active: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let text_col = hsla(0., 0., 0.80, 1.);
        let muted_col = hsla(0., 0., 0.50, 1.);
        let green = hsla(138. / 360., 0.50, 0.74, 1.);
        let red = hsla(352. / 360., 0.52, 0.76, 1.);
        let pull_request_open = rgb(0x059669);
        let pull_request_closed = rgb(0x71717a);
        let pull_request_merged = rgb(0x7c3aed);
        let hover_bg = gpui::white().opacity(0.05);
        let active_bg = gpui::white().opacity(0.03);
        let active_border = gpui::white().opacity(0.18);
        let edit_border = hsla(220. / 360., 0.55, 0.60, 1.);
        let edit_bg = gpui::black().opacity(0.14);
        let delete_hover_bg = hsla(0., 0.40, 0.34, 0.24);
        let delete_icon_col = hsla(0., 0.72, 0.72, 1.);

        let project_id = entry.project_id.clone();
        let project_path = entry.project_path.clone();
        let task_id = entry.task_id.clone();
        let branch_name = entry.branch.name.clone();
        let task_name = entry.task_name.clone();
        let is_worktree = entry.kind == TaskKind::Worktree || entry.kind == TaskKind::MultiWorktree;
        let row_id = task_id.clone();
        let pull_request = self
            .project_pull_request(&project_id, &branch_name)
            .cloned();
        let pull_request_color =
            pull_request
                .as_ref()
                .map(|pull_request| match pull_request.state {
                    crate::git_actions::PullRequestState::Open => pull_request_open,
                    crate::git_actions::PullRequestState::Closed => pull_request_closed,
                    crate::git_actions::PullRequestState::Merged => pull_request_merged,
                });
        let pull_request_url = pull_request
            .as_ref()
            .map(|pull_request| pull_request.url.clone());
        let root_project_id = self
            .sidebar_root_project_for_project(&project_id)
            .map(|project| project.id)
            .unwrap_or_else(|| project_id.clone());
        let meta_indent = 20.;
        let meta = [
            (entry.task_name != entry.branch.name).then(|| entry.branch.name.clone()),
            (!entry.branch.last_commit_relative.is_empty())
                .then(|| entry.branch.last_commit_relative.clone()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" • ");
        let meta: SharedString = meta.into();
        let rename_state = self
            .sidebar_task_rename
            .as_ref()
            .filter(|state| state.project_id == project_id && state.row_id == row_id);
        let is_editing = rename_state.is_some();
        let is_pinned = self.sidebar_task_entry_is_pinned(entry);
        let menu_open = self
            .sidebar_task_menu
            .as_ref()
            .is_some_and(|menu| menu.project_id == project_id && menu.row_id == row_id);
        let keep_delete_visible = !is_editing && menu_open;
        let row_tooltip =
            "Open this task in the terminal. Double-click to rename it or right-click for more actions.";
        let task_label: AnyElement = if let Some(rename) = rename_state {
            div()
                .id(SharedString::from(format!(
                    "task-rename-{}-{}",
                    project_id, rename.row_id
                )))
                .flex()
                .items_center()
                .flex_1()
                .min_w(px(0.))
                .h(px(28.))
                .px(px(8.))
                .rounded_sm()
                .border_1()
                .border_color(edit_border)
                .bg(edit_bg)
                .cursor_text()
                .tooltip(move |_window, cx| {
                    Self::action_tooltip_view(
                        "Rename this task. Press Enter to save or Escape to cancel.",
                        cx,
                    )
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_this, _ev: &MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                    }),
                )
                .child(Self::render_sidebar_task_name_content(
                    rename.task_name.clone().into(),
                    rename.task_name_cursor,
                    selected_sidebar_task_name_range(rename),
                ))
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .min_w(px(0.))
                .flex_1()
                .child(
                    div()
                        .text_sm()
                        .text_color(text_col)
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .truncate()
                        .child(SharedString::from(task_name.clone())),
                )
                .when(is_worktree, |row| {
                    row.child(
                        svg()
                            .flex_shrink_0()
                            .path("assets/icons/icons__git-split.svg")
                            .size(px(11.))
                            .text_color(muted_col),
                    )
                })
                .when(is_pinned, |row| {
                    row.child(
                        svg()
                            .flex_shrink_0()
                            .path("assets/icons/icons__pin-off.svg")
                            .size(px(11.))
                            .text_color(muted_col),
                    )
                })
                .into_any_element()
        };

        let row_group = SharedString::from(format!("task-row-{project_id}-{row_id}"));
        let mut right_controls = div().flex().flex_row().items_center().gap(px(6.));
        let has_diff = entry.branch.lines_added > 0 || entry.branch.lines_removed > 0;

        let delete_project_id = project_id.clone();
        let delete_task_id = task_id.clone();
        let delete_task_name = task_name.clone();
        let delete_branch_name = branch_name.clone();
        let delete_preferred_project_id = root_project_id.clone();
        let delete_tooltip = if is_worktree {
            "Delete this worktree task"
        } else {
            "Delete this task"
        };

        right_controls = right_controls.child(
            div()
                .id(SharedString::from(format!("task-delete-{}", row_id)))
                .flex()
                .items_center()
                .justify_center()
                .w(px(22.))
                .h(px(22.))
                .rounded_sm()
                .when(keep_delete_visible, |button| button.visible())
                .when(!keep_delete_visible, |button| {
                    button.invisible().when(!is_editing, |button| {
                        button.group_hover(row_group.clone(), |button| button.visible())
                    })
                })
                .when(!is_editing, |button| {
                    button
                        .cursor_pointer()
                        .hover(move |style| style.bg(delete_hover_bg))
                        .tooltip(move |_window, cx| Self::action_tooltip_view(delete_tooltip, cx))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                cx.stop_propagation();
                                this.request_sidebar_task_delete(
                                    SidebarTaskDeleteRequest {
                                        project_id: delete_project_id.clone(),
                                        task_id: delete_task_id.clone(),
                                        task_name: delete_task_name.clone(),
                                        branch_name: delete_branch_name.clone(),
                                        is_worktree,
                                        preferred_project_id: delete_preferred_project_id.clone(),
                                    },
                                    cx,
                                );
                            }),
                        )
                })
                .child(
                    svg()
                        .path("assets/icons/icons__trash.svg")
                        .size(px(13.))
                        .text_color(delete_icon_col),
                ),
        );

        let top_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(6.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .flex_1()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .child(if let Some(color) = pull_request_color {
                        let pull_request_url = pull_request_url.clone();
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .w(px(14.))
                            .h(px(14.))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    this.sidebar_task_menu = None;
                                    if let Some(pull_request_url) = pull_request_url.clone() {
                                        if let Err(err) = open_external_url(&pull_request_url) {
                                            this.show_error_toast(err, cx);
                                        }
                                    }
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__pull-request.svg")
                                    .size(px(13.))
                                    .text_color(color),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .flex_shrink_0()
                            .w(px(14.))
                            .h(px(14.))
                            .into_any_element()
                    })
                    .child(task_label)
                    .when(is_worktree && entry.branch.is_default, |row| {
                        row.child(
                            div()
                                .flex_shrink_0()
                                .text_color(muted_col)
                                .text_size(rems(12. / 16.))
                                .child("★"),
                        )
                    }),
            )
            .child(right_controls);

        let left_click_project_id = project_id.clone();
        let left_click_row_id = row_id.clone();
        let left_click_task_id = task_id.clone();
        let left_click_branch_name = branch_name.clone();
        let left_click_task_name = task_name.clone();
        let right_click_project_id = project_id.clone();
        let right_click_task_id = task_id.clone();
        let right_click_task_name = task_name.clone();
        let right_click_branch_name = branch_name.clone();
        let right_click_kind = entry.kind;

        let mut container = div()
            .id(SharedString::from(format!(
                "task-{}-{}",
                entry.project_id, entry.task_id
            )))
            .group(row_group.clone())
            .flex()
            .flex_col()
            .pl(px(18.))
            .pr(px(10.))
            .py(px(4.))
            .mx(px(2.))
            .rounded_md()
            .border_1()
            .border_color(if is_active {
                active_border
            } else {
                gpui::transparent_black()
            })
            .when(is_editing, |d| d.cursor_text())
            .when(!is_editing, |d| d.cursor_pointer())
            .when(is_active, |d| d.bg(active_bg))
            .hover(move |s| if is_editing { s } else { s.bg(hover_bg) })
            .tooltip(move |_window, cx| Self::action_tooltip_view(row_tooltip, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                    let now = std::time::Instant::now();
                    let repeated_click = this.sidebar_task_last_click.as_ref().is_some_and(
                        |(last_project_id, last_row_id, last_click_at)| {
                            last_project_id == &left_click_project_id
                                && last_row_id == &left_click_row_id
                                && now.duration_since(*last_click_at)
                                    <= crate::app::SIDEBAR_TASK_DOUBLE_CLICK_THRESHOLD
                        },
                    );
                    this.sidebar_task_last_click = Some((
                        left_click_project_id.clone(),
                        left_click_row_id.clone(),
                        now,
                    ));
                    this.sidebar_task_menu = None;
                    this.focus_handle.focus(window);

                    if repeated_click {
                        cx.stop_propagation();
                        this.begin_sidebar_task_rename(
                            &left_click_project_id,
                            &left_click_row_id,
                            &left_click_task_name,
                            cx,
                        );
                        return;
                    }

                    if this
                        .sidebar_task_is_being_renamed(&left_click_project_id, &left_click_row_id)
                    {
                        cx.stop_propagation();
                        return;
                    }

                    this.commit_sidebar_task_rename(cx);
                    let sid = SectionId::for_task(
                        &left_click_project_id,
                        &left_click_branch_name,
                        &left_click_task_id,
                    );
                    let sid_for_state = sid.clone();
                    let project_path = project_path.clone();
                    this.workspace_pane.update(cx, |workspace, cx| {
                        workspace.activate_section(
                            sid_for_state,
                            Some(project_path.clone()),
                            None,
                            cx,
                        );
                    });
                    this.prefetch_section_pull_request_and_checks(&sid, &project_path);
                    this.mark_git_refresh_stale();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _window, cx| {
                    this.open_sidebar_task_menu(
                        SidebarTaskMenuRequest {
                            project_id: right_click_project_id.clone(),
                            task_id: right_click_task_id.clone(),
                            task_name: right_click_task_name.clone(),
                            branch_name: right_click_branch_name.clone(),
                            kind: right_click_kind,
                        },
                        ev,
                        cx,
                    );
                }),
            )
            .child(top_row);

        if !meta.is_empty() || has_diff {
            let added_text: SharedString = format!("+{}", entry.branch.lines_added).into();
            let removed_text: SharedString = format!("-{}", entry.branch.lines_removed).into();
            container = container.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .pl(px(meta_indent))
                    .text_xs()
                    .text_color(muted_col)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .min_w(px(0.))
                            .flex_1()
                            .when(!meta.is_empty(), |row| {
                                row.child(div().min_w(px(0.)).truncate().child(meta.clone()))
                            })
                            .when(!meta.is_empty() && has_diff, |row| row.child("•"))
                            .when(has_diff, |row| {
                                row.child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(4.))
                                        .text_xs()
                                        .child(div().text_color(green).child(added_text))
                                        .child(div().text_color(red).child(removed_text)),
                                )
                            }),
                    ),
            );
        }

        container
    }

    fn project_menu_panel(&self, _target_id: &str, _cx: &mut Context<Self>) -> impl IntoElement {
        let bg = rgb(0x2b2d31);
        let border = gpui::black().opacity(0.35);
        let title_col = hsla(0., 0., 0.92, 1.);
        let body_col = hsla(0., 0., 0.78, 1.);
        let muted_col = hsla(0., 0., 0.58, 1.);
        let hover_bg = gpui::white().opacity(0.06);

        div()
            .w(px(MENU_W))
            .rounded_md()
            .bg(bg)
            .border_1()
            .border_color(border)
            .shadow_md()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(14.))
                    .pt(px(10.))
                    .pb(px(6.))
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(muted_col)
                    .child("Sort tasks by"),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(38.))
                    .px(px(14.))
                    .hover(move |s| s.bg(hover_bg))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(16.)).text_color(title_col).child("◉"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(title_col)
                                    .child("Recent activity"),
                            ),
                    )
                    .child(div()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(38.))
                    .px(px(14.))
                    .hover(move |s| s.bg(hover_bg))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(16.)).text_color(muted_col).child("○"))
                            .child(div().text_sm().text_color(body_col).child("Most activity")),
                    )
                    .child(div()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(38.))
                    .px(px(14.))
                    .hover(move |s| s.bg(hover_bg))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().w(px(16.)).text_color(muted_col).child("○"))
                            .child(div().text_sm().text_color(body_col).child("Manual")),
                    )
                    .child(div()),
            )
    }

    fn project_menu_top(&self, target_id: &str) -> f32 {
        let mut top = 36. + LIST_TOP_PAD;
        for group in self.sidebar_groups() {
            if group.root_project.id == target_id {
                return top;
            }

            top += PROJECT_ROW_H + LIST_GAP;
            if self.expanded_projects.contains(&group.root_project.repo_id) {
                top += (BRANCH_ROW_H + LIST_GAP) * group.child_entries.len() as f32;
            }
        }
        top
    }

    pub(crate) fn project_menu_overlay(&self, sw: f32, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.sidebar_is_open() || self.project_menu_project.is_none() {
            return div().id("project-menu-popover");
        }

        let target_id = self.project_menu_project.as_deref().unwrap();
        let menu_top = self.project_menu_top(target_id);

        div()
            .id("project-menu-popover")
            .absolute()
            .left(px(sw - 4.))
            .top(px(menu_top))
            .on_mouse_down_out(cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.project_menu_project = None;
                cx.notify();
            }))
            .child(self.project_menu_panel(target_id, cx))
    }

    fn sidebar_task_menu_item(
        button_id: SharedString,
        icon_path: &'static str,
        label: SharedString,
        style: SidebarTaskMenuItemStyle,
        on_click: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(button_id)
            .flex()
            .items_center()
            .gap(px(10.))
            .h(px(38.))
            .px(px(14.))
            .cursor_pointer()
            .hover(move |hover| hover.bg(style.hover_bg))
            .tooltip(move |_window, cx| Self::action_tooltip_view(style.tooltip_label, cx))
            .on_mouse_down(MouseButton::Left, cx.listener(on_click))
            .child(
                svg()
                    .path(icon_path)
                    .size(px(15.))
                    .text_color(style.text_color),
            )
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(style.text_color)
                    .child(label),
            )
    }

    fn sidebar_task_menu_panel(
        &self,
        menu: &SidebarTaskMenuState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let bg = rgb(0x2b2d31);
        let border = gpui::black().opacity(0.35);
        let text_col = hsla(0., 0., 0.92, 1.);
        let danger_col = hsla(0., 0.78, 0.72, 1.);
        let hover_bg = gpui::white().opacity(0.06);
        let danger_hover_bg = hsla(0., 0.45, 0.34, 0.26);
        let is_pinned = menu
            .task_id
            .as_deref()
            .is_some_and(|task_id| self.sidebar_task_is_pinned(task_id));

        let pin_task_id = menu.task_id.clone();
        let next_pinned = !is_pinned;
        let pin_label: SharedString = if is_pinned { "Unpin" } else { "Pin" }.into();
        let pin_tooltip = if is_pinned {
            "Unpin this task and return it to the normal task order"
        } else {
            "Pin this task to keep it at the top of the task list"
        };

        let new_task_project_id = menu.root_project_id.clone();
        let new_task_branch_name = menu.branch_name.clone();

        let rename_project_id = menu.project_id.clone();
        let rename_row_id = menu.row_id.clone();
        let rename_task_name = menu.task_name.clone();
        let delete_project_id = menu.project_id.clone();
        let delete_task_id = menu.task_id.clone();
        let delete_task_name = menu.task_name.clone();
        let delete_branch_name = menu.branch_name.clone();
        let delete_is_worktree = menu.is_worktree;
        let delete_preferred_project_id = menu.root_project_id.clone();
        let delete_tooltip = if menu.is_worktree {
            "Delete this worktree task and remove its local branch"
        } else {
            "Delete this direct task from the sidebar"
        };

        div()
            .w(px(TASK_MENU_W))
            .rounded_md()
            .bg(bg)
            .border_1()
            .border_color(border)
            .shadow_md()
            .occlude()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(Self::sidebar_task_menu_item(
                "sidebar-task-menu-pin".into(),
                "assets/icons/icons__pin-off.svg",
                pin_label,
                SidebarTaskMenuItemStyle {
                    tooltip_label: pin_tooltip,
                    text_color: text_col,
                    hover_bg,
                },
                move |this, _ev, _window, cx| {
                    this.sidebar_task_menu = None;
                    if let Some(task_id) = pin_task_id.as_deref() {
                        this.set_sidebar_task_pinned(task_id, next_pinned);
                    }
                    cx.stop_propagation();
                    cx.notify();
                },
                cx,
            ))
            .child(Self::sidebar_task_menu_item(
                "sidebar-task-menu-new-task".into(),
                "assets/icons/icons__git-worktree.svg",
                "New task from current branch".into(),
                SidebarTaskMenuItemStyle {
                    tooltip_label:
                        "Create a new task using this task's current branch as the starting point",
                    text_color: text_col,
                    hover_bg,
                },
                move |this, _ev, _window, cx| {
                    this.sidebar_task_menu = None;
                    this.focus_handle.focus(_window);
                    this.open_new_task_modal_with_branch(
                        &new_task_project_id,
                        &new_task_branch_name,
                        cx,
                    );
                    cx.stop_propagation();
                    cx.notify();
                },
                cx,
            ))
            .child(Self::sidebar_task_menu_item(
                "sidebar-task-menu-rename".into(),
                "assets/icons/icons__edit.svg",
                "Rename".into(),
                SidebarTaskMenuItemStyle {
                    tooltip_label: "Rename this task inline",
                    text_color: text_col,
                    hover_bg,
                },
                move |this, _ev, window, cx| {
                    this.sidebar_task_menu = None;
                    this.focus_handle.focus(window);
                    this.begin_sidebar_task_rename(
                        &rename_project_id,
                        &rename_row_id,
                        &rename_task_name,
                        cx,
                    );
                    cx.stop_propagation();
                },
                cx,
            ))
            .child(Self::sidebar_task_menu_item(
                "sidebar-task-menu-delete".into(),
                "assets/icons/icons__trash.svg",
                "Delete".into(),
                SidebarTaskMenuItemStyle {
                    tooltip_label: delete_tooltip,
                    text_color: danger_col,
                    hover_bg: danger_hover_bg,
                },
                move |this, _ev, _window, cx| {
                    if let Some(task_id) = delete_task_id.clone() {
                        this.request_sidebar_task_delete(
                            SidebarTaskDeleteRequest {
                                project_id: delete_project_id.clone(),
                                task_id,
                                task_name: delete_task_name.clone(),
                                branch_name: delete_branch_name.clone(),
                                is_worktree: delete_is_worktree,
                                preferred_project_id: delete_preferred_project_id.clone(),
                            },
                            cx,
                        );
                    }
                    cx.stop_propagation();
                },
                cx,
            ))
    }

    pub(crate) fn sidebar_task_menu_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = self.sidebar_task_menu.as_ref() else {
            return div().id("sidebar-task-menu-popover");
        };

        let window_w = f32::from(window.bounds().size.width);
        let window_h = f32::from(window.bounds().size.height);
        let left = (menu.anchor_x + 4.0).min((window_w - TASK_MENU_W - 8.0).max(8.0));
        let top = (menu.anchor_y + 4.0).min((window_h - TASK_MENU_H - 8.0).max(8.0));

        div()
            .id("sidebar-task-menu-popover")
            .absolute()
            .left(px(left.max(8.0)))
            .top(px(top.max(8.0)))
            .on_mouse_down_out(cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.sidebar_task_menu = None;
                cx.notify();
            }))
            .child(self.sidebar_task_menu_panel(menu, cx))
    }

    pub(crate) fn sidebar_task_delete_confirm_modal(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(confirm) = self.sidebar_task_delete_confirm.clone() else {
            return div().id("sidebar-task-delete-confirm-overlay");
        };

        let title_col = hsla(0., 0., 0.92, 1.);
        let body_col = hsla(0., 0., 0.74, 1.);
        let border = gpui::white().opacity(0.08);
        let btn_bg = gpui::white().opacity(0.08);
        let btn_hover = gpui::white().opacity(0.14);
        let danger_bg = hsla(0., 0.62, 0.50, 1.);
        let danger_hover = hsla(0., 0.62, 0.58, 1.);
        let worktree_display_name = confirm
            .project_path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| confirm.task_name.clone());
        let message: SharedString = format!(
            "Delete worktree \"{}\" and remove the local branch \"{}\"?",
            worktree_display_name, confirm.branch_name
        )
        .into();

        div()
            .id("sidebar-task-delete-confirm-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.sidebar_task_delete_confirm = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                if this.sidebar_task_delete_confirm.is_none() {
                    return;
                }

                match ev.keystroke.key.as_str() {
                    "escape" => {
                        this.sidebar_task_delete_confirm = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        this.confirm_sidebar_task_delete(cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .w(px(364.))
                    .rounded_lg()
                    .bg(rgb(0x2b2d31))
                    .border_1()
                    .border_color(border)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .px(px(20.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .child(
                                div()
                                    .text_size(rems(14. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child("Confirm Worktree Deletion"),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .text_color(body_col)
                                    .child(message),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(hsla(0., 0., 0.54, 1.))
                                    .child("This permanently removes the worktree folder from disk. Any uncommitted changes inside it will be lost."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .px(px(20.))
                            .pb(px(16.))
                            .pt(px(8.))
                            .child(
                                div()
                                    .id("sidebar-task-delete-confirm-cancel")
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded_md()
                                    .bg(btn_bg)
                                    .hover(move |style| style.bg(btn_hover))
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view(
                                            "Close without deleting the worktree",
                                            cx,
                                        )
                                    })
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(title_col)
                                    .child("Cancel")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.sidebar_task_delete_confirm = None;
                                            cx.stop_propagation();
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("sidebar-task-delete-confirm-ok")
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded_md()
                                    .bg(danger_bg)
                                    .hover(move |style| style.bg(danger_hover))
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view(
                                            "Permanently delete this worktree task",
                                            cx,
                                        )
                                    })
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child("Delete")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.confirm_sidebar_task_delete(cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
                            ),
                    ),
            )
    }

    pub(crate) fn project_remove_confirm_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(confirm) = self.project_remove_confirm.clone() else {
            return div().id("project-remove-confirm-overlay");
        };

        let title_col = hsla(0., 0., 0.92, 1.);
        let body_col = hsla(0., 0., 0.74, 1.);
        let border = gpui::white().opacity(0.08);
        let btn_bg = gpui::white().opacity(0.08);
        let btn_hover = gpui::white().opacity(0.14);
        let danger_bg = hsla(0., 0.62, 0.50, 1.);
        let danger_hover = hsla(0., 0.62, 0.58, 1.);
        let task_label = if confirm.open_task_count == 1 {
            "1 open task".to_string()
        } else {
            format!("{} open tasks", confirm.open_task_count)
        };
        let message: SharedString = format!(
            "\"{}\" still has {}. Remove the project and its tasks from the sidebar?",
            confirm.project_name, task_label
        )
        .into();

        div()
            .id("project-remove-confirm-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.project_remove_confirm = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                if this.project_remove_confirm.is_none() {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "escape" => {
                        this.project_remove_confirm = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        this.confirm_remove_project_group(cx);
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .w(px(344.))
                    .rounded_lg()
                    .bg(rgb(0x2b2d31))
                    .border_1()
                    .border_color(border)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .px(px(20.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .child(
                                div()
                                    .text_size(rems(14. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child("Confirm Project Removal"),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .text_color(body_col)
                                    .child(message),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(hsla(0., 0., 0.54, 1.))
                                    .child("This only removes them from the sidebar. Files and worktrees stay on disk."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .px(px(20.))
                            .pb(px(16.))
                            .pt(px(8.))
                            .child(
                                div()
                                    .id("project-remove-confirm-cancel")
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded_md()
                                    .bg(btn_bg)
                                    .hover(move |style| style.bg(btn_hover))
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view(
                                            "Close without removing the project",
                                            cx,
                                        )
                                    })
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(title_col)
                                    .child("Cancel")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.project_remove_confirm = None;
                                            cx.stop_propagation();
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("project-remove-confirm-ok")
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded_md()
                                    .bg(danger_bg)
                                    .hover(move |style| style.bg(danger_hover))
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view(
                                            "Remove this project group from the sidebar",
                                            cx,
                                        )
                                    })
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child("Remove")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.confirm_remove_project_group(cx);
                                            cx.stop_propagation();
                                        }),
                                    ),
                            ),
                    ),
            )
    }

    pub(crate) fn on_add_project(
        &mut self,
        _ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        let handle = cx.entity().clone();
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Add Project Folder".into()),
        });
        window
            .spawn(cx, async move |async_cx| {
                if let Ok(Ok(Some(paths))) = receiver.await {
                    if let Some(path) = paths.first() {
                        let _ = handle.update(async_cx, |this, cx| {
                            this.begin_add_project(path.clone(), cx);
                        });
                    }
                }
            })
            .detach();
    }

    pub fn sidebar_content(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let bg = theme::chrome_bg(window);
        let header_col = hsla(0., 0., 0.50, 1.);

        let mut col = div()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(bg)
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.commit_sidebar_task_rename(cx);
                    this.sidebar_task_menu = None;
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(36.))
                    .px(px(10.))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(header_col)
                            .child("PROJECTS"),
                    ),
            );

        let list = {
            let workspace = self.workspace_pane.read(cx);
            let active_project_page = workspace.active_project_page.clone();
            let active_section = workspace.active_section.clone();
            let mut list_div = div().flex().flex_col().py(px(4.)).px(px(4.)).gap(px(2.));
            for group in self.sidebar_groups() {
                self.request_project_github_link_lookup(
                    &group.root_project.id,
                    &group.root_project.path,
                );
                let root_id = &group.root_project.id;
                let expand_id = &group.root_project.repo_id;
                let expanded = self.project_expand_target(expand_id);
                let expand_progress = self.project_expand_progress(expand_id);
                let active = active_project_page
                    .as_deref()
                    .is_some_and(|project_id| project_id == root_id);
                list_div = list_div.child(div().child(Self::project_row(
                    &group.root_project,
                    ProjectRowState {
                        github_url: self.project_github_links.get(root_id).cloned(),
                        active,
                        has_children: !group.child_entries.is_empty(),
                        expanded,
                    },
                    window,
                    cx,
                )));

                if expand_progress > 0.001 {
                    let is_animating = self.project_expand_animations.contains_key(expand_id);
                    let full_height = if group.child_entries.is_empty() {
                        0.0
                    } else {
                        (BRANCH_ROW_H + LIST_GAP) * group.child_entries.len() as f32
                    };
                    let animated_height = (full_height * expand_progress).max(0.);
                    let mut children = div().opacity((0.5 + 0.5 * expand_progress).min(1.));

                    if is_animating {
                        children = children.overflow_hidden().h(px(animated_height));
                    }

                    let mut child_list = div()
                        .flex()
                        .flex_col()
                        .gap(px(LIST_GAP))
                        .pt(px((1.0 - expand_progress) * 4.0));
                    for entry in &group.child_entries {
                        self.request_project_pull_request_lookup_for(
                            &entry.project_id,
                            &entry.branch.name,
                            &entry.project_path,
                        );
                        let is_active = active_section.as_ref().is_some_and(|section| {
                            section.task_id.as_deref() == Some(entry.task_id.as_str())
                        });
                        child_list = child_list.child(self.branch_row(entry, is_active, cx));
                    }

                    children = children.child(child_list);
                    list_div = list_div.child(children);
                }
            }
            list_div
        };
        if self.project_store.projects.is_empty() {
            let empty_col = hsla(0., 0., 0.40, 1.);
            col = col.child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_xs()
                    .text_color(empty_col)
                    .child("Click + to add a project"),
            );
        } else {
            col = col.child(
                div()
                    .id("left-sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(list),
            );
        }

        col
    }

    fn render_sidebar_task_name_content(
        task_name: SharedString,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
    ) -> impl IntoElement {
        let text_col = hsla(0., 0., 0.92, 1.);
        let placeholder_col = hsla(0., 0., 0.55, 1.);
        let cursor = cursor.min(task_name.len());
        let selection =
            selection.map(|range| range.start.min(task_name.len())..range.end.min(task_name.len()));

        if task_name.is_empty() {
            return div()
                .flex()
                .items_center()
                .gap(px(0.))
                .text_size(rems(13. / 16.))
                .child(div().w(px(1.)).h(px(16.)).mr(px(1.)).bg(text_col))
                .child(div().text_color(placeholder_col).child("Task name"));
        }

        let selected = selection.filter(|range| range.start < range.end);
        let selected_contains_cursor = selected
            .as_ref()
            .is_some_and(|range| range.start <= cursor && cursor <= range.end);

        let (prefix_end, selected_end) = if let Some(range) = selected.as_ref() {
            (range.start.min(cursor), range.end.min(task_name.len()))
        } else {
            (cursor.min(task_name.len()), cursor.min(task_name.len()))
        };

        let prefix = task_name[..prefix_end].to_string();
        let middle = if let Some(range) = selected.as_ref() {
            task_name[range.clone()].to_string()
        } else {
            String::new()
        };
        let suffix_start = if selected_contains_cursor {
            selected_end
        } else {
            cursor.min(task_name.len())
        };
        let between = if selected.as_ref().is_some_and(|range| range.end < cursor) {
            task_name[selected_end..cursor.min(task_name.len())].to_string()
        } else {
            String::new()
        };
        let trailing = task_name[suffix_start..].to_string();

        let mut row = div()
            .flex()
            .items_center()
            .gap(px(0.))
            .text_size(rems(13. / 16.));

        if !prefix.is_empty() {
            row = row.child(div().text_color(text_col).child(prefix));
        }

        if selected.as_ref().is_some_and(|range| range.end < cursor) && !middle.is_empty() {
            row = row.child(
                div()
                    .px(px(1.))
                    .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                    .text_color(text_col)
                    .child(middle.clone()),
            );
        }

        row = row.child(div().w(px(1.)).h(px(16.)).bg(text_col));

        if selected_contains_cursor && !middle.is_empty() {
            row = row.child(
                div()
                    .px(px(1.))
                    .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                    .text_color(text_col)
                    .child(middle.clone()),
            );
        }

        if !between.is_empty() {
            row = row.child(div().text_color(text_col).child(between));
        }

        if selected.as_ref().is_some_and(|range| range.start > cursor) && !middle.is_empty() {
            row = row.child(
                div()
                    .px(px(1.))
                    .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                    .text_color(text_col)
                    .child(middle),
            );
        }

        if !trailing.is_empty() {
            row = row.child(div().text_color(text_col).child(trailing));
        }

        row
    }
}

enum SidebarCursorDirection {
    Left,
    Right,
}

fn sanitize_sidebar_task_name_input(text: String) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

fn selected_sidebar_task_name_range(
    state: &SidebarTaskRenameState,
) -> Option<std::ops::Range<usize>> {
    let anchor = state.task_name_selection_anchor?;
    if anchor == state.task_name_cursor {
        None
    } else if anchor < state.task_name_cursor {
        Some(anchor..state.task_name_cursor)
    } else {
        Some(state.task_name_cursor..anchor)
    }
}

fn previous_sidebar_task_name_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .rev()
        .find_map(|(index, _)| (index < cursor).then_some(index))
        .unwrap_or(0)
}

fn next_sidebar_task_name_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .find_map(|(index, _)| (index > cursor).then_some(index))
        .unwrap_or(text.len())
}

fn replace_sidebar_task_name_range(
    state: &mut SidebarTaskRenameState,
    range: std::ops::Range<usize>,
    new_text: &str,
) {
    state.task_name.replace_range(range.clone(), new_text);
    state.task_name_cursor = range.start + new_text.len();
    state.task_name_selection_anchor = None;
}

fn insert_sidebar_task_name_text(state: &mut SidebarTaskRenameState, text: &str) {
    let range = selected_sidebar_task_name_range(state)
        .unwrap_or(state.task_name_cursor..state.task_name_cursor);
    replace_sidebar_task_name_range(state, range, text);
}

fn delete_sidebar_task_name_backward(state: &mut SidebarTaskRenameState) {
    if let Some(range) = selected_sidebar_task_name_range(state) {
        replace_sidebar_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor == 0 {
        return;
    }

    let start = previous_sidebar_task_name_boundary(&state.task_name, state.task_name_cursor);
    replace_sidebar_task_name_range(state, start..state.task_name_cursor, "");
}

fn previous_sidebar_task_name_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor;
    while idx > 0 {
        let start = previous_sidebar_task_name_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if !ch.is_whitespace() {
            break;
        }
        idx = start;
    }

    while idx > 0 {
        let start = previous_sidebar_task_name_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if is_sidebar_task_name_word_char(ch) {
            idx = start;
        } else {
            break;
        }
    }

    idx
}

fn is_sidebar_task_name_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-')
}

fn delete_sidebar_task_name_word_backward(state: &mut SidebarTaskRenameState) {
    if let Some(range) = selected_sidebar_task_name_range(state) {
        replace_sidebar_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor == 0 {
        return;
    }

    let start = previous_sidebar_task_name_word_boundary(&state.task_name, state.task_name_cursor);
    replace_sidebar_task_name_range(state, start..state.task_name_cursor, "");
}

fn delete_sidebar_task_name_to_start(state: &mut SidebarTaskRenameState) {
    if let Some(range) = selected_sidebar_task_name_range(state) {
        replace_sidebar_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor == 0 {
        return;
    }

    replace_sidebar_task_name_range(state, 0..state.task_name_cursor, "");
}

fn delete_sidebar_task_name_forward(state: &mut SidebarTaskRenameState) {
    if let Some(range) = selected_sidebar_task_name_range(state) {
        replace_sidebar_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor >= state.task_name.len() {
        return;
    }

    let end = next_sidebar_task_name_boundary(&state.task_name, state.task_name_cursor);
    replace_sidebar_task_name_range(state, state.task_name_cursor..end, "");
}

fn move_sidebar_task_name_cursor(
    state: &mut SidebarTaskRenameState,
    direction: SidebarCursorDirection,
    extend_selection: bool,
) {
    let next_cursor = match direction {
        SidebarCursorDirection::Left => {
            if let Some(range) = selected_sidebar_task_name_range(state) {
                if extend_selection {
                    previous_sidebar_task_name_boundary(&state.task_name, state.task_name_cursor)
                } else {
                    range.start
                }
            } else {
                previous_sidebar_task_name_boundary(&state.task_name, state.task_name_cursor)
            }
        }
        SidebarCursorDirection::Right => {
            if let Some(range) = selected_sidebar_task_name_range(state) {
                if extend_selection {
                    next_sidebar_task_name_boundary(&state.task_name, state.task_name_cursor)
                } else {
                    range.end
                }
            } else {
                next_sidebar_task_name_boundary(&state.task_name, state.task_name_cursor)
            }
        }
    };

    if extend_selection {
        if state.task_name_selection_anchor.is_none() {
            state.task_name_selection_anchor = Some(state.task_name_cursor);
        }
    } else {
        state.task_name_selection_anchor = None;
    }

    state.task_name_cursor = next_cursor;
}

fn move_sidebar_task_name_cursor_to_edge(
    state: &mut SidebarTaskRenameState,
    to_end: bool,
    extend_selection: bool,
) {
    if extend_selection && state.task_name_selection_anchor.is_none() {
        state.task_name_selection_anchor = Some(state.task_name_cursor);
    }
    if !extend_selection {
        state.task_name_selection_anchor = None;
    }
    state.task_name_cursor = if to_end { state.task_name.len() } else { 0 };
}

fn is_terminal_clipboard_shortcut(modifiers: gpui::Modifiers) -> bool {
    if modifiers.alt || modifiers.function {
        return false;
    }
    #[cfg(target_os = "macos")]
    {
        modifiers.platform && !modifiers.control
    }
    #[cfg(not(target_os = "macos"))]
    {
        modifiers.control && !modifiers.platform
    }
}

fn terminal_key_bytes(ev: &KeyDownEvent) -> Option<Vec<u8>> {
    let key = ev.keystroke.key.as_str();
    let modifiers = ev.keystroke.modifiers;

    let mut bytes = match key {
        "enter" => Some(vec![b'\r']),
        "backspace" => Some(vec![0x7f]),
        "tab" if modifiers.shift => Some(b"\x1b[Z".to_vec()),
        "tab" => Some(vec![b'\t']),
        "escape" => Some(vec![0x1b]),
        "up" => Some(b"\x1b[A".to_vec()),
        "down" => Some(b"\x1b[B".to_vec()),
        "right" => Some(b"\x1b[C".to_vec()),
        "left" => Some(b"\x1b[D".to_vec()),
        "home" => Some(b"\x1b[H".to_vec()),
        "end" => Some(b"\x1b[F".to_vec()),
        "pageup" => Some(b"\x1b[5~".to_vec()),
        "pagedown" => Some(b"\x1b[6~".to_vec()),
        "delete" => Some(b"\x1b[3~".to_vec()),
        _ => None,
    };

    #[cfg(target_os = "macos")]
    let control_pressed = modifiers.control;
    #[cfg(not(target_os = "macos"))]
    let control_pressed = modifiers.control || modifiers.platform;

    if control_pressed {
        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
            if let Some(byte) = control_key_byte(key_char) {
                return Some(vec![byte]);
            }
        }
        if let Some(byte) = control_key_byte(key) {
            return Some(vec![byte]);
        }
    }

    if bytes.is_none() {
        #[cfg(target_os = "macos")]
        let has_terminal_modifier = modifiers.control || modifiers.platform || modifiers.function;
        #[cfg(not(target_os = "macos"))]
        let has_terminal_modifier = modifiers.control || modifiers.function;

        if has_terminal_modifier {
            return None;
        }
        let key_char = ev.keystroke.key_char.as_deref()?;
        bytes = Some(key_char.as_bytes().to_vec());
    }

    let mut bytes = bytes?;
    if modifiers.alt {
        let mut prefixed = vec![0x1b];
        prefixed.append(&mut bytes);
        return Some(prefixed);
    }

    Some(bytes)
}

#[cfg(target_os = "macos")]
fn macos_terminal_command_bytes(ev: &KeyDownEvent) -> Option<Vec<u8>> {
    let modifiers = ev.keystroke.modifiers;
    if !modifiers.platform || modifiers.control || modifiers.function {
        return None;
    }

    let byte = match ev.keystroke.key.as_str() {
        "backspace" => 0x15,
        "delete" => 0x0b,
        "left" | "home" => 0x01,
        "right" | "end" => 0x05,
        _ => return None,
    };

    let mut bytes = vec![byte];
    if modifiers.alt {
        bytes.insert(0, 0x1b);
    }
    Some(bytes)
}

fn control_key_byte(value: &str) -> Option<u8> {
    let mut chars = value.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    match ch {
        '@' | ' ' => Some(0),
        'a'..='z' | 'A'..='Z' => Some((ch.to_ascii_uppercase() as u8) & 0x1f),
        '[' => Some(0x1b),
        '\\' => Some(0x1c),
        ']' => Some(0x1d),
        '^' => Some(0x1e),
        '_' => Some(0x1f),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyDownEvent, Keystroke, Modifiers};
    use std::collections::HashSet;
    use std::path::PathBuf;

    fn key_event(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke {
                modifiers,
                key: key.to_string(),
                key_char: key_char.map(str::to_string),
            },
            is_held: false,
        }
    }

    #[test]
    fn terminal_key_bytes_encodes_enter_and_backspace() {
        assert_eq!(
            terminal_key_bytes(&key_event("enter", None, Modifiers::default())),
            Some(vec![b'\r'])
        );
        assert_eq!(
            terminal_key_bytes(&key_event("backspace", None, Modifiers::default())),
            Some(vec![0x7f])
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_terminal_command_bytes_maps_line_editing_shortcuts() {
        assert_eq!(
            macos_terminal_command_bytes(&key_event("backspace", None, Modifiers::command())),
            Some(vec![0x15])
        );
        assert_eq!(
            macos_terminal_command_bytes(&key_event("left", None, Modifiers::command())),
            Some(vec![0x01])
        );
        assert_eq!(
            macos_terminal_command_bytes(&key_event("delete", None, Modifiers::command())),
            Some(vec![0x0b])
        );
    }

    fn sample_project(
        id: &str,
        repo_id: &str,
        kind: crate::project_store::ProjectKind,
        worktree_name: Option<&str>,
    ) -> Project {
        Project {
            id: id.to_string(),
            repo_id: repo_id.to_string(),
            name: id.to_string(),
            path: PathBuf::from(format!("/tmp/{id}")),
            kind,
            checkout: crate::project_store::ProjectCheckoutState::default(),
            branch_settings: crate::project_store::ProjectBranchSettings::default(),
            worktree_name: worktree_name.map(str::to_string),
            repo_common_dir: None,
        }
    }

    #[test]
    fn removed_repo_ids_without_remaining_projects_keeps_expanded_repo_when_root_remains() {
        let projects = vec![
            sample_project(
                "root",
                "repo-a",
                crate::project_store::ProjectKind::Root,
                None,
            ),
            sample_project(
                "wt-1",
                "repo-a",
                crate::project_store::ProjectKind::Worktree,
                Some("wt-1"),
            ),
        ];
        let removed = HashSet::from(["wt-1".to_string()]);

        let removed_repo_ids =
            AnotherOneApp::removed_repo_ids_without_remaining_projects(&projects, &removed);

        assert!(removed_repo_ids.is_empty());
    }

    #[test]
    fn removed_repo_ids_without_remaining_projects_removes_repo_when_last_project_is_removed() {
        let projects = vec![sample_project(
            "root",
            "repo-a",
            crate::project_store::ProjectKind::Root,
            None,
        )];
        let removed = HashSet::from(["root".to_string()]);

        let removed_repo_ids =
            AnotherOneApp::removed_repo_ids_without_remaining_projects(&projects, &removed);

        assert_eq!(removed_repo_ids, HashSet::from(["repo-a".to_string()]));
    }
}

pub(crate) fn open_external_url(url: &str) -> Result<(), String> {
    use crate::platform::{CurrentPlatform, PlatformServices};
    CurrentPlatform::open_external_url(url)
}
