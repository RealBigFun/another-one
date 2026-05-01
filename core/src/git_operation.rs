//! Cross-cutting coordination for user-initiated git mutations.
//!
//! Commit, push, branch creation, and worktree creation all shell out
//! to `git` and can touch shared repository state such as the index,
//! refs, hooks, signing agents, and credentials. The UI starts those
//! operations on background threads, but the background workers still
//! need a process-wide ordering point so two user actions cannot race
//! each other and leave confusing repository state behind.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};

const GLOBAL_GIT_OPERATION_LOCK_KEY: &str = "global";

#[derive(Default)]
struct GitOperationLock {
    mutex: Mutex<()>,
}

impl GitOperationLock {
    fn run<R>(&self, work: impl FnOnce() -> R) -> R {
        // If a worker panics while holding the lock, keep the app
        // usable: the protected value is unit and carries no state to
        // corrupt, so taking the poisoned guard is safe and preserves
        // serialization for subsequent operations.
        let _guard = match self.mutex.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        work()
    }
}

#[derive(Default)]
struct GitOperationLocks {
    locks: Mutex<HashMap<String, Arc<GitOperationLock>>>,
}

impl GitOperationLocks {
    fn run_for_key<R>(&self, key: impl Into<String>, work: impl FnOnce() -> R) -> R {
        let lock = {
            let mut locks = match self.locks.lock() {
                Ok(locks) => locks,
                Err(poisoned) => poisoned.into_inner(),
            };
            Arc::clone(
                locks
                    .entry(key.into())
                    .or_insert_with(|| Arc::new(GitOperationLock::default())),
            )
        };

        lock.run(work)
    }
}

#[allow(dead_code)]
static GIT_OPERATION_LOCK: OnceLock<GitOperationLock> = OnceLock::new();
static GIT_OPERATION_LOCKS: OnceLock<GitOperationLocks> = OnceLock::new();

/// Run a user-initiated git operation behind a process-wide lock.
///
/// Use this for callers that do not have a clear repository path. Prefer
/// [`run_serialized_git_operation_for_path`] when a project/worktree path is
/// available so unrelated repositories can proceed concurrently.
#[allow(dead_code)]
pub fn run_serialized_git_operation<R>(work: impl FnOnce() -> R) -> R {
    GIT_OPERATION_LOCK.get_or_init(Default::default).run(work)
}

/// Run a user-initiated git operation behind a repository-keyed lock.
///
/// The lock key is the repository's resolved `git common-dir`, so worktrees that
/// share repository state remain serialized while unrelated repositories can run
/// at the same time. If common-dir resolution fails, the lock falls back to the
/// canonical project path and finally to a global fallback key, keeping behavior
/// conservative and safe for invalid or disappearing paths.
pub fn run_serialized_git_operation_for_path<R>(
    repo_path: impl AsRef<Path>,
    work: impl FnOnce() -> R,
) -> R {
    let key = git_operation_lock_key_for_path(repo_path.as_ref());
    GIT_OPERATION_LOCKS
        .get_or_init(Default::default)
        .run_for_key(key, work)
}

fn git_operation_lock_key_for_path(repo_path: &Path) -> String {
    resolve_git_common_dir(repo_path)
        .map(|common_dir| format!("common-dir:{}", stable_path_key(&common_dir)))
        .or_else(|| {
            canonicalize_if_possible(repo_path)
                .map(|repo_path| format!("repo-path:{}", stable_path_key(&repo_path)))
        })
        .unwrap_or_else(|| GLOBAL_GIT_OPERATION_LOCK_KEY.to_string())
}

fn resolve_git_common_dir(repo_path: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["rev-parse", "--git-common-dir"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let common_dir = String::from_utf8(output.stdout).ok()?;
    let common_dir = common_dir.trim();
    if common_dir.is_empty() {
        return None;
    }

    let common_dir = PathBuf::from(common_dir);
    let common_dir = if common_dir.is_absolute() {
        common_dir
    } else {
        repo_path.join(common_dir)
    };
    Some(canonicalize_if_possible(&common_dir).unwrap_or(common_dir))
}

fn canonicalize_if_possible(path: &Path) -> Option<PathBuf> {
    path.canonicalize().ok()
}

fn stable_path_key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::{
        git_operation_lock_key_for_path, run_serialized_git_operation_for_path, GitOperationLock,
        GitOperationLocks, GLOBAL_GIT_OPERATION_LOCK_KEY,
    };
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn run_should_serialize_concurrent_callers() {
        let lock = Arc::new(GitOperationLock::default());
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        let first_lock = Arc::clone(&lock);
        let first = std::thread::spawn(move || {
            first_lock.run(|| {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
            });
        });
        first_entered_rx.recv().unwrap();

        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second_lock = Arc::clone(&lock);
        let second = std::thread::spawn(move || {
            second_lock.run(|| {
                second_entered_tx.send(()).unwrap();
            });
        });

        assert!(second_entered_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release_first_tx.send(()).unwrap();
        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        first.join().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn run_for_key_should_serialize_same_key() {
        let locks = Arc::new(GitOperationLocks::default());
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        let first_locks = Arc::clone(&locks);
        let first = std::thread::spawn(move || {
            first_locks.run_for_key("repo-a", || {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
            });
        });
        first_entered_rx.recv().unwrap();

        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second_locks = Arc::clone(&locks);
        let second = std::thread::spawn(move || {
            second_locks.run_for_key("repo-a", || {
                second_entered_tx.send(()).unwrap();
            });
        });

        assert!(second_entered_rx
            .recv_timeout(Duration::from_millis(50))
            .is_err());
        release_first_tx.send(()).unwrap();
        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();

        first.join().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn run_for_key_should_allow_different_keys_to_run_concurrently() {
        let locks = Arc::new(GitOperationLocks::default());
        let (first_entered_tx, first_entered_rx) = mpsc::channel();
        let (release_first_tx, release_first_rx) = mpsc::channel();

        let first_locks = Arc::clone(&locks);
        let first = std::thread::spawn(move || {
            first_locks.run_for_key("repo-a", || {
                first_entered_tx.send(()).unwrap();
                release_first_rx.recv().unwrap();
            });
        });
        first_entered_rx.recv().unwrap();

        let (second_entered_tx, second_entered_rx) = mpsc::channel();
        let second_locks = Arc::clone(&locks);
        let second = std::thread::spawn(move || {
            second_locks.run_for_key("repo-b", || {
                second_entered_tx.send(()).unwrap();
            });
        });

        second_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap();
        release_first_tx.send(()).unwrap();

        first.join().unwrap();
        second.join().unwrap();
    }

    #[test]
    fn run_for_key_should_recover_from_poisoned_lock() {
        let locks = GitOperationLocks::default();

        let result = catch_unwind(AssertUnwindSafe(|| {
            locks.run_for_key("repo-a", || panic!("poison this lock"));
        }));
        assert!(result.is_err());

        let recovered = locks.run_for_key("repo-a", || 42);
        assert_eq!(recovered, 42);
    }

    #[test]
    fn lock_key_should_fall_back_to_global_for_missing_paths() {
        let missing_path = std::path::Path::new("/definitely/missing/another-one/git/repo");
        let key = git_operation_lock_key_for_path(missing_path);

        assert_eq!(key, GLOBAL_GIT_OPERATION_LOCK_KEY);
        assert_eq!(run_serialized_git_operation_for_path(missing_path, || 7), 7);
    }
}
