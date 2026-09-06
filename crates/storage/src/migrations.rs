use rusqlite::Connection;
use rusqlite::TransactionBehavior;

use crate::StorageError;

const LATEST_SCHEMA_VERSION: i64 = 11;

const MIGRATION_1: &str = r#"
CREATE TABLE projects (
    project_id      TEXT PRIMARY KEY NOT NULL,
    root            TEXT NOT NULL UNIQUE CHECK (length(trim(root)) > 0),
    name            TEXT NOT NULL CHECK (length(trim(name)) > 0)
) STRICT;

CREATE TABLE tasks (
    task_id         TEXT PRIMARY KEY NOT NULL,
    project_id      TEXT NOT NULL,
    title           TEXT NOT NULL CHECK (length(trim(title)) > 0),
    created_at_ms   INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms   INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    last_sequence   INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence >= 0),
    FOREIGN KEY (project_id) REFERENCES projects(project_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE events (
    event_id        TEXT PRIMARY KEY NOT NULL,
    task_id         TEXT NOT NULL,
    turn_id         TEXT,
    sequence        INTEGER NOT NULL CHECK (sequence > 0),
    event_type      TEXT NOT NULL CHECK (length(trim(event_type)) > 0),
    payload_json    TEXT NOT NULL CHECK (json_valid(payload_json)),
    created_at_ms   INTEGER NOT NULL CHECK (created_at_ms >= 0),
    FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE RESTRICT,
    UNIQUE (task_id, sequence)
) STRICT;

CREATE INDEX events_task_sequence_idx ON events(task_id, sequence);
CREATE INDEX tasks_project_idx ON tasks(project_id, updated_at_ms DESC);
CREATE INDEX tasks_updated_idx ON tasks(updated_at_ms DESC, created_at_ms DESC);
"#;

const MIGRATION_2: &str = r#"
CREATE TABLE model_profiles (
    profile_id          TEXT PRIMARY KEY NOT NULL,
    name                TEXT NOT NULL CHECK (length(trim(name)) > 0),
    base_url            TEXT NOT NULL CHECK (length(trim(base_url)) > 0),
    model               TEXT NOT NULL CHECK (length(trim(model)) > 0),
    dialect             TEXT NOT NULL CHECK (dialect IN ('standard', 'deep_seek', 'qwen')),
    credential_ref      TEXT NOT NULL CHECK (length(trim(credential_ref)) > 0),
    max_output_tokens   INTEGER CHECK (max_output_tokens IS NULL OR max_output_tokens > 0),
    timeout_ms          INTEGER NOT NULL CHECK (timeout_ms BETWEEN 1000 AND 600000),
    is_default          INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at_ms       INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms       INTEGER NOT NULL CHECK (updated_at_ms >= 0)
) STRICT;

CREATE UNIQUE INDEX model_profiles_one_default_idx
ON model_profiles(is_default) WHERE is_default = 1;
CREATE INDEX model_profiles_updated_idx ON model_profiles(updated_at_ms DESC);
"#;

const MIGRATION_3: &str = r#"
ALTER TABLE model_profiles
ADD COLUMN context_window_tokens INTEGER
CHECK (
    context_window_tokens IS NULL
    OR context_window_tokens BETWEEN 4096 AND 1048576
);
"#;

// Version 3 of the desktop UI silently stored 4096 for every provider and did
// not expose an output-limit control. Those values were application defaults,
// not an informed user choice. Upgrade DeepSeek to its current 1M context and
// 384K output capability. Qwen model limits vary, so only raise its hidden
// output default conservatively and leave its configured context untouched.
const MIGRATION_4: &str = r#"
UPDATE model_profiles
SET max_output_tokens = 384000,
    context_window_tokens = 1000000
WHERE max_output_tokens = 4096
  AND dialect = 'deep_seek'
  AND (context_window_tokens IS NULL OR context_window_tokens = 32768);

UPDATE model_profiles
SET max_output_tokens = 8192
WHERE max_output_tokens = 4096
  AND dialect = 'qwen'
  AND (context_window_tokens IS NULL OR context_window_tokens >= 16384);
"#;

// An intermediate local build briefly migrated the same hidden DeepSeek
// default to 8192. Advance that already-versioned state without touching a
// user-selected value paired with any other context size.
const MIGRATION_5: &str = r#"
UPDATE model_profiles
SET max_output_tokens = 384000,
    context_window_tokens = 1000000
WHERE max_output_tokens = 8192
  AND dialect = 'deep_seek'
  AND context_window_tokens = 32768;
"#;

// The agent journal is intentionally separate from the legacy task event
// stream. It is a transport-neutral, thread-scoped fact log that can evolve
// without changing the desktop repository API. Snapshots are replaceable
// projections; journal events themselves are protected against mutation.
const MIGRATION_6: &str = r#"
CREATE TABLE journal_streams (
    thread_id       TEXT PRIMARY KEY NOT NULL CHECK (length(trim(thread_id)) > 0),
    last_sequence   INTEGER NOT NULL DEFAULT 0 CHECK (last_sequence >= 0)
) STRICT;

CREATE TABLE journal_events (
    event_id        TEXT PRIMARY KEY NOT NULL CHECK (length(trim(event_id)) > 0),
    thread_id       TEXT NOT NULL,
    turn_id         TEXT,
    step_id         TEXT,
    sequence        INTEGER NOT NULL CHECK (sequence > 0),
    schema_version  INTEGER NOT NULL CHECK (schema_version > 0),
    event_type      TEXT NOT NULL CHECK (length(trim(event_type)) > 0),
    payload_json    TEXT NOT NULL CHECK (json_valid(payload_json)),
    created_at_ms   INTEGER NOT NULL CHECK (created_at_ms >= 0),
    FOREIGN KEY (thread_id) REFERENCES journal_streams(thread_id) ON DELETE RESTRICT,
    UNIQUE (thread_id, sequence)
) STRICT;

CREATE INDEX journal_events_thread_sequence_idx
ON journal_events(thread_id, sequence);
CREATE INDEX journal_events_turn_sequence_idx
ON journal_events(thread_id, turn_id, sequence)
WHERE turn_id IS NOT NULL;
CREATE INDEX journal_events_step_sequence_idx
ON journal_events(thread_id, step_id, sequence)
WHERE step_id IS NOT NULL;

CREATE TRIGGER journal_events_reject_update
BEFORE UPDATE ON journal_events
BEGIN
    SELECT RAISE(ABORT, 'journal events are immutable');
END;

CREATE TRIGGER journal_events_reject_delete
BEFORE DELETE ON journal_events
BEGIN
    SELECT RAISE(ABORT, 'journal events are immutable');
END;

CREATE TABLE thread_snapshots (
    thread_id          TEXT PRIMARY KEY NOT NULL,
    through_sequence   INTEGER NOT NULL CHECK (through_sequence >= 0),
    schema_version     INTEGER NOT NULL CHECK (schema_version > 0),
    payload_json       TEXT NOT NULL CHECK (json_valid(payload_json)),
    created_at_ms      INTEGER NOT NULL CHECK (created_at_ms >= 0),
    FOREIGN KEY (thread_id) REFERENCES journal_streams(thread_id) ON DELETE RESTRICT
) STRICT;
"#;

// Mutable action claims are a rebuildable guard projection over the immutable
// journal. The unique idempotency key is acquired in the same transaction as
// the intent (and, when required, approval declaration) facts. An `executing`
// row is deliberately never made runnable again on open: after an unclean
// shutdown the caller must reconcile the external effect instead of repeating
// it speculatively.
const MIGRATION_7: &str = r#"
CREATE TABLE action_claims (
    idempotency_key     TEXT PRIMARY KEY NOT NULL CHECK (length(trim(idempotency_key)) > 0),
    action_id           TEXT NOT NULL UNIQUE CHECK (length(trim(action_id)) > 0),
    thread_id           TEXT NOT NULL,
    turn_id             TEXT,
    step_id             TEXT,
    action_kind         TEXT NOT NULL CHECK (length(trim(action_kind)) > 0),
    schema_version      INTEGER NOT NULL CHECK (schema_version > 0),
    status              TEXT NOT NULL CHECK (
        status IN ('pending_approval', 'ready', 'executing', 'completed', 'rejected', 'failed')
    ),
    intent_event_id     TEXT NOT NULL UNIQUE,
    approval_event_id   TEXT UNIQUE,
    last_event_id       TEXT NOT NULL,
    created_at_ms       INTEGER NOT NULL CHECK (created_at_ms >= 0),
    updated_at_ms       INTEGER NOT NULL CHECK (updated_at_ms >= 0),
    FOREIGN KEY (thread_id) REFERENCES journal_streams(thread_id) ON DELETE RESTRICT,
    FOREIGN KEY (intent_event_id) REFERENCES journal_events(event_id) ON DELETE RESTRICT,
    FOREIGN KEY (approval_event_id) REFERENCES journal_events(event_id) ON DELETE RESTRICT,
    FOREIGN KEY (last_event_id) REFERENCES journal_events(event_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX action_claims_thread_status_idx
ON action_claims(thread_id, status, created_at_ms);
"#;

// The settings form previously treated a model's maximum generation capability
// as the default reserve and silently clamped it to context - 1024. Reducing the
// context field could therefore persist a profile with only 1024 input tokens,
// which is too small for the agent's own fixed context. Repair only recognizable
// application defaults or the exact invalid near-full-output shape.
const MIGRATION_8: &str = r#"
UPDATE model_profiles
SET context_window_tokens = 1048576,
    max_output_tokens = 32768
WHERE dialect = 'deep_seek'
  AND (
    (context_window_tokens = 1000000 AND max_output_tokens = 384000)
    OR (
      context_window_tokens IS NOT NULL
      AND max_output_tokens IS NOT NULL
      AND context_window_tokens - max_output_tokens < 8192
    )
  );

UPDATE model_profiles
SET context_window_tokens = 1000000,
    max_output_tokens = 32768
WHERE dialect = 'qwen'
  AND (
    (context_window_tokens = 131072 AND max_output_tokens = 8192)
    OR (
      context_window_tokens IS NOT NULL
      AND max_output_tokens IS NOT NULL
      AND context_window_tokens - max_output_tokens < 8192
    )
  );

UPDATE model_profiles
SET context_window_tokens = CASE
        WHEN context_window_tokens < 16384 THEN 32768
        ELSE context_window_tokens
    END,
    max_output_tokens = 8192
WHERE dialect = 'standard'
  AND context_window_tokens IS NOT NULL
  AND max_output_tokens IS NOT NULL
  AND context_window_tokens - max_output_tokens < 8192;
"#;

// Simple owns only the product-level association. Codex rollout remains the
// source of truth for thread, turn, item, approval and tool lifecycle state.
// A binding is immutable so reopening a task cannot silently switch its model
// profile or attach it to another Codex history.
const MIGRATION_9: &str = r#"
CREATE TABLE codex_thread_bindings (
    task_id             TEXT PRIMARY KEY NOT NULL,
    codex_thread_id     TEXT NOT NULL UNIQUE CHECK (length(trim(codex_thread_id)) > 0),
    model_profile_id    TEXT NOT NULL,
    created_at_ms       INTEGER NOT NULL CHECK (created_at_ms >= 0),
    FOREIGN KEY (task_id) REFERENCES tasks(task_id) ON DELETE RESTRICT,
    FOREIGN KEY (model_profile_id) REFERENCES model_profiles(profile_id) ON DELETE RESTRICT
) STRICT;

CREATE INDEX codex_thread_bindings_profile_idx
ON codex_thread_bindings(model_profile_id, created_at_ms);
"#;

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), StorageError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
            version INTEGER PRIMARY KEY NOT NULL CHECK (version > 0), \
            applied_at_ms INTEGER NOT NULL CHECK (applied_at_ms >= 0)\
        ) STRICT;",
    )?;

    // The version check and migration share one immediate transaction. Two
    // processes opening a fresh database therefore cannot both apply version 1.
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found_version = transaction
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0);

    if found_version > LATEST_SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema {
            found: found_version,
            supported: LATEST_SCHEMA_VERSION,
        });
    }

    // Check every ledger entry instead of relying only on MAX(version). An
    // interrupted or interim build may have applied a later migration while a
    // repair migration's ledger row is absent. Skipping that gap can leave the
    // database permanently on the wrong data semantics even though its maximum
    // version looks current.
    if !migration_applied(&transaction, 1)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_1)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (1_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 2)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_2)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (2_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 3)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_3)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (3_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 4)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_4)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (4_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 5)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_5)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (5_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 6)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_6)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (6_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 7)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_7)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (7_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 8)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_8)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (8_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 9)? {
        let applied_at_ms = crate::repository::unix_time_ms()?;
        transaction.execute_batch(MIGRATION_9)?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (9_i64, applied_at_ms),
        )?;
    }

    if !migration_applied(&transaction, 10)? {
        transaction.execute_batch(
            "CREATE TABLE codex_history_locations (
                task_id TEXT PRIMARY KEY NOT NULL REFERENCES tasks(task_id) ON DELETE RESTRICT,
                home_name TEXT NOT NULL CHECK(length(home_name) = 64)
             ) STRICT;",
        )?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (10_i64, crate::repository::unix_time_ms()?),
        )?;
    }

    if !migration_applied(&transaction, 11)? {
        transaction.execute_batch(
            "CREATE TABLE project_memory_notes (
                project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects(project_id) ON DELETE RESTRICT,
                content TEXT NOT NULL CHECK(length(CAST(content AS BLOB)) <= 8192),
                revision INTEGER NOT NULL CHECK(revision > 0)
             ) STRICT;",
        )?;
        transaction.execute(
            "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (?1, ?2)",
            (11_i64, crate::repository::unix_time_ms()?),
        )?;
    }
    transaction.commit()?;

    Ok(())
}

fn migration_applied(
    transaction: &rusqlite::Transaction<'_>,
    version: i64,
) -> Result<bool, StorageError> {
    transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            [version],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use rusqlite::{Connection, OptionalExtension};

    use super::{MIGRATION_1, MIGRATION_2, migrate};

    #[test]
    fn migration_is_idempotent() {
        let mut connection = Connection::open_in_memory().expect("open database");

        migrate(&mut connection).expect("first migration");
        migrate(&mut connection).expect("second migration");

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read version");
        assert_eq!(version, 11);
    }

    #[test]
    fn version_two_database_gains_an_optional_context_window() {
        let mut connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY NOT NULL CHECK (version > 0),
                    applied_at_ms INTEGER NOT NULL CHECK (applied_at_ms >= 0)
                 ) STRICT;",
            )
            .expect("create migration ledger");
        connection
            .execute_batch(MIGRATION_1)
            .expect("apply version one schema");
        connection
            .execute_batch(MIGRATION_2)
            .expect("apply version two schema");
        connection
            .execute(
                "INSERT INTO model_profiles(
                    profile_id, name, base_url, model, dialect, credential_ref,
                    max_output_tokens, timeout_ms, is_default, created_at_ms, updated_at_ms
                 ) VALUES ('old-profile', 'Old', 'http://127.0.0.1:8000/v1', 'old-model',
                           'standard', 'old-credential', 4096, 120000, 1, 1, 1)",
                [],
            )
            .expect("insert an old profile");
        connection
            .execute(
                "INSERT INTO schema_migrations(version, applied_at_ms) VALUES (1, 1), (2, 2)",
                [],
            )
            .expect("record old versions");

        migrate(&mut connection).expect("upgrade old database");

        let version: i64 = connection
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .expect("read version");
        let context_window: Option<i64> = connection
            .query_row(
                "SELECT context_window_tokens FROM model_profiles WHERE profile_id = 'old-profile'",
                [],
                |row| row.get(0),
            )
            .optional()
            .expect("new column should be readable")
            .flatten();
        assert_eq!(version, 11);
        assert_eq!(context_window, None);
    }

    #[test]
    fn legacy_deepseek_defaults_are_repaired_to_safe_current_defaults() {
        let mut connection = Connection::open_in_memory().expect("open database");
        migrate(&mut connection).expect("create current database");

        connection
            .execute(
                "INSERT INTO model_profiles(
                    profile_id, name, base_url, model, dialect, credential_ref,
                    max_output_tokens, context_window_tokens, timeout_ms, is_default,
                    created_at_ms, updated_at_ms
                 ) VALUES ('legacy', 'DeepSeek', 'https://api.deepseek.com/v1',
                           'deepseek-chat', 'deep_seek', 'credential', 8192, 32768,
                           120000, 1, 1, 1)",
                [],
            )
            .expect("insert legacy profile");
        connection
            .execute("DELETE FROM schema_migrations WHERE version IN (5, 8)", [])
            .expect("simulate database missing both repair migrations");

        migrate(&mut connection).expect("apply output limit migration");

        let maximum: i64 = connection
            .query_row(
                "SELECT max_output_tokens FROM model_profiles WHERE profile_id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .expect("read upgraded limit");
        let context: i64 = connection
            .query_row(
                "SELECT context_window_tokens FROM model_profiles WHERE profile_id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .expect("read upgraded context");
        assert_eq!(maximum, 32_768);
        assert_eq!(context, 1_048_576);
    }

    #[test]
    fn near_full_output_profile_is_repaired_before_first_message() {
        let mut connection = Connection::open_in_memory().expect("open database");
        migrate(&mut connection).expect("create current database");

        connection
            .execute(
                "INSERT INTO model_profiles(
                    profile_id, name, base_url, model, dialect, credential_ref,
                    max_output_tokens, context_window_tokens, timeout_ms, is_default,
                    created_at_ms, updated_at_ms
                 ) VALUES ('broken', 'DeepSeek', 'https://api.deepseek.com/v1',
                           'deepseek-v4-flash', 'deep_seek', 'credential', 197632, 198656,
                           120000, 1, 1, 1)",
                [],
            )
            .expect("insert silently clamped profile");
        connection
            .execute("DELETE FROM schema_migrations WHERE version = 8", [])
            .expect("simulate version seven database");

        migrate(&mut connection).expect("repair broken profile");

        let limits: (i64, i64) = connection
            .query_row(
                "SELECT context_window_tokens, max_output_tokens
                 FROM model_profiles WHERE profile_id = 'broken'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("read repaired limits");
        assert_eq!(limits, (1_048_576, 32_768));
    }
}
