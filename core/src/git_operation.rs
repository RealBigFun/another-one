//! Cross-cutting coordination for user-initiated git mutations.
//!
//! Commit, push, branch creation, and worktree creation all shell out
//! to `git` and can touch shared repository state such as the index,
//! refs, hooks, signing agents, and credentials. The UI starts those
//! operations on background threads, but the background workers still
//! need a process-wide ordering point so two user actions cannot race
//! each other and leave confusing repository state behind.

use std::sync::{Mutex, OnceLock};

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

static GIT_OPERATION_LOCK: OnceLock<GitOperationLock> = OnceLock::new();

/// Run a user-initiated git operation behind a process-wide lock.
///
/// This intentionally serializes across repositories/worktrees for now.
/// A per-repository lock would improve concurrency, but correctly
/// resolving git common-dir/index/ref sharing across worktrees is easy
/// to get wrong; the conservative global lock keeps macOS/Linux
/// behavior predictable while slow operations still run off the UI
/// thread.
pub fn run_serialized_git_operation<R>(work: impl FnOnce() -> R) -> R {
    GIT_OPERATION_LOCK.get_or_init(Default::default).run(work)
}

#[cfg(test)]
mod tests {
    use super::GitOperationLock;
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
}
