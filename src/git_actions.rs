//! Background git actions for toolbar buttons.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde::Deserialize;

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
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    match action {
        ToolbarGitAction::Commit => commit_with_ai(repo_path, false),
        ToolbarGitAction::CommitAndPush => commit_with_ai(repo_path, true),
        ToolbarGitAction::UndoLastCommit => undo_last_commit(repo_path),
        ToolbarGitAction::Fetch => run_simple_git_command(repo_path, ToolbarGitAction::Fetch),
        ToolbarGitAction::Pull => run_simple_git_command(repo_path, ToolbarGitAction::Pull),
        ToolbarGitAction::Push { force } => push_branch(repo_path, force),
        ToolbarGitAction::CreatePr { draft, base_branch } => {
            create_pull_request(repo_path, draft, base_branch.as_deref())
        }
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
            warning: true,
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
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    let _staged_all_changes = ensure_staged_changes(repo_path)?;
    let diff_summary = staged_diff_summary(repo_path).map_err(ToolbarActionError::from_message)?;
    let diff_patch = staged_diff_patch(repo_path).map_err(ToolbarActionError::from_message)?;
    let prompt = build_commit_prompt(&diff_summary, &diff_patch);
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

    let mut cmd = Command::new(gh);
    cmd.args(create_pull_request_args(&head_branch, draft, base_branch))
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
) -> Vec<String> {
    let mut args = vec![
        "pr".to_string(),
        "create".to_string(),
        "--fill".to_string(),
        "--head".to_string(),
        head_branch.to_string(),
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

fn staged_diff_summary(repo_path: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(["diff", "--cached", "--stat", "--no-color"])
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Could not inspect staged changes: {err}"))?;

    if !output.status.success() {
        return Err(command_failure("Could not inspect staged changes", &output));
    }

    let summary = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if summary.is_empty() {
        return Err("Commit failed. There are no staged changes to commit.".to_string());
    }

    Ok(summary)
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

fn build_commit_prompt(diff_summary: &str, diff_patch: &str) -> String {
    format!(
        concat!(
            "Generate a git commit message for these staged changes.\n",
            "Return only the commit message text.\n",
            "Rules:\n",
            "- Prefer Conventional Commit style when it fits.\n",
            "- First line must be a concise subject in imperative mood.\n",
            "- Keep the subject under 72 characters.\n",
            "- Add a blank line plus a short body only if it materially helps.\n",
            "- No markdown fences, no commentary, no quotes.\n\n",
            "Staged summary:\n",
            "{diff_summary}\n\n",
            "Staged patch:\n",
            "{diff_patch}\n"
        ),
        diff_summary = diff_summary,
        diff_patch = diff_patch,
    )
}

fn generate_commit_message(
    repo_path: &Path,
    prompt: &str,
) -> Result<GeneratedCommitMessage, String> {
    let raw = match run_codex(prompt, repo_path) {
        Ok(raw) => raw,
        Err(codex_err) => match run_claude(prompt, repo_path) {
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

fn run_codex(prompt: &str, repo_path: &Path) -> Result<String, String> {
    let codex = find_codex_cli().ok_or_else(|| "Codex CLI was not found.".to_string())?;
    let output_path = temp_output_path("codex-commit");
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
        return Err("Codex returned an empty commit message.".to_string());
    }

    Ok(raw)
}

fn run_claude(prompt: &str, repo_path: &Path) -> Result<String, String> {
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
        return Err("Claude returned an empty commit message.".to_string());
    }

    Ok(raw)
}

fn parse_commit_message(raw: &str) -> Result<GeneratedCommitMessage, String> {
    let normalized = raw.trim().trim_matches('`').trim();
    let normalized = normalized
        .strip_prefix("```")
        .and_then(|text| text.strip_suffix("```"))
        .unwrap_or(normalized)
        .trim();

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

fn git_commit(repo_path: &Path, message: &GeneratedCommitMessage) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.arg("commit").arg("-m").arg(&message.subject);
    if let Some(body) = message.body.as_ref() {
        cmd.arg("-m").arg(body);
    }

    let output = cmd
        .current_dir(repo_path)
        .output()
        .map_err(|err| format!("Commit failed: {err}"))?;

    if !output.status.success() {
        return Err(command_failure("Commit failed", &output));
    }

    Ok(())
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
        create_pull_request_args, find_latest_pull_request_args, git_stdout,
        indicates_missing_pull_request, normalize_github_remote,
        normalize_pull_request_check_bucket, parse_commit_message,
        parse_pull_request_checks_output, push_branch, simple_toolbar_git_command,
        PullRequestCheckBucket, ToolbarGitAction,
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
        let args = create_pull_request_args("feature/test", false, Some("main"));

        assert_eq!(
            args,
            [
                "pr",
                "create",
                "--fill",
                "--head",
                "feature/test",
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
        let args = create_pull_request_args("feature/test", true, None);

        assert_eq!(
            args,
            [
                "pr",
                "create",
                "--fill",
                "--head",
                "feature/test",
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
