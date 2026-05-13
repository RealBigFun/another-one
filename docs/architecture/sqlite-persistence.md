# SQLite persistence for app state

> Replace the whole-blob `projects.json` writer with a row-oriented SQLite store, and lower the persistence seam from "save the entire `StoreFileV4`" to "apply one mutation".

#architecture · #state-sync · #persistence

Status: **shipped**. Companion to [daemon-owned-state-authority.md](daemon-owned-state-authority.md).
See [sqlite-persistence-audit.md](sqlite-persistence-audit.md) for the caller migration story.

## What shipped (vs. what this doc originally proposed)

The design below is the planning doc as written. The implementation took a more conservative path on the schema; this section is the authoritative summary of the as-shipped behaviour.

**Schema (as shipped, v2):**

```sql
CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE app_state (id INTEGER PRIMARY KEY CHECK (id = 1), state_json TEXT NOT NULL);
CREATE TABLE sections (section_id TEXT PRIMARY KEY, state_json TEXT NOT NULL);
```

Rather than the fully-normalised per-table schema sketched below (separate `repos` / `projects` / `tasks` / `tabs` / `ui_state` / `host_settings` tables), what shipped is:

- One **whole-blob** `app_state` row carrying the full serialised `StoreFileV4` — same JSON the legacy `projects.json` carried.
- One **row-level overlay** `sections` table for the hot-path section mutations (`set_section_state`, `update_task_tabs`, `remove_terminal_sections`).

Reads merge the two: `ProjectStore::load_from_persistence` calls `persistence.load()` for the blob and `persistence.read_sections()` for the overlay, with overlay winning on collision. A full `save()` writes the blob and clears the overlay (the blob already contains the latest sections via per-task records).

**Why the minimal schema:** the original sketch's win was "every mutation is one targeted row UPDATE". In practice the hot path that motivated the whole effort is *just* section mutations (PTY-storm-driven). Other mutations (theme, agents, host settings, project catalog) fire on user interaction — at most a handful per second. Whole-blob saves cost ~50 KB serialisation, which is fine at human-interaction rates but not at PTY-burst rates. Splitting only sections out captures the perf delta without the schema-design surface area the full normalisation would carry.

**The persistence trait surface (as shipped):**

```rust
trait ProjectStorePersistence: Send + Sync + Debug {
    fn load(&self) -> StoreFileV4;
    fn save(&self, store: &StoreFileV4);
    fn path(&self) -> &Path;

    // Row-level overlay; default impl returns empty / falls back to save():
    fn read_sections(&self) -> Vec<(String, PersistedSectionState)> { Vec::new() }
    fn upsert_section(&self, id: &str, state: &PersistedSectionState, full_blob: &StoreFileV4) {
        self.save(full_blob);
    }
    fn remove_section_rows(&self, ids: &[String], full_blob: &StoreFileV4) {
        self.save(full_blob);
    }
}
```

The default-implementation pattern means `NoopPersistence` (test) and `InMemoryProjectStorePersistence` (test) keep working unchanged. Only `SqliteProjectStorePersistence` overrides the row-level methods.

**What we deferred (intentionally):**

- Per-row writes for non-section mutations. Theme changes, agent settings, host settings, project catalog mutations still rewrite the whole blob. Cheap at user-interaction rates; revisit if telemetry surfaces a hot mutation we missed.
- The fully normalised schema (separate tables for projects/tasks/tabs/etc.). The trait + `Mutation` enum are the seam; later commits can add `upsert_project` / `upsert_task` / `delete_tabs` etc. without touching callers.
- A benchmark proving the perf claim under PTY-storm load. The unit tests prove the *mechanism* (row writes don't touch the blob), but a real CDP-burst benchmark is a follow-up.

**Sequencing as shipped:** 11 commits across two phases.

| Phase | Commits | Lines |
|---|---|---|
| PR1 (state authority refactor) | foundation, daemon_host, app, lockdown | 4 commits, ~750 lines |
| PR2 (SQLite swap) | scaffolding, schema, migration, swap+delete-SaveWorker, row-level, smoke test | 6 commits, ~1100 lines |

The original design doc proposed two PRs; what shipped landed both on a single branch (`sqlite`) as 10 standalone commits + 1 smoke test commit, each independently bisectable.

---

## Decision (original planning text)

Adopt SQLite (via `rusqlite` with the `bundled` feature) as the on-disk format for durable app state owned by the daemon. The current JSON store becomes a one-shot migration source and is retired after one release.

This is **not** motivated by ACID-across-clients. Issue #156 already routes every client (desktop, mobile, MCP) through the daemon's `Control::*` dispatch, so the daemon is the single writer. Concurrency is solved.

It **is** motivated by:

1. **Crash safety.** `projects.json` is rewritten in place by a background writer thread. A crash mid-write can truncate the file. SQLite WAL gives atomic durability without us hand-rolling temp-file + rename.
2. **Stop rewriting the whole world on every keystroke.** Today each `save()` calls `serde_json::to_string_pretty` over the entire `StoreFileV4`. Under PTY storms (CDP / browser-tools sub-agents) this stalled the GPUI render thread for >2 s; the fix in #129 was a 50 ms debounce + a single-slot mailbox writer thread. That machinery exists only to mask whole-blob serialization. Row-level `UPDATE`s make it unnecessary.
3. **A real mutation API.** The daemon-owned-state-authority doc has been waiting for a typed mutation enum. SQLite forces us to define one (we need to know which row(s) a mutation touches), so the two efforts collapse into one.

Secondary, deferred wins: queryable projections, disciplined migrations (`refinery` / `sqlx::migrate!`-style), and a friendlier substrate for future peer-to-peer sync.

## Non-goals

- Multi-process writers. The daemon stays the single writer; SQLite's locking is a backstop, not a feature we're using.
- `sqlx`, async persistence, or a connection pool. The authority is synchronous and serialized; one connection is correct.
- Replacing in-memory state. `ProjectStore`'s in-memory fields stay as the read model. SQLite is the durable projection of that state, not the source of truth at runtime.
- Cross-process sharing of the DB file at runtime. (If a future tool needs read-only access, it opens a separate read-only connection.)

## Architecture

### Today

```
caller mutates ProjectStore.projects directly
        │
        ▼
ProjectStore::save()
        │
        ▼
ProjectStorePersistence::save(&StoreFileV4)   ← whole-blob seam
        │
        ▼ JSON: serde_json::to_string_pretty(entire StoreFile) → background writer → fs::write
```

### Proposed

```
caller submits Mutation (typed enum)
        │
        ▼
StateAuthority::apply(mutation)
        │
        ├─ validate
        ├─ mutate in-memory state
        ├─ rebuild affected projections
        ├─ persist via AppStatePersistence::apply(&mutation, &tx)   ← per-mutation seam
        └─ broadcast state-changed tick
```

The persistence trait shape:

```rust
trait AppStatePersistence {
    fn load(&self) -> Result<CanonicalAppState, LoadError>;
    fn apply(&self, mutation: &Mutation) -> Result<(), SaveError>;
    // Snapshot fallback for the few mutations that don't decompose cleanly
    // (e.g. legacy migration import). Used sparingly.
    fn snapshot(&self, state: &CanonicalAppState) -> Result<(), SaveError>;
}
```

The JSON adapter implements `apply` as "rebuild full state, write JSON" — i.e. today's behavior. The SQLite adapter implements `apply` as one transaction with row-targeted writes.

This way the seam refactor lands first with no behavior change, and the SQLite swap is a follow-up PR that only touches the adapter.

## Schema sketch

Rough; the design doc will get a full version. Names match existing struct field names where possible.

```sql
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
-- meta('schema_version', '1'), meta('migrated_from_json_v4', '<timestamp>')

CREATE TABLE repos (
    id          TEXT PRIMARY KEY,
    record_json TEXT NOT NULL  -- RepoRecord; small, rarely mutated
);

CREATE TABLE projects (
    id            TEXT PRIMARY KEY,
    sort_index    INTEGER NOT NULL,
    repo_id       TEXT REFERENCES repos(id),
    project_json  TEXT NOT NULL  -- Project struct minus children
);
CREATE INDEX idx_projects_sort ON projects(sort_index);

CREATE TABLE tasks (
    id              TEXT PRIMARY KEY,
    root_project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sort_index      INTEGER NOT NULL,
    task_json       TEXT NOT NULL
);
CREATE INDEX idx_tasks_root ON tasks(root_project_id, sort_index);

CREATE TABLE tabs (
    id        TEXT PRIMARY KEY,
    task_id   TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    pinned    INTEGER NOT NULL DEFAULT 0,
    title     TEXT,
    tab_json  TEXT NOT NULL
);

CREATE TABLE sections (
    id          TEXT PRIMARY KEY,   -- composite or uuid; matches today's section keying
    owner_kind  TEXT NOT NULL,      -- 'task' | 'project' | 'global'
    owner_id    TEXT,
    state_json  TEXT NOT NULL       -- the high-frequency write target
);
CREATE INDEX idx_sections_owner ON sections(owner_kind, owner_id);

CREATE TABLE ui_state (
    id          INTEGER PRIMARY KEY CHECK (id = 1),  -- singleton
    state_json  TEXT NOT NULL
);

CREATE TABLE host_settings (
    key         TEXT PRIMARY KEY,   -- 'shortcuts' | 'agents' | 'open_in' | 'git_actions' | ...
    value_json  TEXT NOT NULL
);
```

Notes:
- JSON columns are deliberate: many of these structs (Project, Task, Tab) are deeply nested and not worth fully normalizing in v1. The win we care about is "write only the rows that changed", not "query inside a Task".
- `sections` is the hot path — that's the row that takes the brunt of CDP-burst writes. Keeping it isolated means `persist_section_state` becomes a single-row `UPDATE`.
- `meta.schema_version` replaces today's hand-rolled `STORE_VERSION = 4`.

## Mutation → SQL mapping

A mutation lists the rows it touches. Examples:

| Mutation | SQL |
|---|---|
| `SetSectionState { id, state }` | `UPDATE sections SET state_json=? WHERE id=?` |
| `AddProject { project }` | `INSERT INTO projects ...` + `INSERT INTO repos ...` if new |
| `RemoveTask { id }` | `DELETE FROM tasks WHERE id=?` (cascades tabs) |
| `SetThemePreference { theme }` | `UPDATE ui_state SET state_json=json_set(state_json, '$.theme', ?) WHERE id=1` |
| `RenameTask { id, name }` | `UPDATE tasks SET task_json=? WHERE id=?` |

One mutation = one transaction. WAL mode with `synchronous=NORMAL`.

## Migration

One-shot at first launch on the new binary:

1. Open `projects.json` (current v4 reader).
2. Open / create `state.sqlite` next to it.
3. If `meta.schema_version` is missing, populate every table from the loaded `StoreFileV4`.
4. Rename `projects.json` → `projects.json.bak.<timestamp>`.
5. From here on, JSON adapter is unused but compiles.

Rollback path: if a user reports corruption, they can rename `.bak` back and downgrade. We keep the JSON reader (not writer) for one release.

## Test strategy

- Unit tests use `rusqlite::Connection::open_in_memory()`. Faster than tempfiles, deterministic.
- Existing `InMemoryProjectStorePersistence` is replaced by an in-memory SQLite adapter; tests that asserted on JSON file contents are rewritten to assert on canonical state via the authority's projections (which is what they should have been doing anyway).
- One integration test pins `projects.json` v4 → migrate → SQLite → load → assert canonical state round-trips.
- Benchmark: a synthetic CDP burst (1000 `SetSectionState` mutations in <1 s) must not block the render thread. Today's JSON path is the baseline.

## What we delete

After both PRs land:

- `SaveWorker`, `SAVE_DEBOUNCE`, `SAVE_WORKER`, `save_worker_loop`, `save_worker` — all of `core/src/project_store.rs`'s `#[cfg(not(test))]` writer machinery (lines ~30–120).
- The `#[cfg(test)]` / `#[cfg(not(test))]` split inside `JsonProjectStorePersistence::save`.
- The `to_string_pretty` call site as a hot path.
- The `STORE_VERSION` / `LEGACY_STORE_VERSION` constants (subsumed by `meta.schema_version`).

## Sequencing

Two PRs, in order:

1. **Lower the persistence seam.** Introduce `Mutation` enum + `StateAuthority::apply` + per-mutation `AppStatePersistence::apply`. JSON adapter implements `apply` as "rebuild whole state, write JSON" — behavior unchanged. Removes `pub` fields on `ProjectStore` in favor of mutation-only writes. This PR is large but mechanical.
2. **SQLite adapter + migration.** New crate dep on `rusqlite`. New adapter. Migration code. Delete the SaveWorker.

Splitting this way means PR 1 is reviewable on its own merits (it's the long-deferred authority refactor), and PR 2 is a contained storage swap.

## Open questions

- Do we want `serde_rusqlite` or hand-rolled `FromRow` impls? Lean toward hand-rolled for the small number of tables.
- Where does `state.sqlite` live? Same dir as `projects.json` today (`~/Library/Application Support/another-one/` on macOS, XDG config on Linux). Mobile path TBD when mobile gets durable state.
- Backup strategy: SQLite's `.backup` API on graceful shutdown? Or rely on filesystem snapshots? Probably defer to a follow-up.
- Is there a use case for opening the DB read-only from a sibling tool (e.g. a future CLI inspector)? If so, `PRAGMA journal_mode=WAL` already supports concurrent readers; just document it.
