//! Background git actions for toolbar buttons.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

const LEGACY_GIT_COMMIT_DIFF_PATCH_TOKEN: &str = "{{diff_patch}}";
const GIT_PULL_REQUEST_CONTEXT_TOKEN: &str = "{{pull_request_context}}";
const GIT_PULL_REQUEST_FORMAT_CONTRACT: &str = concat!(
    "Response format requirements:\n",
    "- Return only the PR title/body content.\n",
    "- The first line must be the PR title.\n",
    "- The second line must be blank.\n",
    "- The remaining lines are the PR body.\n",
    "- Do not wrap the response in markdown fences.\n",
    "- Do not add commentary before or after the title/body.\n",
    "- Do not surround the title or body with quotes.\n"
);
const GIT_COMMIT_TIMEOUT: Duration = Duration::from_secs(30);
const GIT_COMMIT_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitActionSettings {
    pub commit_generation_script: String,
    pub pr_generation_script: String,
}

impl Default for GitActionSettings {
    fn default() -> Self {
        Self {
            commit_generation_script: default_commit_generation_script().to_string(),
            pr_generation_script: default_pr_generation_script().to_string(),
        }
    }
}

impl GitActionSettings {
    fn commit_generation_script(&self) -> &str {
        let script = self.commit_generation_script.trim();
        if script.is_empty() {
            default_commit_generation_script()
        } else {
            script
        }
    }

    fn pr_generation_script(&self) -> &str {
        let script = self.pr_generation_script.trim();
        if script.is_empty() {
            default_pr_generation_script()
        } else {
            script
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPagePullRequest {
    pub number: u64,
    pub url: String,
    pub title: String,
    pub branch: String,
    pub author: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub draft: bool,
    pub review_required: bool,
    pub review_requested_to_me: bool,
    pub created_by_me: bool,
    pub state: PullRequestState,
}

#[derive(Debug, Deserialize)]
struct GitHubProjectPagePullRequestRecord {
    number: u64,
    url: String,
    title: String,
    #[serde(rename = "headRefName")]
    head_ref_name: Option<String>,
    #[serde(rename = "isDraft")]
    is_draft: Option<bool>,
    additions: Option<i32>,
    deletions: Option<i32>,
    state: Option<String>,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    author: Option<GitHubActorRecord>,
    #[serde(rename = "reviewDecision")]
    review_decision: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubActorRecord {
    login: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestStatus {
    pub number: u64,
    pub url: String,
    pub state: PullRequestState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestCheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestCheck {
    pub name: String,
    pub state: String,
    pub bucket: PullRequestCheckBucket,
    pub description: Option<String>,
    pub link: Option<String>,
    pub duration_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubPullRequestRecord {
    number: u64,
    url: String,
    state: Option<String>,
    #[serde(rename = "mergedAt")]
    merged_at: Option<String>,
    #[serde(rename = "updatedAt")]
    updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolbarGitAction {
    Commit,
    CommitAndPush,
    UndoLastCommit,
    Fetch,
    Pull,
    Push {
        force: bool,
    },
    CreatePr {
        draft: bool,
        base_branch: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub struct ToolbarActionOutcome {
    pub toast_message: String,
    pub warning: bool,
    pub refresh_git_state: bool,
}

#[derive(Debug, Clone)]
pub struct ToolbarActionError {
    pub message: String,
    pub refresh_git_state: bool,
}

#[derive(Debug, Clone)]
struct GeneratedCommitMessage {
    subject: String,
    body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GeneratedPullRequestContent {
    title: String,
    body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SimpleToolbarGitCommand {
    args: &'static [&'static str],
    failure_prefix: &'static str,
    success_toast: &'static str,
    warning: bool,
    refresh_git_state: bool,
}

pub fn execute_toolbar_git_action(
    repo_path: &Path,
    action: ToolbarGitAction,
    settings: GitActionSettings,
    on_progress: &mut dyn FnMut(String),
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    match action {
        ToolbarGitAction::Commit => commit_with_ai(repo_path, false, &settings, on_progress),
        ToolbarGitAction::CommitAndPush => commit_with_ai(repo_path, true, &settings, on_progress),
        ToolbarGitAction::UndoLastCommit => undo_last_commit(repo_path),
        ToolbarGitAction::Fetch => run_simple_git_command(repo_path, ToolbarGitAction::Fetch),
        ToolbarGitAction::Pull => run_simple_git_command(repo_path, ToolbarGitAction::Pull),
        ToolbarGitAction::Push { force } => push_branch(repo_path, force),
        ToolbarGitAction::CreatePr { draft, base_branch } => create_pull_request(
            repo_path,
            draft,
            base_branch.as_deref(),
            &settings,
            on_progress,
        ),
    }
}

fn toolbar_action_outcome(
    toast_message: impl Into<String>,
    warning: bool,
    refresh_git_state: bool,
) -> ToolbarActionOutcome {
    ToolbarActionOutcome {
        toast_message: toast_message.into(),
        warning,
        refresh_git_state,
    }
}

fn simple_toolbar_git_command(action: ToolbarGitAction) -> Option<SimpleToolbarGitCommand> {
    match action {
        ToolbarGitAction::UndoLastCommit => Some(SimpleToolbarGitCommand {
            args: &["reset", "--soft", "HEAD~1"],
            failure_prefix: "Undo last commit failed",
            success_toast: "Undid the last commit.",
            warning: false,
            refresh_git_state: true,
        }),
        ToolbarGitAction::Fetch => Some(SimpleToolbarGitCommand {
            args: &["fetch"],
            failure_prefix: "Fetch failed",
            success_toast: "Fetched remote updates.",
            warning: false,
            refresh_git_state: true,
        }),
        ToolbarGitAction::Pull => Some(SimpleToolbarGitCommand {
            args: &["pull", "--ff-only"],
            failure_prefix: "Pull failed",
            success_toast: "Pulled remote updates with fast-forward only.",
            warning: false,
            refresh_git_state: true,
        }),
        _ => None,
    }
}

fn run_simple_git_command(
    repo_path: &Path,
    action: ToolbarGitAction,
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    let Some(command) = simple_toolbar_git_command(action) else {
        return Err(ToolbarActionError::from_message(
            "The requested git action is not supported.",
        ));
    };

    let output = Command::new("git")
        .args(command.args)
        .current_dir(repo_path)
        .output()
        .map_err(|err| {
            ToolbarActionError::from_message(format!("{}: {err}", command.failure_prefix))
        })?;

    if !output.status.success() {
        return Err(ToolbarActionError::from_message(command_failure(
            command.failure_prefix,
            &output,
        )));
    }

    Ok(toolbar_action_outcome(
        command.success_toast,
        command.warning,
        command.refresh_git_state,
    ))
}

fn undo_last_commit(repo_path: &Path) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    run_simple_git_command(repo_path, ToolbarGitAction::UndoLastCommit)
}

fn git_stdout(repo_path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

pub fn find_github_repo_url(repo_path: &Path) -> Option<String> {
    git_stdout(repo_path, &["remote", "get-url", "origin"])
        .and_then(|remote| normalize_github_remote(&remote))
}

pub fn find_latest_pull_request_status(
    repo_path: &Path,
    head_branch: &str,
) -> Option<PullRequestStatus> {
    if head_branch.trim().is_empty() {
        return None;
    }

    let gh = find_gh_cli()?;
    let output = Command::new(gh)
        .args(find_latest_pull_request_args(head_branch))
        .current_dir(repo_path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let mut pull_requests =
        serde_json::from_slice::<Vec<GitHubPullRequestRecord>>(&output.stdout).ok()?;
    pull_requests.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));

    let pull_request = pull_requests
        .iter()
        .find(|pull_request| normalize_pull_request_state(pull_request) == PullRequestState::Open)
        .or_else(|| pull_requests.first())?;
    let url = pull_request.url.trim();
    if url.is_empty() {
        return None;
    }

    Some(PullRequestStatus {
        number: pull_request.number,
        url: url.to_string(),
        state: normalize_pull_request_state(pull_request),
    })
}

pub fn find_pull_request_checks(
    repo_path: &Path,
    pull_request_number: Option<u64>,
) -> Result<Option<Vec<PullRequestCheck>>, String> {
    let gh = find_gh_cli().ok_or_else(|| {
        "Could not load PR checks. GitHub CLI (`gh`) is not installed or not on the app PATH."
            .to_string()
    })?;
    let mut command = Command::new(gh);
    command.args(["pr", "checks"]);
    if let Some(pull_request_number) = pull_request_number {
        command.arg(pull_request_number.to_string());
    }
    let output = command
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not load PR checks: {err}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let checks = parse_pull_request_checks_output(&stdout);
    if !checks.is_empty() {
        return Ok(Some(checks));
    }

    let detail = [stderr.trim(), stdout.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if indicates_missing_pull_request(&detail) {
        return Ok(None);
    }

    if indicates_missing_pull_request_checks(&detail) {
        return Ok(Some(Vec::new()));
    }

    if output.status.success() {
        return Ok(Some(Vec::new()));
    }

    Err(if detail.is_empty() {
        "Could not load PR checks. No additional details were reported.".to_string()
    } else {
        format!("Could not load PR checks. {detail}")
    })
}

fn parse_pull_request_checks_output(output: &str) -> Vec<PullRequestCheck> {
    output
        .split(['\n', '\r'])
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .flat_map(|line| {
            let columns = line.split('\t').collect::<Vec<_>>();
            let [name, state, duration_text, link, description @ ..] = columns.as_slice() else {
                return Vec::new();
            };

            vec![PullRequestCheck {
                name: name.trim().to_string(),
                state: state.trim().to_string(),
                bucket: normalize_pull_request_check_bucket(state),
                description: {
                    let joined = description.join("\t").trim().to_string();
                    (!joined.is_empty()).then_some(joined)
                },
                link: {
                    let trimmed = link.trim();
                    (!trimmed.is_empty()).then_some(trimmed.to_string())
                },
                duration_text: {
                    let trimmed = duration_text.trim();
                    (!trimmed.is_empty()).then_some(trimmed.to_string())
                },
            }]
        })
        .collect()
}

fn normalize_pull_request_check_bucket(state: &str) -> PullRequestCheckBucket {
    match state.trim().to_ascii_lowercase().as_str() {
        "pass" => PullRequestCheckBucket::Pass,
        "fail" => PullRequestCheckBucket::Fail,
        "skipping" | "skipped" => PullRequestCheckBucket::Skipping,
        "cancel" | "cancelled" => PullRequestCheckBucket::Cancel,
        _ => PullRequestCheckBucket::Pending,
    }
}

fn indicates_missing_pull_request(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("no pull request found")
        || lowered.contains("no pull requests found")
        || lowered.contains("no associated pull requests")
        || lowered.contains("pull request not found")
}

fn indicates_missing_pull_request_checks(text: &str) -> bool {
    text.to_ascii_lowercase()
        .contains("no checks reported on the")
}

fn indicates_missing_git_remote(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    lowered.contains("no git remotes found")
        || lowered.contains("no remotes found")
        || lowered.contains("could not find any git remotes")
}

fn github_https_url(path: &str) -> String {
    format!("https://github.com/{}", path.trim_end_matches(".git"))
}

fn normalize_github_remote(remote: &str) -> Option<String> {
    let remote = remote.trim().trim_end_matches('/');
    if remote.is_empty() {
        return None;
    }

    [
        "git@github.com:",
        "ssh://git@github.com/",
        "https://github.com/",
        "http://github.com/",
    ]
    .into_iter()
    .find_map(|prefix| remote.strip_prefix(prefix))
    .map(github_https_url)
}

fn normalize_pull_request_state(pull_request: &GitHubPullRequestRecord) -> PullRequestState {
    let normalized_state = pull_request.state.as_deref().map(str::trim);
    if pull_request
        .merged_at
        .as_deref()
        .is_some_and(|merged_at| !merged_at.trim().is_empty())
        || normalized_state == Some("MERGED")
    {
        return PullRequestState::Merged;
    }
    if normalized_state == Some("CLOSED") {
        return PullRequestState::Closed;
    }
    PullRequestState::Open
}

fn find_latest_pull_request_args(head_branch: &str) -> Vec<String> {
    [
        "pr",
        "list",
        "--head",
        head_branch,
        "--state",
        "all",
        "--limit",
        "20",
        "--json",
        "number,url,state,mergedAt,updatedAt",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn commit_with_ai(
    repo_path: &Path,
    push_after: bool,
    settings: &GitActionSettings,
    _on_progress: &mut dyn FnMut(String),
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    let _staged_all_changes = ensure_staged_changes(repo_path)?;
    let diff_patch = staged_diff_patch(repo_path).map_err(ToolbarActionError::from_message)?;
    let prompt = render_commit_generation_script(settings.commit_generation_script(), &diff_patch);
    let generated =
        generate_commit_message(repo_path, &prompt).map_err(ToolbarActionError::from_message)?;
    git_commit(repo_path, &generated).map_err(ToolbarActionError::from_message)?;

    if push_after {
        match push_branch(repo_path, false) {
            Ok(_) => Ok(toolbar_action_outcome(
                format!("Committed and pushed: {}", generated.subject),
                false,
                true,
            )),
            Err(error) => Err(ToolbarActionError {
                message: format!(
                    "Committed staged changes as \"{}\", but push failed. {}",
                    generated.subject, error.message
                ),
                refresh_git_state: true,
            }),
        }
    } else {
        Ok(toolbar_action_outcome(
            format!("Committed staged changes: {}", generated.subject),
            false,
            true,
        ))
    }
}

fn ensure_staged_changes(repo_path: &Path) -> Result<bool, ToolbarActionError> {
    if has_staged_changes(repo_path).map_err(ToolbarActionError::from_message)? {
        return Ok(false);
    }

    if !has_worktree_changes(repo_path).map_err(ToolbarActionError::from_message)? {
        return Err(ToolbarActionError::from_message(
            "Commit failed. There are no local changes to commit.",
        ));
    }

    stage_all_changes(repo_path)?;
    Ok(true)
}

fn push_branch(repo_path: &Path, force: bool) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    let output = git_push_output(repo_path, force, None, None)
        .map_err(|err| ToolbarActionError::from_message(format!("Push failed: {err}")))?;

    if !output.status.success() && is_missing_upstream_push_error(&output) {
        if let Some(branch_name) = git_current_branch(repo_path) {
            if let Some(remote_name) = git_push_remote(repo_path, &branch_name) {
                let retry =
                    git_push_output(repo_path, force, Some(&remote_name), Some(&branch_name))
                        .map_err(|err| {
                            ToolbarActionError::from_message(format!(
                                "{}: {err}",
                                if force {
                                    "Force push failed"
                                } else {
                                    "Push failed"
                                }
                            ))
                        })?;

                if retry.status.success() {
                    return Ok(toolbar_action_outcome(
                        if force {
                            format!("Force-pushed {branch_name} to {remote_name} and set upstream.")
                        } else {
                            format!("Pushed {branch_name} to {remote_name} and set upstream.")
                        },
                        force,
                        false,
                    ));
                }

                return Err(ToolbarActionError::from_message(command_failure(
                    if force {
                        "Force push failed"
                    } else {
                        "Push failed"
                    },
                    &retry,
                )));
            }
        }
    }

    if !output.status.success() {
        return Err(ToolbarActionError::from_message(command_failure(
            if force {
                "Force push failed"
            } else {
                "Push failed"
            },
            &output,
        )));
    }

    Ok(toolbar_action_outcome(
        if force {
            "Force-pushed the current branch with lease."
        } else {
            "Pushed the current branch to its remote."
        },
        force,
        false,
    ))
}

fn git_push_output(
    repo_path: &Path,
    force: bool,
    remote_name: Option<&str>,
    branch_name: Option<&str>,
) -> std::io::Result<Output> {
    let mut command = Command::new("git");
    command.arg("push");
    if force {
        command.arg("--force-with-lease");
    }
    if let (Some(remote_name), Some(branch_name)) = (remote_name, branch_name) {
        command
            .arg("--set-upstream")
            .arg(remote_name)
            .arg(branch_name);
    }
    command.current_dir(repo_path).output()
}

fn is_missing_upstream_push_error(output: &Output) -> bool {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stderr}\n{stdout}");
    combined.contains("has no upstream branch")
        || combined.contains("git push --set-upstream")
        || combined.contains("push.autoSetupRemote")
}

fn git_push_remote(repo_path: &Path, branch_name: &str) -> Option<String> {
    git_stdout(
        repo_path,
        &["config", &format!("branch.{branch_name}.pushRemote")],
    )
    .or_else(|| git_stdout(repo_path, &["config", "remote.pushDefault"]))
    .or_else(|| {
        git_stdout(
            repo_path,
            &["config", &format!("branch.{branch_name}.remote")],
        )
    })
    .or_else(|| {
        let remotes = git_stdout(repo_path, &["remote"])?;
        let mut remote_names = remotes
            .lines()
            .map(str::trim)
            .filter(|remote| !remote.is_empty())
            .collect::<Vec<_>>();
        if remote_names.is_empty() {
            None
        } else if let Some(origin) = remote_names.iter().find(|remote| **remote == "origin") {
            Some((*origin).to_string())
        } else {
            Some(remote_names.remove(0).to_string())
        }
    })
}

fn create_pull_request(
    repo_path: &Path,
    draft: bool,
    base_branch: Option<&str>,
    settings: &GitActionSettings,
    _on_progress: &mut dyn FnMut(String),
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    let gh = find_gh_cli().ok_or_else(|| {
        ToolbarActionError::from_message(
            "Create PR failed. GitHub CLI (`gh`) is not installed or not on the app PATH.",
        )
    })?;
    let head_branch = git_current_branch(repo_path).ok_or_else(|| {
        ToolbarActionError::from_message(
            "Create PR failed. Could not determine the current branch.",
        )
    })?;
    let generated =
        generate_pull_request_content(repo_path, base_branch, settings).map_err(|message| {
            ToolbarActionError::from_message(format!("Create PR failed. {message}"))
        })?;

    let mut cmd = Command::new(gh);
    cmd.args(create_pull_request_args(
        &head_branch,
        draft,
        base_branch,
        &generated.title,
        &generated.body,
    ))
    .current_dir(repo_path);

    let output = cmd
        .output()
        .map_err(|err| ToolbarActionError::from_message(format!("Create PR failed: {err}")))?;

    if !output.status.success() {
        return Err(ToolbarActionError::from_message(command_failure(
            if draft {
                "Draft PR creation failed"
            } else {
                "PR creation failed"
            },
            &output,
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let url = extract_url(&stdout);

    Ok(toolbar_action_outcome(
        match (draft, url.as_deref(), base_branch) {
            (true, Some(url), Some(base_branch)) => {
                format!("Created draft pull request into {base_branch}: {url}")
            }
            (false, Some(url), Some(base_branch)) => {
                format!("Created pull request into {base_branch}: {url}")
            }
            (true, None, Some(base_branch)) => {
                format!("Created draft pull request into {base_branch}.")
            }
            (false, None, Some(base_branch)) => {
                format!("Created pull request into {base_branch}.")
            }
            (true, Some(url), None) => format!("Created draft pull request: {url}"),
            (false, Some(url), None) => format!("Created pull request: {url}"),
            (true, None, None) => "Created draft pull request.".to_string(),
            (false, None, None) => "Created pull request.".to_string(),
        },
        false,
        false,
    ))
}

fn create_pull_request_args(
    head_branch: &str,
    draft: bool,
    base_branch: Option<&str>,
    title: &str,
    body: &str,
) -> Vec<String> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--head".to_string(),
        head_branch.to_string(),
        "--title".to_string(),
        title.to_string(),
        "--body".to_string(),
        body.to_string(),
    ];
    if let Some(base_branch) = base_branch {
        args.push("--base".to_string());
        args.push(base_branch.to_string());
    }
    if draft {
        args.push("--draft".to_string());
    }
    args
}

impl ToolbarActionError {
    fn from_message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            refresh_git_state: false,
        }
    }
}

fn has_staged_changes(repo_path: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not inspect staged changes: {err}"))?;

    match output.status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(command_failure("Could not inspect staged changes", &output)),
    }
}

fn has_worktree_changes(repo_path: &Path) -> Result<bool, String> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not inspect local changes: {err}"))?;

    if !output.status.success() {
        return Err(command_failure("Could not inspect local changes", &output));
    }

    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn stage_all_changes(repo_path: &Path) -> Result<(), ToolbarActionError> {
    let output = Command::new("git")
        .args(["add", "-A"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| {
            ToolbarActionError::from_message(format!("Could not stage changes: {err}"))
        })?;

    if !output.status.success() {
        return Err(ToolbarActionError::from_message(command_failure(
            "Could not stage changes",
            &output,
        )));
    }

    Ok(())
}

fn staged_diff_patch(repo_path: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--no-ext-diff", "--no-color"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not read the staged diff: {err}"))?;

    if !output.status.success() {
        return Err(command_failure("Could not read the staged diff", &output));
    }

    let patch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if patch.is_empty() {
        return Err("Commit failed. There are no staged changes to commit.".to_string());
    }

    Ok(patch)
}

pub fn default_commit_generation_script() -> &'static str {
    concat!(
        "Generate a git commit message for these staged changes.\n",
        "Return only the commit message text.\n",
        "Rules:\n",
        "- Prefer Conventional Commit style when it fits.\n",
        "- First line must be a concise subject in imperative mood.\n",
        "- Keep the subject under 72 characters.\n",
        "- Add a blank line plus a short body only if it materially helps.\n",
        "- No markdown fences, no commentary, no quotes.\n"
    )
}

pub fn default_pr_generation_script() -> &'static str {
    concat!(
        "Generate a GitHub pull request title and body for these branch changes.\n",
        "Focus on the substance of the change, not the git mechanics.\n",
        "Rules:\n",
        "- Write a concise, specific PR title.\n",
        "- The body should summarize what changed and any important reviewer context.\n",
        "- Mention notable user-visible behavior changes, refactors, fixes, or follow-up context when relevant.\n",
        "- Keep the body skimmable and avoid filler.\n"
    )
}

fn render_commit_generation_script(script_template: &str, diff_patch: &str) -> String {
    if script_template.contains(LEGACY_GIT_COMMIT_DIFF_PATCH_TOKEN) {
        return script_template.replace(LEGACY_GIT_COMMIT_DIFF_PATCH_TOKEN, diff_patch);
    }

    format!("{script_template}\n\nStaged patch:\n{diff_patch}\n")
}

fn render_pull_request_generation_script(
    script_template: &str,
    base_branch: &str,
    commit_list: &str,
    diff_patch: &str,
) -> String {
    let context = format!(
        "Base branch: {base_branch}\n\nBranch-only commits:\n{commit_list}\n\nDiff against {base_branch}:\n{diff_patch}\n"
    );
    let script_with_contract = format!(
        "{}\n\n{}",
        script_template.trim(),
        GIT_PULL_REQUEST_FORMAT_CONTRACT.trim_end()
    );

    if script_template.contains(GIT_PULL_REQUEST_CONTEXT_TOKEN) {
        return script_with_contract.replace(GIT_PULL_REQUEST_CONTEXT_TOKEN, context.trim_end());
    }

    format!("{script_with_contract}\n\n{context}")
}

fn generate_commit_message(
    repo_path: &Path,
    prompt: &str,
) -> Result<GeneratedCommitMessage, String> {
    let raw = match run_codex(prompt, repo_path, "codex-commit", "commit message") {
        Ok(raw) => raw,
        Err(codex_err) => match run_claude(prompt, repo_path, "commit message") {
            Ok(raw) => raw,
            Err(claude_err) => {
                return Err(format!(
                    "Commit message generation failed. Codex error: {codex_err} Claude error: {claude_err}"
                ))
            }
        },
    };

    parse_commit_message(&raw)
}

fn generate_pull_request_content(
    repo_path: &Path,
    selected_base_branch: Option<&str>,
    settings: &GitActionSettings,
) -> Result<GeneratedPullRequestContent, String> {
    let base_branch = resolve_pull_request_base_branch(repo_path, selected_base_branch)?;
    let commit_list = branch_commit_list(repo_path, &base_branch)?;
    let diff_patch = branch_diff_patch(repo_path, &base_branch)?;
    let prompt = render_pull_request_generation_script(
        settings.pr_generation_script(),
        &base_branch,
        &commit_list,
        &diff_patch,
    );
    let raw = match run_codex(prompt.as_str(), repo_path, "codex-pr", "PR title/body") {
        Ok(raw) => raw,
        Err(codex_err) => match run_claude(prompt.as_str(), repo_path, "PR title/body") {
            Ok(raw) => raw,
            Err(claude_err) => {
                return Err(format!(
                    "PR title/body generation failed. Codex error: {codex_err} Claude error: {claude_err}"
                ))
            }
        },
    };

    parse_pull_request_content(&raw)
}

fn resolve_pull_request_base_branch(
    repo_path: &Path,
    selected_base_branch: Option<&str>,
) -> Result<String, String> {
    if let Some(base_branch) = selected_base_branch
        .map(str::trim)
        .filter(|branch| !branch.is_empty())
    {
        return Ok(base_branch.to_string());
    }

    if let Some(default_remote_branch) = git_stdout(
        repo_path,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    ) {
        return Ok(default_remote_branch
            .trim_start_matches("origin/")
            .to_string());
    }

    ["main", "master"]
        .into_iter()
        .find_map(|branch| {
            git_stdout(repo_path, &["rev-parse", "--verify", "--quiet", branch]).map(|_| branch)
        })
        .map(str::to_string)
        .ok_or_else(|| {
            "Could not determine the PR base branch. Set a default target branch in project settings or configure origin/HEAD."
                .to_string()
        })
}

fn branch_commit_list(repo_path: &Path, base_branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args([
            "log",
            "--no-decorate",
            "--pretty=format:%h %s",
            &format!("{base_branch}..HEAD"),
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not read branch commits for PR generation: {err}"))?;

    if !output.status.success() {
        return Err(command_failure(
            "Could not read branch commits for PR generation",
            &output,
        ));
    }

    let commits = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok(if commits.is_empty() {
        format!("No unique commits found between {base_branch} and HEAD.")
    } else {
        commits
    })
}

fn branch_diff_patch(repo_path: &Path, base_branch: &str) -> Result<String, String> {
    let output = Command::new("git")
        .args([
            "diff",
            "--no-ext-diff",
            "--no-color",
            &format!("{base_branch}...HEAD"),
        ])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not read the branch diff for PR generation: {err}"))?;

    if !output.status.success() {
        return Err(command_failure(
            "Could not read the branch diff for PR generation",
            &output,
        ));
    }

    let patch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if patch.is_empty() {
        return Err(format!(
            "There are no branch changes to describe relative to {base_branch}."
        ));
    }

    Ok(patch)
}

fn run_codex(
    prompt: &str,
    repo_path: &Path,
    output_prefix: &str,
    empty_output_label: &str,
) -> Result<String, String> {
    let codex = find_codex_cli().ok_or_else(|| "Codex CLI was not found.".to_string())?;
    let output_path = temp_output_path(output_prefix);
    let mut cmd = Command::new(codex);
    cmd.args([
        "exec",
        "--model",
        "gpt-5.4-mini",
        "--sandbox",
        "read-only",
        "--output-last-message",
    ])
    .arg(&output_path)
    .arg("-")
    .current_dir(repo_path)
    .stdin(Stdio::piped())
    .stdout(Stdio::null())
    .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|err| format!("Could not start Codex CLI: {err}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(prompt.as_bytes())
            .map_err(|err| format!("Could not write the Codex prompt: {err}"))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|err| format!("Codex did not complete: {err}"))?;

    if !output.status.success() {
        let _ = fs::remove_file(&output_path);
        return Err(command_failure("Codex message generation failed", &output));
    }

    let raw = fs::read_to_string(&output_path)
        .map_err(|err| format!("Could not read Codex output: {err}"))?;
    let _ = fs::remove_file(&output_path);
    if raw.trim().is_empty() {
        return Err(format!("Codex returned an empty {empty_output_label}."));
    }

    Ok(raw)
}

fn run_claude(prompt: &str, repo_path: &Path, empty_output_label: &str) -> Result<String, String> {
    let claude = find_claude_cli().ok_or_else(|| "Claude CLI was not found.".to_string())?;
    let output = Command::new(claude)
        .args([
            "-p",
            "--model",
            "haiku",
            "--output-format",
            "text",
            "--tools",
            "",
        ])
        .arg(prompt)
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not start Claude CLI: {err}"))?;

    if !output.status.success() {
        return Err(command_failure("Claude message generation failed", &output));
    }

    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        return Err(format!("Claude returned an empty {empty_output_label}."));
    }

    Ok(raw)
}

fn parse_commit_message(raw: &str) -> Result<GeneratedCommitMessage, String> {
    let normalized = normalize_generated_output(raw);

    let mut lines = normalized.lines().map(str::trim_end).collect::<Vec<_>>();
    while matches!(lines.first(), Some(line) if line.trim().is_empty()) {
        lines.remove(0);
    }
    while matches!(lines.last(), Some(line) if line.trim().is_empty()) {
        lines.pop();
    }

    let Some(subject) = lines
        .first()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
    else {
        return Err("The AI generator returned an empty commit message.".to_string());
    };

    let mut body_start = 1usize;
    while body_start < lines.len() && lines[body_start].trim().is_empty() {
        body_start += 1;
    }
    let body = if body_start < lines.len() {
        Some(lines[body_start..].join("\n").trim().to_string()).filter(|body| !body.is_empty())
    } else {
        None
    };

    Ok(GeneratedCommitMessage {
        subject: subject.to_string(),
        body,
    })
}

fn parse_pull_request_content(raw: &str) -> Result<GeneratedPullRequestContent, String> {
    let normalized = normalize_generated_output(raw);
    let mut lines = normalized.lines().map(str::trim_end).collect::<Vec<_>>();
    while matches!(lines.first(), Some(line) if line.trim().is_empty()) {
        lines.remove(0);
    }
    while matches!(lines.last(), Some(line) if line.trim().is_empty()) {
        lines.pop();
    }

    if matches!(
        lines.first(),
        Some(line)
            if line.trim().eq_ignore_ascii_case("pull request")
                || line.trim().eq_ignore_ascii_case("pr")
                || line.trim().eq_ignore_ascii_case("pull request title")
    ) {
        lines.remove(0);
        while matches!(lines.first(), Some(line) if line.trim().is_empty()) {
            lines.remove(0);
        }
    }

    let Some(first_line) = lines
        .first()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
    else {
        return Err("The AI generator returned an empty PR title/body response.".to_string());
    };
    let title = first_line
        .strip_prefix("Title:")
        .or_else(|| first_line.strip_prefix("PR Title:"))
        .map(str::trim)
        .unwrap_or(first_line);
    if title.is_empty() {
        return Err("The AI generator returned an empty PR title.".to_string());
    }

    let mut body_start = 1usize;
    while body_start < lines.len() && lines[body_start].trim().is_empty() {
        body_start += 1;
    }
    let body = if body_start < lines.len() {
        let first_body_line = lines[body_start]
            .trim()
            .strip_prefix("Body:")
            .map(str::trim)
            .unwrap_or(lines[body_start].trim());
        let mut body_lines = vec![first_body_line.to_string()];
        body_lines.extend(
            lines[body_start + 1..]
                .iter()
                .map(|line| line.trim_end().to_string()),
        );
        body_lines.join("\n").trim().to_string()
    } else {
        String::new()
    };

    Ok(GeneratedPullRequestContent {
        title: title.to_string(),
        body,
    })
}

fn normalize_generated_output(raw: &str) -> &str {
    let normalized = raw.trim().trim_matches('`').trim();
    let normalized = normalized
        .strip_prefix("```")
        .and_then(|text| text.strip_suffix("```"))
        .unwrap_or(normalized)
        .trim();

    normalized
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(normalized)
        .trim()
}

fn git_commit(repo_path: &Path, message: &GeneratedCommitMessage) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.arg("commit").arg("-m").arg(&message.subject);
    if let Some(body) = message.body.as_ref() {
        cmd.arg("-m").arg(body);
    }

    let mut child = cmd
        .current_dir(repo_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| format!("Commit failed: {err}"))?;

    let deadline = Instant::now() + GIT_COMMIT_TIMEOUT;
    loop {
        match child
            .try_wait()
            .map_err(|err| format!("Commit failed: {err}"))?
        {
            Some(_status) => break,
            None if Instant::now() < deadline => thread::sleep(GIT_COMMIT_POLL_INTERVAL),
            None => {
                let _ = child.kill();
                let output = child.wait_with_output().map_err(|err| {
                    format!("Commit timed out and could not be cleaned up: {err}")
                })?;
                let details = command_output_details(&output);
                return Err(format!(
                    "Commit timed out after {} seconds. This usually means a git hook or signing step is waiting or running too long.{}",
                    GIT_COMMIT_TIMEOUT.as_secs(),
                    if details.is_empty() {
                        String::new()
                    } else {
                        format!(" {details}")
                    }
                ));
            }
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|err| format!("Commit failed: {err}"))?;

    if !output.status.success() {
        return Err(command_failure("Commit failed", &output));
    }

    Ok(())
}
fn command_output_details(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        format!("stderr: {stderr}")
    } else if !stdout.is_empty() {
        format!("stdout: {stdout}")
    } else {
        String::new()
    }
}

fn git_current_branch(repo_path: &Path) -> Option<String> {
    git_stdout(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|branch| branch != "HEAD")
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

fn extract_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|token| token.starts_with("http://") || token.starts_with("https://"))
        .map(|token| token.trim_end_matches(['.', ')', ']']).to_string())
}

fn temp_output_path(prefix: &str) -> PathBuf {
    env::temp_dir().join(format!("{prefix}-{}.txt", uuid::Uuid::new_v4()))
}

fn find_codex_cli() -> Option<PathBuf> {
    find_executable(
        "codex",
        &[PathBuf::from(
            "/Applications/Codex.app/Contents/Resources/codex",
        )],
    )
}

fn find_claude_cli() -> Option<PathBuf> {
    let mut fallbacks = Vec::new();
    if let Some(home) = dirs::home_dir() {
        fallbacks.push(home.join(".local/bin/claude"));
    }
    find_executable("claude", &fallbacks)
}

fn find_gh_cli() -> Option<PathBuf> {
    find_executable(
        "gh",
        &[
            PathBuf::from("/opt/homebrew/bin/gh"),
            PathBuf::from("/usr/local/bin/gh"),
        ],
    )
}

fn find_executable(command: &str, fallbacks: &[PathBuf]) -> Option<PathBuf> {
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let candidate = dir.join(command);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    fallbacks.iter().find(|path| path.is_file()).cloned()
}

#[cfg(test)]
mod tests {
    use super::{
        create_pull_request_args, default_commit_generation_script, default_pr_generation_script,
        find_latest_pull_request_args, git_stdout, indicates_missing_git_remote,
        indicates_missing_pull_request, indicates_missing_pull_request_checks,
        normalize_github_remote, normalize_pull_request_check_bucket, parse_commit_message,
        parse_pull_request_checks_output, parse_pull_request_content, push_branch,
        render_commit_generation_script, render_pull_request_generation_script,
        simple_toolbar_git_command, PullRequestCheckBucket, ToolbarGitAction,
        GIT_PULL_REQUEST_CONTEXT_TOKEN, GIT_PULL_REQUEST_FORMAT_CONTRACT,
    };
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    #[test]
    fn normalizes_supported_github_remote_formats() {
        assert_eq!(
            normalize_github_remote("git@github.com:owner/repo.git").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            normalize_github_remote("ssh://git@github.com/owner/repo/").as_deref(),
            Some("https://github.com/owner/repo")
        );
        assert_eq!(
            normalize_github_remote("http://github.com/owner/repo").as_deref(),
            Some("https://github.com/owner/repo")
        );
    }

    #[test]
    fn parse_commit_message_trims_fences_and_blank_lines() {
        let message = parse_commit_message("```feat: simplify parser\n\nKeeps behavior.\n```")
            .expect("commit message should parse");

        assert_eq!(message.subject, "feat: simplify parser");
        assert_eq!(message.body.as_deref(), Some("Keeps behavior."));
    }

    #[test]
    fn simple_toolbar_git_command_uses_fetch_args_and_refreshes_state() {
        let command = simple_toolbar_git_command(ToolbarGitAction::Fetch)
            .expect("fetch should use a simple git command");

        assert_eq!(command.args, &["fetch"]);
        assert_eq!(command.failure_prefix, "Fetch failed");
        assert_eq!(command.success_toast, "Fetched remote updates.");
        assert!(command.refresh_git_state);
    }

    #[test]
    fn simple_toolbar_git_command_uses_soft_reset_for_undo_last_commit() {
        let command = simple_toolbar_git_command(ToolbarGitAction::UndoLastCommit)
            .expect("undo last commit should use a simple git command");

        assert_eq!(command.args, &["reset", "--soft", "HEAD~1"]);
        assert_eq!(command.failure_prefix, "Undo last commit failed");
        assert_eq!(command.success_toast, "Undid the last commit.");
        assert!(command.warning);
        assert!(command.refresh_git_state);
    }

    #[test]
    fn simple_toolbar_git_command_uses_ff_only_pull_args_and_refreshes_state() {
        let command = simple_toolbar_git_command(ToolbarGitAction::Pull)
            .expect("pull should use a simple git command");

        assert_eq!(command.args, &["pull", "--ff-only"]);
        assert_eq!(command.failure_prefix, "Pull failed");
        assert_eq!(
            command.success_toast,
            "Pulled remote updates with fast-forward only."
        );
        assert!(command.refresh_git_state);
    }

    #[test]
    fn create_pull_request_args_include_base_when_configured() {
        let args = create_pull_request_args("feature/test", false, Some("main"), "Add PR AI", "");

        assert_eq!(
            args,
            [
                "pr",
                "create",
                "--head",
                "feature/test",
                "--title",
                "Add PR AI",
                "--body",
                "",
                "--base",
                "main"
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn create_pull_request_args_omit_base_when_unset() {
        let args = create_pull_request_args("feature/test", true, None, "Add PR AI", "Body");

        assert_eq!(
            args,
            [
                "pr",
                "create",
                "--head",
                "feature/test",
                "--title",
                "Add PR AI",
                "--body",
                "Body",
                "--draft"
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn find_existing_pull_request_args_target_branch_lookup() {
        let args = find_latest_pull_request_args("feature/test");

        assert_eq!(
            args,
            [
                "pr",
                "list",
                "--head",
                "feature/test",
                "--state",
                "all",
                "--limit",
                "20",
                "--json",
                "number,url,state,mergedAt,updatedAt"
            ]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn normalize_pull_request_check_bucket_maps_expected_states() {
        assert_eq!(
            normalize_pull_request_check_bucket("pass"),
            PullRequestCheckBucket::Pass
        );
        assert_eq!(
            normalize_pull_request_check_bucket("fail"),
            PullRequestCheckBucket::Fail
        );
        assert_eq!(
            normalize_pull_request_check_bucket("skipped"),
            PullRequestCheckBucket::Skipping
        );
        assert_eq!(
            normalize_pull_request_check_bucket("cancelled"),
            PullRequestCheckBucket::Cancel
        );
        assert_eq!(
            normalize_pull_request_check_bucket("pending"),
            PullRequestCheckBucket::Pending
        );
    }

    #[test]
    fn parse_pull_request_checks_output_parses_tab_separated_rows() {
        let checks = parse_pull_request_checks_output(
            "build\tpass\t1m 12s\thttps://example.com/build\tmain workflow\nunit\tfail\t\t\t",
        );

        assert_eq!(checks.len(), 2);
        assert_eq!(checks[0].name, "build");
        assert_eq!(checks[0].bucket, PullRequestCheckBucket::Pass);
        assert_eq!(checks[0].duration_text.as_deref(), Some("1m 12s"));
        assert_eq!(checks[0].link.as_deref(), Some("https://example.com/build"));
        assert_eq!(checks[0].description.as_deref(), Some("main workflow"));
        assert_eq!(checks[1].bucket, PullRequestCheckBucket::Fail);
        assert!(checks[1].duration_text.is_none());
        assert!(checks[1].link.is_none());
        assert!(checks[1].description.is_none());
    }

    #[test]
    fn indicates_missing_pull_request_matches_common_gh_messages() {
        assert!(indicates_missing_pull_request(
            "no pull requests found for branch feature/test"
        ));
        assert!(indicates_missing_pull_request("pull request not found"));
        assert!(!indicates_missing_pull_request("GraphQL request failed"));
    }

    #[test]
    fn indicates_missing_pull_request_checks_matches_gh_message() {
        assert!(indicates_missing_pull_request_checks(
            "no checks reported on the 'main' branch"
        ));
        assert!(!indicates_missing_pull_request_checks(
            "no pull requests found for branch feature/test"
        ));
    }

    #[test]
    fn indicates_missing_git_remote_matches_common_gh_messages() {
        assert!(indicates_missing_git_remote("no git remotes found"));
        assert!(indicates_missing_git_remote(
            "Could not load pull requests. no git remotes found"
        ));
        assert!(!indicates_missing_git_remote("GraphQL request failed"));
    }

    #[test]
    fn default_commit_generation_script_contains_only_instructions() {
        let script = default_commit_generation_script();

        assert!(script.contains("Generate a git commit message"));
        assert!(!script.contains("{{diff_patch}}"));
    }

    #[test]
    fn default_pr_generation_script_contains_only_instructions() {
        let script = default_pr_generation_script();

        assert!(script.contains("Generate a GitHub pull request title and body"));
        assert!(!script.contains(GIT_PULL_REQUEST_CONTEXT_TOKEN));
    }

    #[test]
    fn render_commit_generation_script_replaces_diff_tokens() {
        let rendered = render_commit_generation_script("Patch:\n{{diff_patch}}", "diff --git a b");

        assert_eq!(rendered, "Patch:\ndiff --git a b");
    }

    #[test]
    fn render_commit_generation_script_appends_patch_when_no_placeholder_is_present() {
        let rendered =
            render_commit_generation_script("Use conventional commits.", "diff --git a b");

        assert_eq!(
            rendered,
            "Use conventional commits.\n\nStaged patch:\ndiff --git a b\n"
        );
    }

    #[test]
    fn render_pull_request_generation_script_replaces_context_tokens() {
        let rendered = render_pull_request_generation_script(
            "Summarize:\n{{pull_request_context}}",
            "main",
            "abc123 feat: add PR AI",
            "diff --git a b",
        );

        assert_eq!(
            rendered,
            format!(
                "Summarize:\nBase branch: main\n\nBranch-only commits:\nabc123 feat: add PR AI\n\nDiff against main:\ndiff --git a b\n\n{}",
                GIT_PULL_REQUEST_FORMAT_CONTRACT.trim_end()
            )
        );
    }

    #[test]
    fn render_pull_request_generation_script_appends_context_when_no_placeholder_is_present() {
        let rendered = render_pull_request_generation_script(
            "Write a concise PR summary.",
            "main",
            "abc123 feat: add PR AI",
            "diff --git a b",
        );

        assert_eq!(
            rendered,
            format!(
                "Write a concise PR summary.\n\n{}\n\nBase branch: main\n\nBranch-only commits:\nabc123 feat: add PR AI\n\nDiff against main:\ndiff --git a b\n",
                GIT_PULL_REQUEST_FORMAT_CONTRACT.trim_end()
            )
        );
    }

    #[test]
    fn parse_pull_request_content_accepts_title_only_output() {
        let generated = parse_pull_request_content("feat: add AI PR creation")
            .expect("PR title-only response should parse");

        assert_eq!(generated.title, "feat: add AI PR creation");
        assert!(generated.body.is_empty());
    }

    #[test]
    fn parse_pull_request_content_accepts_title_and_body_output() {
        let generated = parse_pull_request_content(
            "feat: add AI PR creation\n\n## Summary\n- Adds AI PR generation.",
        )
        .expect("PR title/body response should parse");

        assert_eq!(generated.title, "feat: add AI PR creation");
        assert_eq!(generated.body, "## Summary\n- Adds AI PR generation.");
    }

    #[test]
    fn parse_pull_request_content_normalizes_fences_and_labels() {
        let generated = parse_pull_request_content(
            "```Title: feat: add AI PR creation\n\nBody: Summarize changes for reviewers.\n```",
        )
        .expect("fenced PR response should parse");

        assert_eq!(generated.title, "feat: add AI PR creation");
        assert_eq!(generated.body, "Summarize changes for reviewers.");
    }

    #[test]
    fn parse_pull_request_content_rejects_empty_output() {
        let error =
            parse_pull_request_content("``` \n```").expect_err("empty PR response should fail");

        assert_eq!(
            error,
            "The AI generator returned an empty PR title/body response."
        );
    }

    #[test]
    fn push_branch_sets_upstream_when_branch_has_no_tracking_remote() {
        let temp_dir = TempDir::new().expect("tempdir should exist");
        let remote_path = temp_dir.path().join("remote.git");
        let repo_path = temp_dir.path().join("repo");

        run_git(
            temp_dir.path(),
            &["init", "--bare", remote_path.to_str().unwrap()],
        );
        run_git(
            temp_dir.path(),
            &["init", "--initial-branch=main", repo_path.to_str().unwrap()],
        );
        run_git(&repo_path, &["config", "user.name", "Test User"]);
        run_git(&repo_path, &["config", "user.email", "test@example.com"]);
        run_git(
            &repo_path,
            &["remote", "add", "origin", remote_path.to_str().unwrap()],
        );

        std::fs::write(repo_path.join("README.md"), "hello\n").expect("seed file should write");
        run_git(&repo_path, &["add", "README.md"]);
        run_git(&repo_path, &["commit", "-m", "feat: seed repository"]);
        run_git(&repo_path, &["push", "--set-upstream", "origin", "main"]);
        run_git(&repo_path, &["checkout", "-b", "feature/test"]);

        let outcome = push_branch(&repo_path, false).expect("push should succeed");

        assert_eq!(
            outcome.toast_message,
            "Pushed feature/test to origin and set upstream."
        );
        assert_eq!(
            git_stdout(
                &repo_path,
                &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]
            )
            .as_deref(),
            Some("origin/feature/test")
        );
    }

    fn run_git(repo_path: &Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(repo_path)
            .output()
            .expect("git command should start");

        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

pub fn find_project_pull_requests(
    repo_path: &Path,
    filter_index: usize,
    query: Option<&str>,
) -> Result<Vec<ProjectPagePullRequest>, String> {
    let gh = find_gh_cli().ok_or_else(|| {
        "Could not load pull requests. GitHub CLI (`gh`) is not installed or not on the app PATH."
            .to_string()
    })?;

    let mut search_terms = Vec::new();
    match filter_index {
        1 => search_terms.push("review-requested:@me".to_string()),
        2 => search_terms.push("author:@me".to_string()),
        3 => search_terms.push("draft:true".to_string()),
        _ => {}
    }
    if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
        search_terms.push(query.to_string());
    }

    let mut command = Command::new(gh);
    command.args([
        "pr",
        "list",
        "--state",
        "open",
        "--limit",
        "100",
        "--json",
        "number,url,title,headRefName,isDraft,additions,deletions,state,mergedAt,author,reviewDecision",
    ]);
    if !search_terms.is_empty() {
        command.arg("--search");
        command.arg(search_terms.join(" "));
    }

    let output = command
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not load pull requests: {err}"))?;

    if !output.status.success() {
        let failure = command_failure("Could not load pull requests", &output);
        if indicates_missing_git_remote(&failure) {
            return Ok(Vec::new());
        }
        return Err(failure);
    }

    let records = serde_json::from_slice::<Vec<GitHubProjectPagePullRequestRecord>>(&output.stdout)
        .map_err(|err| format!("Could not parse pull requests: {err}"))?;

    Ok(records
        .into_iter()
        .map(|record| {
            let state = normalize_pull_request_state(&GitHubPullRequestRecord {
                number: record.number,
                url: record.url.clone(),
                state: record.state.clone(),
                merged_at: record.merged_at.clone(),
                updated_at: None,
            });
            let author = record
                .author
                .and_then(|author| author.login)
                .unwrap_or_else(|| "unknown".to_string());
            let review_decision = record.review_decision.unwrap_or_default();
            ProjectPagePullRequest {
                number: record.number,
                url: record.url,
                title: record.title,
                branch: record.head_ref_name.unwrap_or_default(),
                author: author.clone(),
                lines_added: record.additions.unwrap_or(0),
                lines_removed: record.deletions.unwrap_or(0),
                draft: record.is_draft.unwrap_or(false),
                review_required: review_decision.eq_ignore_ascii_case("REVIEW_REQUIRED"),
                review_requested_to_me: filter_index == 1,
                created_by_me: author.eq_ignore_ascii_case("fazulk"),
                state,
            }
        })
        .collect())
}
