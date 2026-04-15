//! Background git actions for toolbar buttons.

use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarGitAction {
    Commit,
    CommitAndPush,
    Push { force: bool },
    CreatePr { draft: bool },
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

pub fn execute_toolbar_git_action(
    repo_path: &Path,
    action: ToolbarGitAction,
) -> Result<ToolbarActionOutcome, ToolbarActionError> {
    match action {
        ToolbarGitAction::Commit => commit_with_ai(repo_path, false),
        ToolbarGitAction::CommitAndPush => commit_with_ai(repo_path, true),
        ToolbarGitAction::Push { force } => push_branch(repo_path, force),
        ToolbarGitAction::CreatePr { draft } => create_pull_request(repo_path, draft),
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
    let output = Command::new("git")
        .arg("push")
        .args(force.then_some("--force-with-lease"))
        .current_dir(repo_path)
        .output()
        .map_err(|err| ToolbarActionError::from_message(format!("Push failed: {err}")))?;

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

fn create_pull_request(
    repo_path: &Path,
    draft: bool,
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
    cmd.args(["pr", "create", "--fill", "--head"])
        .arg(&head_branch)
        .current_dir(repo_path);
    if draft {
        cmd.arg("--draft");
    }

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
        match (draft, url.as_deref()) {
            (true, Some(url)) => format!("Created draft pull request: {url}"),
            (false, Some(url)) => format!("Created pull request: {url}"),
            (true, None) => "Created draft pull request.".to_string(),
            (false, None) => "Created pull request.".to_string(),
        },
        false,
        false,
    ))
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
    use super::{normalize_github_remote, parse_commit_message};

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
}
