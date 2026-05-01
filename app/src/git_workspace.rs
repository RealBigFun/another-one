//! User-facing Git workspace coordination state for project Git UI flows.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub(crate) struct GitWorkspace {
    last_status_refresh: Instant,
    last_metadata_refresh: Instant,
}

impl GitWorkspace {
    pub(crate) fn new_stale(
        now: Instant,
        status_interval: Duration,
        metadata_interval: Duration,
    ) -> Self {
        Self {
            last_status_refresh: now - status_interval,
            last_metadata_refresh: now - metadata_interval,
        }
    }

    pub(crate) fn mark_stale(&mut self, status_interval: Duration, metadata_interval: Duration) {
        let now = Instant::now();
        self.last_status_refresh = now - status_interval;
        self.last_metadata_refresh = now - metadata_interval;
    }

    pub(crate) fn mark_refreshed(&mut self, include_metadata: bool) {
        let now = Instant::now();
        self.last_status_refresh = now;
        if include_metadata {
            self.last_metadata_refresh = now;
        }
    }

    pub(crate) fn status_due(&self, interval: Duration) -> bool {
        self.last_status_refresh.elapsed() >= interval
    }

    pub(crate) fn metadata_due(&self, interval: Duration) -> bool {
        self.last_metadata_refresh.elapsed() >= interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_workspace_reports_status_and_metadata_due() {
        let status = Duration::from_secs(4);
        let metadata = Duration::from_secs(30);
        let workspace = GitWorkspace::new_stale(Instant::now(), status, metadata);

        assert!(workspace.status_due(status));
        assert!(workspace.metadata_due(metadata));
    }

    #[test]
    fn status_only_refresh_keeps_metadata_due() {
        let status = Duration::from_secs(4);
        let metadata = Duration::from_secs(30);
        let mut workspace = GitWorkspace::new_stale(Instant::now(), status, metadata);
        workspace.mark_refreshed(false);

        assert!(!workspace.status_due(status));
        assert!(workspace.metadata_due(metadata));
    }
}
