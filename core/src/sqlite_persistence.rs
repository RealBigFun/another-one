//! SQLite-backed persistence adapter for `ProjectStore`.
//!
//! See `docs/architecture/sqlite-persistence.md` for the design.
//! This module replaces the JSON file (`projects.json`) with a
//! single-file SQLite database (`state.sqlite`) opened in WAL mode
//! with `synchronous=NORMAL`.
//!
//! The historical sequencing of this work was:
//!
//! - **A** — scaffolding (rusqlite dep, connection-open helper).
//! - **B** — `SqliteProjectStorePersistence` implementing
//!   `ProjectStorePersistence`, single-row whole-blob storage.
//! - **C** — `migrate_from_json` for first-launch import from
//!   the legacy `projects.json`.
//! - **D** — wired live: `ProjectStore::load()` runs the migration
//!   then opens the SQLite adapter; `ProjectStore::save()` is a thin
//!   delegate. The 50 ms debounced background writer (#129) is gone
//!   because WAL gives durability per commit.
//!
//! Whole-blob storage is still in effect: the entire `StoreFileV4`
//! is one JSON column on the singleton `app_state` row. The per-row
//! schema in the design doc (sections / tabs / projects as separate
//! tables) is the eventual destination but lands incrementally
//! against real workloads, not as a big-bang re-shape.
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

#![allow(dead_code)] // some helpers (default_state_db_path, schema_version) are
                     // surface-level and used only by tests / future code.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::project_store::{
    PersistedSectionState, ProjectStore, ProjectStorePersistence, StoreFileV4,
};

/// File name for the SQLite database. Lives next to the existing
/// `projects.json` in the app config dir (see `app_config_dir` in
/// `project_store.rs`). Same dir means migration is a single
/// directory read; same dir means the user's backup tools see them
/// together.
pub(crate) const STATE_DB_FILENAME: &str = "state.sqlite";

/// Bumped on schema changes.
///
/// - **v1**: single-row whole-blob storage. One `app_state` row,
///   one JSON column with the entire `StoreFileV4`.
/// - **v2**: introduces a `sections` table for row-level writes on
///   the hot path (`PersistedSectionState` mutations from PTY
///   storms). The `app_state` blob is unchanged; on load the
///   `sections` table is overlaid on top of `blob.terminal_sections`,
///   row-level wins. This means the blob carries a possibly-stale
///   copy of sections at all times — but the user-visible state
///   is what `load()` returns, which always merges the freshest
///   per-row data on top.
const SCHEMA_VERSION: i64 = 2;

/// DDL for v2.
///
/// `sections.section_id` is the same string the in-memory
/// `terminal_sections: HashMap<String, _>` is keyed by — either
/// a task-bound section id or a project-page section id. We don't
/// add a foreign key to a hypothetical `tasks(id)` here because
/// project-page sections aren't tied to a task at all.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS app_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    state_json TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sections (
    section_id TEXT PRIMARY KEY,
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

/// Initialise the schema and stamp the schema version. Idempotent
/// — safe to call on every open. The schema migration from v1 to
/// v2 is implicit: v2 adds the `sections` table, never modifies
/// pre-existing tables, so a v1 database stamps as v2 the first
/// time it's opened by this binary. Existing v1 state in
/// `app_state.state_json` survives untouched; the empty `sections`
/// table is overlaid by `load()` (which is a no-op when empty),
/// so the next live mutation populates the row-level cache and
/// from then on `load()` reads the freshest data row-by-row.
fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    // Stamp / re-stamp the version to the current. INSERT OR REPLACE
    // (not OR IGNORE) so a v1 database picks up the v2 stamp on
    // first open without needing a separate migration step.
    conn.execute(
        "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
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

impl std::fmt::Debug for SqliteProjectStorePersistence {
    /// Manual `Debug` because `rusqlite::Connection` doesn't impl
    /// `Debug` and the persistence trait now requires it (so we can
    /// derive `Debug` on `ProjectStore` without dropping it from the
    /// outer type's debug output). The connection's internal state
    /// isn't useful in logs anyway — the path is the identifier
    /// that matters.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteProjectStorePersistence")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
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

/// Outcome of a one-shot migration from `projects.json` to the
/// SQLite database. Used by tests and (eventually) by the live
/// boot path in commit D — the call site can branch on the
/// outcome to surface a one-time toast / log line.
#[derive(Debug)]
pub(crate) enum MigrationOutcome {
    /// SQLite already has an `app_state` row — either we ran a
    /// migration on a previous launch or the user is on a fresh
    /// install with no JSON to import. Either way: nothing to do.
    AlreadyMigrated,
    /// Imported `projects.json` into SQLite and renamed the JSON
    /// to `projects.json.bak.<unix-timestamp>`. The bak path is
    /// returned so the caller can log it.
    Migrated { backup_path: PathBuf },
    /// SQLite is empty *and* there's no `projects.json` to import.
    /// Fresh install on the SQLite binary. The caller should
    /// proceed with an empty store.
    NoLegacyState,
}

/// Migrate from a v4 `projects.json` into the SQLite-backed
/// `state.sqlite`, idempotently.
///
/// Behaviour:
///
/// 1. If `state.sqlite` already has an `app_state` row — do
///    nothing, return `AlreadyMigrated`. Migration ran on a previous
///    launch.
/// 2. Else if `projects.json` exists — read it via
///    [`ProjectStore::try_read_from_json_path`] (which preserves the
///    existing version-coercion / corrupt-file-backup behaviour),
///    write the resulting `StoreFileV4` blob into SQLite, then
///    rename the JSON to `projects.json.bak.<unix-timestamp>` so a
///    rollback is one rename away. Returns `Migrated { backup_path }`.
/// 3. Else — nothing to migrate. Returns `NoLegacyState`.
///
/// The migration is intentionally one-way: once the JSON is
/// renamed to `.bak.*`, subsequent launches see no
/// `projects.json` and skip the import (case 1 or 3). This means
/// the SQLite database is the durable copy of state from that
/// point forward; the `.bak.*` file is for human rollback only.
///
/// Errors are logged to stderr but do not panic — the caller can
/// still proceed with an empty store. This matches the JSON
/// adapter's existing "corrupt file? back it up and continue"
/// recovery posture.
pub(crate) fn migrate_from_json(
    json_path: &Path,
    db_path: &Path,
) -> rusqlite::Result<MigrationOutcome> {
    let conn = open_state_db(db_path)?;
    init_schema(&conn)?;

    // Case 1: SQLite already has state. Don't overwrite.
    let has_state: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_state WHERE id = 1)",
            [],
            |row| row.get::<_, i64>(0).map(|n| n != 0),
        )
        .unwrap_or(false);
    if has_state {
        return Ok(MigrationOutcome::AlreadyMigrated);
    }

    // Case 3: no JSON to import.
    let Some(store) = ProjectStore::try_read_from_json_path(json_path) else {
        return Ok(MigrationOutcome::NoLegacyState);
    };

    // Case 2: import.
    let json = match serde_json::to_string(&store) {
        Ok(s) => s,
        Err(err) => {
            eprintln!(
                "sqlite_persistence: failed to serialise legacy StoreFileV4 during migration: {err}"
            );
            return Ok(MigrationOutcome::NoLegacyState);
        }
    };
    conn.execute(
        "INSERT OR REPLACE INTO app_state (id, state_json) VALUES (1, ?1)",
        params![json],
    )?;

    let backup_path = legacy_backup_path(json_path);
    if let Err(err) = std::fs::rename(json_path, &backup_path) {
        // Rename failed (e.g. cross-fs move on a weird config dir).
        // The DB write already committed, so we're not data-lossy
        // here — just leaving the JSON in place. Next launch will
        // hit the AlreadyMigrated branch and skip re-importing,
        // which is correct.
        eprintln!(
            "sqlite_persistence: imported {json_path:?} into SQLite but failed to back up the JSON: {err}"
        );
        return Ok(MigrationOutcome::Migrated {
            backup_path: json_path.to_path_buf(),
        });
    }
    Ok(MigrationOutcome::Migrated { backup_path })
}

/// Compute a unique-per-run backup path for the legacy JSON. The
/// timestamp is unix-epoch seconds, which is enough granularity
/// because there's exactly one migration per binary launch and any
/// retry happens at least one second later.
fn legacy_backup_path(json_path: &Path) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut backup = json_path.to_path_buf();
    let file_name = json_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "projects.json".to_string());
    backup.set_file_name(format!("{file_name}.bak.{ts}"));
    backup
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

    fn read_sections(&self) -> Vec<(String, PersistedSectionState)> {
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let mut stmt = match conn.prepare("SELECT section_id, state_json FROM sections") {
            Ok(s) => s,
            Err(err) => {
                eprintln!("sqlite_persistence: prepare sections read failed: {err}");
                return Vec::new();
            }
        };
        let rows = match stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        {
            Ok(r) => r,
            Err(err) => {
                eprintln!("sqlite_persistence: query sections failed: {err}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows.flatten() {
            let (section_id, state_json) = row;
            match serde_json::from_str::<PersistedSectionState>(&state_json) {
                Ok(state) => out.push((section_id, state)),
                Err(err) => {
                    eprintln!(
                        "sqlite_persistence: failed to deserialise section {section_id}: {err}"
                    );
                }
            }
        }
        out
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
        // Whole-blob save. Sections are NOT cleared from the
        // `sections` row-level table here — they're an independent
        // durable surface, written/deleted by `upsert_section` /
        // `remove_section_rows`. `load()` overlays the rows on top
        // of the blob's task-embedded sections, so the blob carrying
        // possibly-stale section data is harmless: the overlay wins.
        if let Err(err) = conn.execute(
            "INSERT OR REPLACE INTO app_state (id, state_json) VALUES (1, ?1)",
            params![json],
        ) {
            eprintln!("sqlite_persistence: write app_state failed: {err}");
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn upsert_section(
        &self,
        section_id: &str,
        state: &PersistedSectionState,
        _full_blob: &StoreFileV4,
    ) {
        let json = match serde_json::to_string(state) {
            Ok(s) => s,
            Err(err) => {
                eprintln!(
                    "sqlite_persistence: failed to serialise section {section_id} for upsert: {err}"
                );
                return;
            }
        };
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(err) = conn.execute(
            "INSERT OR REPLACE INTO sections (section_id, state_json) VALUES (?1, ?2)",
            params![section_id, json],
        ) {
            eprintln!(
                "sqlite_persistence: failed to upsert section {section_id}: {err}"
            );
        }
    }

    fn remove_section_rows(&self, section_ids: &[String], _full_blob: &StoreFileV4) {
        if section_ids.is_empty() {
            return;
        }
        let conn = self.conn.lock().unwrap_or_else(|p| p.into_inner());
        let tx = match conn.unchecked_transaction() {
            Ok(t) => t,
            Err(err) => {
                eprintln!("sqlite_persistence: failed to begin remove_section_rows tx: {err}");
                return;
            }
        };
        for id in section_ids {
            if let Err(err) = tx.execute("DELETE FROM sections WHERE section_id = ?1", params![id])
            {
                eprintln!("sqlite_persistence: failed to delete section {id}: {err}");
            }
        }
        if let Err(err) = tx.commit() {
            eprintln!("sqlite_persistence: failed to commit remove_section_rows: {err}");
        }
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

    // ── Migration tests ──────────────────────────────────────────────────

    /// Fresh install on the SQLite binary: no `projects.json`, no
    /// `state.sqlite`. Migration is a no-op and reports
    /// `NoLegacyState`.
    #[test]
    fn migrate_with_no_legacy_json_returns_no_legacy_state() {
        let tmp = TempDir::new().unwrap();
        let json_path = tmp.path().join("projects.json");
        let db_path = tmp.path().join(STATE_DB_FILENAME);

        let outcome = migrate_from_json(&json_path, &db_path).unwrap();
        assert!(
            matches!(outcome, MigrationOutcome::NoLegacyState),
            "expected NoLegacyState, got {outcome:?}"
        );
        // JSON file shouldn't have been created.
        assert!(!json_path.exists());
    }

    /// Legacy `projects.json` exists, SQLite is empty: read JSON,
    /// write to SQLite, rename JSON to `.bak.<ts>`. Subsequent
    /// `load()` returns the imported state.
    #[test]
    fn migrate_imports_legacy_json_and_backs_it_up() {
        let tmp = TempDir::new().unwrap();
        let json_path = tmp.path().join("projects.json");
        let db_path = tmp.path().join(STATE_DB_FILENAME);

        // Build a representative legacy file. The shape doesn't
        // need to be exhaustive — we just want one non-default UI
        // field to assert on after the round-trip.
        let legacy = serde_json::json!({
            "version": 4,
            "repos": [],
            "projects": [],
            "tasks": [],
            "ui": {
                "theme_mode": "dark",
                "left_sidebar_open": false,
            },
        });
        std::fs::write(&json_path, serde_json::to_string(&legacy).unwrap()).unwrap();

        let outcome = migrate_from_json(&json_path, &db_path).unwrap();
        let backup_path = match outcome {
            MigrationOutcome::Migrated { backup_path } => backup_path,
            other => panic!("expected Migrated, got {other:?}"),
        };

        // JSON renamed to `.bak.<ts>`, original gone.
        assert!(!json_path.exists());
        assert!(backup_path.exists());
        assert!(
            backup_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.starts_with("projects.json.bak."))
                .unwrap_or(false),
            "unexpected backup filename: {backup_path:?}"
        );

        // SQLite should now hold the legacy state. Re-open and
        // load to make sure the write committed durably, then
        // assert on a UI field that survived the round-trip.
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();
        let loaded = adapter.load();
        // serde_json round-trip via the StoreFileV4 -> Value path so
        // we don't need to expose StoreFileV4's private fields here.
        let loaded_value = serde_json::to_value(&loaded).unwrap();
        assert_eq!(
            loaded_value
                .get("ui")
                .and_then(|ui| ui.get("theme_mode"))
                .and_then(|t| t.as_str()),
            Some("dark")
        );
        assert_eq!(
            loaded_value
                .get("ui")
                .and_then(|ui| ui.get("left_sidebar_open"))
                .and_then(|b| b.as_bool()),
            Some(false)
        );
    }

    /// Migration is idempotent: running it twice is the same as
    /// running it once. Second invocation hits the `AlreadyMigrated`
    /// branch, doesn't touch the JSON backup, and doesn't overwrite
    /// the SQLite state with a fresh one.
    #[test]
    fn migrate_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let json_path = tmp.path().join("projects.json");
        let db_path = tmp.path().join(STATE_DB_FILENAME);

        let legacy = serde_json::json!({
            "version": 4, "repos": [], "projects": [], "tasks": [],
            "ui": { "theme_mode": "dark", "left_sidebar_open": false },
        });
        std::fs::write(&json_path, serde_json::to_string(&legacy).unwrap()).unwrap();

        // First run: imports.
        let first = migrate_from_json(&json_path, &db_path).unwrap();
        assert!(matches!(first, MigrationOutcome::Migrated { .. }));

        // Second run with no projects.json present (it was renamed):
        // the AlreadyMigrated branch fires.
        let second = migrate_from_json(&json_path, &db_path).unwrap();
        assert!(matches!(second, MigrationOutcome::AlreadyMigrated));

        // Third run after recreating projects.json with different
        // content: the AlreadyMigrated branch still wins, the new
        // JSON is *not* imported (would clobber the user's current
        // state otherwise).
        std::fs::write(
            &json_path,
            r#"{"version":4,"repos":[],"projects":[],"tasks":[],"ui":{"theme_mode":"light"}}"#,
        )
        .unwrap();
        let third = migrate_from_json(&json_path, &db_path).unwrap();
        assert!(matches!(third, MigrationOutcome::AlreadyMigrated));

        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();
        let loaded = adapter.load();
        let theme = serde_json::to_value(&loaded)
            .ok()
            .and_then(|v| v.get("ui")?.get("theme_mode")?.as_str().map(str::to_string));
        assert_eq!(theme, Some("dark".to_string()));
    }

    // ── Row-level section writes (step E) ───────────────────────────────

    /// `upsert_section` writes the section row without touching the
    /// blob. Reading sections back via `read_sections` returns the
    /// upserted state.
    #[test]
    fn upsert_section_writes_row_without_touching_blob() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();

        // Take a baseline of the blob contents (empty StoreFileV4)
        // so we can compare bytes after an upsert and confirm the
        // blob was not rewritten by upsert_section.
        let blob_before = serde_json::to_string(&adapter.load()).unwrap();

        let state = PersistedSectionState {
            active_tab_id: "tab-1".to_string(),
            next_tab_id: 2,
            cwd: None,
            tabs: Vec::new(),
        };
        adapter.upsert_section("section-x", &state, &StoreFileV4::default());

        // Blob unchanged: upsert_section is row-only.
        let blob_after = serde_json::to_string(&adapter.load()).unwrap();
        assert_eq!(
            blob_before, blob_after,
            "upsert_section must not rewrite app_state.state_json"
        );

        // But read_sections returns the row.
        let rows = adapter.read_sections();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "section-x");
        assert_eq!(rows[0].1.active_tab_id, "tab-1");
        assert_eq!(rows[0].1.next_tab_id, 2);
    }

    /// `remove_section_rows` deletes only the listed sections from
    /// the row-level cache; the blob is untouched.
    #[test]
    fn remove_section_rows_deletes_only_listed_sections() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();

        let mk = |tab: &str| PersistedSectionState {
            active_tab_id: tab.to_string(),
            next_tab_id: 1,
            cwd: None,
            tabs: Vec::new(),
        };
        adapter.upsert_section("a", &mk("ta"), &StoreFileV4::default());
        adapter.upsert_section("b", &mk("tb"), &StoreFileV4::default());
        adapter.upsert_section("c", &mk("tc"), &StoreFileV4::default());

        adapter.remove_section_rows(&["b".to_string()], &StoreFileV4::default());

        let mut ids: Vec<String> = adapter.read_sections().into_iter().map(|(id, _)| id).collect();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "c".to_string()]);
    }

    /// Full `save()` writes the blob but does NOT touch the
    /// row-level cache: sections table is an independent durable
    /// surface, only mutated by `upsert_section` /
    /// `remove_section_rows`. This is the post-step-E correctness
    /// fix — prior behaviour cleared the cache on save, which made
    /// the row-level write redundant with the blob save that always
    /// followed (and undone the perf claim of step E).
    #[test]
    fn save_does_not_clobber_row_level_cache() {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        let adapter = SqliteProjectStorePersistence::open(db_path).unwrap();

        let cached = PersistedSectionState {
            active_tab_id: "cached".to_string(),
            next_tab_id: 1,
            cwd: None,
            tabs: Vec::new(),
        };
        adapter.upsert_section("section-x", &cached, &StoreFileV4::default());
        assert_eq!(adapter.read_sections().len(), 1);

        // Full save with an empty blob should leave the cache alone.
        adapter.save(&StoreFileV4::default());
        let rows = adapter.read_sections();
        assert_eq!(
            rows.len(),
            1,
            "save() must not clear the row-level sections cache"
        );
        assert_eq!(rows[0].1.active_tab_id, "cached");
    }

    /// Smoke test against the live `projects.json` from the user's
    /// config dir, copied into a tempdir so we never mutate live
    /// state. `#[ignore]` so it doesn't run on CI / normal `cargo
    /// test`. Run explicitly with:
    ///
    ///     cargo test -p another-one-core --lib smoke_migrate_real_projects_json -- --ignored --nocapture
    ///
    /// Asserts the migration outcome, the JSON → .bak rename, the
    /// SQLite file is non-empty, the blob round-trips with the
    /// expected order-of-magnitude size, the row-level sections
    /// table is empty on a fresh migration, and migrate is
    /// idempotent on a second invocation.
    #[test]
    #[ignore]
    fn smoke_migrate_real_projects_json() {
        let xdg = dirs::home_dir()
            .expect("home dir")
            .join(".config/another-one/projects.json");
        let real_json = if xdg.exists() {
            xdg
        } else {
            let mac = dirs::home_dir()
                .unwrap()
                .join("Library/Application Support/another-one/projects.json");
            assert!(mac.exists(), "no live projects.json found");
            mac
        };
        let real_size = std::fs::metadata(&real_json).map(|m| m.len()).unwrap_or(0);
        eprintln!("smoke: real projects.json size = {real_size} bytes");

        let tmp = TempDir::new().unwrap();
        let staged_json = tmp.path().join("projects.json");
        let db_path = tmp.path().join(STATE_DB_FILENAME);
        std::fs::copy(&real_json, &staged_json).expect("copy projects.json into staging dir");

        let outcome = migrate_from_json(&staged_json, &db_path)
            .expect("migrate_from_json must not error on a real projects.json");
        eprintln!("smoke: migration outcome = {outcome:?}");
        assert!(
            matches!(outcome, MigrationOutcome::Migrated { .. }),
            "expected Migrated, got {outcome:?}"
        );

        assert!(!staged_json.exists(), "projects.json should have been renamed");
        let bak = std::fs::read_dir(tmp.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .find(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.starts_with("projects.json.bak."))
                    .unwrap_or(false)
            });
        assert!(bak.is_some(), "no projects.json.bak.<ts> in {:?}", tmp.path());
        assert_eq!(
            bak.unwrap().metadata().map(|m| m.len()).unwrap_or(0),
            real_size,
            "backup size should match original"
        );

        let db_size = std::fs::metadata(&db_path).map(|m| m.len()).unwrap_or(0);
        eprintln!("smoke: state.sqlite size = {db_size} bytes (vs JSON {real_size})");
        assert!(db_size > 0, "state.sqlite should be non-empty");

        let adapter = SqliteProjectStorePersistence::open(db_path.clone()).unwrap();
        let blob = adapter.load();
        let blob_json = serde_json::to_string(&blob).expect("serialise loaded blob");
        eprintln!("smoke: loaded blob size = {} bytes", blob_json.len());
        assert!(
            blob_json.len() as u64 > real_size / 4,
            "loaded blob is suspiciously small ({} bytes vs {} on disk)",
            blob_json.len(),
            real_size
        );

        let sections = adapter.read_sections();
        eprintln!("smoke: read_sections after migration = {} rows", sections.len());
        assert!(
            sections.is_empty(),
            "fresh migration leaves the row-level sections table empty"
        );

        let again = migrate_from_json(&staged_json, &db_path).expect("second migrate");
        assert!(matches!(again, MigrationOutcome::AlreadyMigrated));

        eprintln!("smoke: ✓ migration round-trips a {real_size}-byte projects.json into SQLite");
    }
}
