use crate::terminal_runtime::TerminalRuntimeKey;

/// Cross-thread events delivered to the GPUI render thread via the
/// single `AnotherOneApp::app_event_tx` channel. Drained once per
/// render tick in `AnotherOneApp::drain_app_events`.
///
/// ## Two-pattern policy
///
/// This bus carries **push events** — cross-thread notifications that
/// producers deliver asynchronously (daemon acks, background task
/// results). "Polled state" drains (`drain_pending_*`,
/// `drain_git_refresh`, `iroh_client::drain_*`) source from `Vec<T>`
/// fields in `RegistryState` or from global statics; those remain
/// as explicit drain methods on `AnotherOneApp`. The two patterns
/// coexist by design and are not expected to merge.
///
/// ## Invariants
///
/// * All variants must be `Send + 'static` so producers on background
///   threads can clone the sender and post freely.
/// * Each variant maps to exactly one match arm in `drain_app_events`
///   that mutates render-side state and returns `bool` (dirty?).
/// * Handlers do **not** call `cx.notify()` — the render timer does
///   exactly once per tick after collecting all dirty bits.
pub(crate) enum AppEvent {
    /// Daemon confirmed `SetTaskName` with `changed: true`. Patches the
    /// render-side `project_store` immediately so the UI reflects the
    /// new name without waiting for the next 50 ms `ProjectList`
    /// broadcast. See `dispatch_rename_task` for the producer.
    TaskRenameAcked { task_id: String, new_name: String },

    /// A background commit file-change lookup completed. `result` is
    /// `Ok(files)` or `Err(message)`; the handler updates
    /// `commit_file_changes_states` and surfaces a warning toast on
    /// error.
    CommitFileChanges {
        project_id: String,
        commit_id: String,
        result: Result<Vec<crate::project_store::BranchCompareFile>, String>,
    },

    /// A `Control::TerminalSearch` reply arrived. Only applied if the
    /// active `terminal_search` state still matches `query` and `key`.
    TerminalSearchReplyReceived {
        query: String,
        key: TerminalRuntimeKey,
        reply: daemon_proto::TerminalSearchReply,
    },

    /// One-shot: the daemon-host thread either produced an
    /// `EndpointHandle` or failed. Carried as `Result<_, String>`
    /// (not `anyhow::Error`) because we serialize the error at the
    /// thread boundary. After this fires, no further events of this
    /// variant arrive.
    DaemonHandleResolved(Result<daemon::EndpointHandle, String>),

    /// A background changed-file diff load completed. Only applied if
    /// the active `workspace_pane.active_git_diff` still matches
    /// `selection`; stale replies are silently dropped.
    ChangedFileDiffLoaded {
        selection: crate::project_store::GitDiffSelection,
        result: Result<crate::project_store::GitDiff, String>,
    },

    /// A background add-project preparation completed. `result` is
    /// `Ok(prepared)` or `Err(message)`; on success the handler folds
    /// the project into `project_store`, activates its page, and shows
    /// a success toast — or an info toast if the project was already
    /// present (in which case the page is not re-activated).
    ProjectAddCompleted {
        result: Result<crate::project_store::PreparedProject, String>,
    },

    /// A background project GitHub-link lookup completed. The handler
    /// updates `project_github_links`, clears the in-flight entry in
    /// `project_github_link_requests`, and adds the project to
    /// `project_github_link_checked` to permanently suppress re-requests.
    ProjectGitHubLinkReplyReceived {
        project_id: String,
        github_url: Option<String>,
    },
}
