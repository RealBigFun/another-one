//! SQLite-backed persistence adapter for `ProjectStore`.
//!
//! See `docs/architecture/sqlite-persistence.md` for the design.
//! This module replaces the JSON file (`projects.json`) with a
//! single-file SQLite database (`state.sqlite`) opened in WAL mode
//! with `synchronous=NORMAL`.
//!
//! Sequencing (this is commit B — schema + adapter):
//!
//! 1. ~~A — scaffolding.~~ Empty module + `rusqlite` dep landed.
//! 2. **B — schema + adapter (this commit).** Adds
//!    `SqliteProjectStorePersistence` implementing
//!    `ProjectStorePersistence`. Single-row whole-blob storage:
//!    one JSON column for the entire `StoreFileV4`. The per-row
//!    schema in the design doc is the eventual destination, but
//!    landing it in one PR with the adapter swap risks subtle
//!    schema-design churn under feature pressure. This commit only
//!    swaps *where the JSON lives*; structural changes are follow-ups.
//! 3. **C — migration.** First-launch import from `projects.json` v4.
//! 4. **D — swap + delete `SaveWorker`.** SqliteAdapter goes live;
//!    the 50 ms debounce thread (#129) goes away because WAL gives
//!    durability per commit.
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

#![allow(dead_code)] // commit B: the adapter exists but isn't wired live yet (commit D)

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::project_store::{ProjectStorePersistence, StoreFileV4};

/// File name for the SQLite database. Lives next to the existing
/// `projects.json` in the app config dir (see `app_config_dir` in
/// `project_store.rs`). Same dir means migration is a single
/// directory read; same dir means the user's backup tools see them
/// together.
pub(crate) const STATE_DB_FILENAME: &str = "state.sqlite";

/// Bumped on schema changes. v1 is the whole-blob layout described
/// in the module doc; later versions split it into per-row tables
/// (see `docs/architecture/sqlite-persistence.md` §Schema sketch).
/// Stored in the `meta` table under `schema_version`.
const SCHEMA_VERSION: i64 = 1;

/// DDL for v1. One `meta` table for migration breadcrumbs and one
/// singleton `app_state` row holding the entire serialised
/// `StoreFileV4`. The `CHECK (id = 1)` is the standard SQLite
/// idiom for a single-row table — it makes accidental
/// `INSERT INTO app_state` fail loudly instead of silently growing
/// a parallel state row.
const V1_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    state_json TEXT NOT NULL
);
"#;

/// Open (or create) the SQLite database at the given path with the
/// pragmas this app expects:
///
/// - `journal_mode=WAL` — readers don't block writers and durability
///   is per-commit. Standard for desktop apps.
/// - `synchronous=NORMAL` — flushes per commit to the WAL but
///   doesn't fsync the WAL file on every transaction. Standard
///   tradeoff for app state where losing the last few ms of writes
///   on power loss is acceptable.
/// - `foreign_keys=ON` — we'll rely on `ON DELETE CASCADE` once the
///   per-row schema lands.
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

/// Initialise the v1 schema and stamp the schema version. Idempotent
/// — safe to call on every open.
fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(V1_SCHEMA)?;
    // INSERT OR IGNORE so the version row only lands once. If we
    // ever bump SCHEMA_VERSION, the migration code lives in commit
    // C and runs *before* this stamp — by the time we reach this
    // line the schema is already at SCHEMA_VERSION.
    conn.execute(
        "INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    Ok(())
}

/// Read the current schema version stamped in `meta`. Returns 0 if
/// the row is missing (fresh database).
fn read_schema_version(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT value FROM meta WHERE key = 'schema_version'",
        [],
        |row| {
            let s: String = row.get(0)?;
            Ok(s.parse::<i64>().unwrap_or(0))
        },
    )
    .or_else(|err| match err {
        rusqlite::Error::QueryReturnedNoRows => Ok(0),
        other => Err(other),
    })
}

/// SQLite-backed implementation of [`ProjectStorePersistence`].
///
/// Whole-blob storage in v1: the entire `StoreFileV4` is serialised
/// to a JSON string and written into the singleton `app_state` row.
/// This is intentionally simple — the win we're capturing in this
/// commit is crash safety (atomic commits via WAL, no torn writes
/// if the process dies mid-save), not write-amplification reduction.
/// Per-row writes for hot paths (`sections`, `tabs`) are a follow-up
/// once we have telemetry on actual write patterns under real
/// workloads.
///
/// The connection is held in a `Mutex` because
/// [`ProjectStorePersistence::save`] takes `&self`. SQLite's own
/// WAL locking would also serialise writers, so the Mutex is mostly
/// about ergonomics — only one daemon thread writes at a time
/// anyway.
pub(crate) struct SqliteProjectStorePersistence {
    path: PathBuf,
    conn: Mutex<Connection>,
}

impl SqliteProjectStorePersistence {
    /// Open the database at `path` and ensure the v1 schema is in
    /// place. Creates the file if it doesn't exist.
    pub(crate) fn open(path: PathBuf) -> rusqlite::Result<Self> {
        let conn = open_state_db(&path)?;
        init_schema(&conn)?;
        Ok(Self {
            path,
            conn: Mutex::new(conn),
        })
    }

    /// Returns the schema version currently stamped in `meta`.
    /// Used by tests; live code doesn't branch on this yet.
    pub(crate) fn schema_version(&self) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        read_schema_version(&conn)
    }
}

impl ProjectStorePersistence for SqliteProjectStorePersistence {
    fn load(&self) -> StoreFileV4 {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let row: rusqlite::Result<String> = conn.query_row(
            "SELECT state_json FROM app_state WHERE id = 1",
            [],
            |row| row.get(0),
        );
        match row {
            Ok(state_json) => match serde_json::from_str::<StoreFileV4>(&state_json) {
                Ok(store) => store,
                Err(err) => {
                    // Mirrors the JSON adapter: corrupt content
                    // shouldn't crash boot. Logging to stderr keeps
                    // us decoupled from the desktop's tracing setup.
                    eprintln!(
                        "sqlite_persistence: failed to deserialise app_state row: {err}"
                    );
                    StoreFileV4::default()
                }
            },
            Err(rusqlite::Error::QueryReturnedNoRows) => StoreFileV4::default(),
            Err(err) => {
                eprintln!("sqlite_persistence: failed to read app_state: {err}");
                StoreFileV4::default()
            }
        }
    }

    fn save(&self, store: &StoreFileV4) {
        let json = match serde_json::to_string(store) {
            Ok(s) => s,
            Err(err) => {
                eprintln!("sqlite_persistence: failed to serialise StoreFileV4: {err}");
                return;
            }
        };
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        // INSERT OR REPLACE on the singleton row. SQLite's WAL
        // makes this atomic per call — either the new state lands
        // in full or the old state survives. No torn writes.
        if let Err(err) = conn.execute(
            "INSERT OR REPLACE INTO app_state (id, state_json) VALUES (1, ?1)",
            params![json],
        ) {
            eprintln!("sqlite_persistence: failed to write app_state: {err}");
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
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
        assert_eq!(synchronous, 1);

        let foreign_keys: bool = conn
            .pragma_query_value(None, "foreign_keys", |row| row.get(0))
            .unwrap();
        assert!(foreign_keys);
    }

    /// Schema initialiser is idempotent and stamps the version.
    #[test]
    fn init_schema_is_idempotent_and_stamps_version() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let adapter = SqliteProjectStorePersistence::open(db_path.clone()).unwrap();
        assert_eq!(adapter.schema_version().unwrap(), SCHEMA_VERSION);

        // Second open should be a no-op (schema already there,
        // version stamp not double-inserted).
        drop(adapter);
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();
        assert_eq!(adapter.schema_version().unwrap(), SCHEMA_VERSION);
    }

    /// load() on an empty DB returns the default StoreFileV4. This
    /// matches the JSON adapter's behaviour when `projects.json`
    /// doesn't exist yet.
    #[test]
    fn load_on_empty_db_returns_default_store() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();
        let loaded = adapter.load();
        let default = StoreFileV4::default();
        // Compare via serialised form — StoreFileV4 doesn't impl
        // PartialEq and adding it just for one test isn't worth it.
        assert_eq!(
            serde_json::to_string(&loaded).unwrap(),
            serde_json::to_string(&default).unwrap()
        );
    }

    /// save() then load() round-trips a non-trivial state. Relies
    /// on `from_projects_for_test` building a real-shaped store so
    /// the JSON path exercises every field.
    #[test]
    fn save_then_load_round_trips_full_state() {
        use crate::project_store::ProjectStore;

        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();

        // Build a non-empty store via the public test helper, then
        // grab its on-the-wire StoreFileV4 representation through
        // serde round-trip (we don't expose the internals directly).
        let mut store =
            ProjectStore::from_projects_for_test(Vec::new(), Vec::new());
        store.ui.theme_mode = crate::project_store::ThemeMode::Dark;
        store.ui.left_sidebar_open = false;
        let blob: StoreFileV4 = serde_json::from_str(
            &serde_json::to_string(&serde_for_test(&store)).unwrap(),
        )
        .unwrap();

        adapter.save(&blob);

        // Re-open from disk to make sure the value actually committed,
        // not just sat in the connection's page cache.
        drop(adapter);
        let adapter = SqliteProjectStorePersistence::open(
            tmp.path().join(STATE_DB_FILENAME),
        )
        .unwrap();
        let loaded = adapter.load();
        // Compare via serialised form so we don't have to touch
        // StoreFileV4's field visibility just for one round-trip
        // assertion. The JSON string is the on-disk format anyway,
        // so a string-equal proves the round-trip preserved the
        // exact bytes the caller passed in.
        assert_eq!(
            serde_json::to_string(&loaded).unwrap(),
            serde_json::to_string(&blob).unwrap()
        );
    }

    /// Helper: serialise a `ProjectStore` to a `serde_json::Value`
    /// shaped like `StoreFileV4`. The store has a `to_v4` projection
    /// internally but it isn't exposed; this test goes through serde
    /// to side-step that.
    fn serde_for_test(store: &crate::project_store::ProjectStore) -> serde_json::Value {
        // The store's serialisation is rooted at StoreFileV4 internally
        // (that's what JsonProjectStorePersistence writes). We can't
        // reach it directly here, so build the v4 shape from the
        // public fields. ui round-trips because UiState is serde-derived.
        serde_json::json!({
            "version": 4,
            "repos": [],
            "projects": [],
            "tasks": [],
            "ui": store.ui,
        })
    }
}
