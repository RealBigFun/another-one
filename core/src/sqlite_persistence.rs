//! SQLite-backed persistence adapter for `ProjectStore`.
//!
//! See `docs/architecture/sqlite-persistence.md` for the design.
//! This module replaces the JSON file (`projects.json`) with a
//! single-file SQLite database (`state.sqlite`) opened in WAL mode
//! with `synchronous=NORMAL`.
//!
//! Sequencing (this is commit A — scaffolding only):
//!
//! 1. **A — scaffolding.** Empty module + `rusqlite` dep landed.
//!    Nothing is wired through the live persistence path; `ProjectStore`
//!    still saves to `projects.json` via `JsonProjectStorePersistence`.
//! 2. **B — schema + adapter.** Add `SqliteProjectStorePersistence`
//!    implementing the existing `ProjectStorePersistence` trait. It
//!    serialises the whole `StoreFileV4` into one JSON column for v1
//!    — the per-mutation row-level writes come in a later pass once
//!    the schema is exercised against real workloads.
//! 3. **C — migration.** On first launch with the new binary, read
//!    the existing `projects.json` v4 once, populate SQLite, rename
//!    the JSON to `projects.json.bak.<timestamp>`. JSON reader stays
//!    around for one release as a rollback path.
//! 4. **D — swap + delete `SaveWorker`.** SqliteAdapter becomes the
//!    primary persistence. WAL gives durability for free, so the
//!    50 ms debounce + background writer thread (#129 fix) goes
//!    away.
//!
//! Why bundled SQLite (`rusqlite/bundled`): greenfield desktop app
//! shipped on mac and linux. We don't want the runtime persistence
//! layer to depend on whichever `libsqlite3` happens to be installed
//! on the user's system, especially on linux distros where it might
//! be older than what we tested against. The ~1 MB binary-size hit
//! is acceptable.
//!
//! Why not `sqlx`: this layer is synchronous and serialised by the
//! state-authority lock. Async + a connection pool would be friction
//! for zero gain.

#![allow(dead_code)] // commit A: nothing wired through live paths yet

use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// File name for the SQLite database. Lives next to the existing
/// `projects.json` in the app config dir (see `app_config_dir` in
/// `project_store.rs`). Same dir means migration is a single
/// directory read; same dir means the user's backup tools see them
/// together.
pub(crate) const STATE_DB_FILENAME: &str = "state.sqlite";

/// Open (or create) the SQLite database at the given path with the
/// pragmas this app expects:
///
/// - `journal_mode=WAL` — readers don't block writers and durability
///   is per-commit. Standard for desktop apps.
/// - `synchronous=NORMAL` — flushes per commit to the WAL but
///   doesn't fsync the WAL file on every transaction. Standard
///   tradeoff for app state where losing the last few ms of writes
///   on power loss is acceptable.
/// - `foreign_keys=ON` — we'll rely on `ON DELETE CASCADE` for
///   `tasks → tabs` and `projects → tasks`.
///
/// Returns the connection ready for schema setup.
pub(crate) fn open_state_db(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Resolve the on-disk location of the SQLite state database.
/// Mirrors `ProjectStore::default_path()` so a future swap is just
/// a constant change.
pub(crate) fn default_state_db_path() -> PathBuf {
    crate::project_store::app_config_dir().join(STATE_DB_FILENAME)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Sanity check: bundled SQLite is linked, WAL pragmas apply,
    /// and we can round-trip a value through one connection. Catches
    /// `libsqlite3-sys` build / linkage regressions before they
    /// turn into mystery panics on first launch.
    #[test]
    fn open_state_db_applies_wal_pragmas() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let conn = open_state_db(&db_path).unwrap();

        let journal_mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal");

        let synchronous: i64 = conn
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        // 1 == NORMAL (0=OFF, 1=NORMAL, 2=FULL, 3=EXTRA).
        assert_eq!(synchronous, 1);

        let foreign_keys: bool = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert!(foreign_keys);

        // Round-trip a tiny piece of data so we know writes commit.
        conn.execute(
            "CREATE TABLE smoke (k TEXT PRIMARY KEY, v TEXT NOT NULL)",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO smoke VALUES ('hello', 'world')", [])
            .unwrap();
        let v: String = conn
            .query_row("SELECT v FROM smoke WHERE k = 'hello'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, "world");
    }
}
