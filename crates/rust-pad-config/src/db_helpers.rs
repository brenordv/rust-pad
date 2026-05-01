//! Helper macros and utilities for common redb transaction patterns.
//!
//! Both `problem_log` and `session` repeat the same begin→open_table→
//! (operate)→commit boilerplate. These macros capture the pattern once.
//! The [`open_or_create_db`] function centralizes the directory creation,
//! database opening, and permission hardening shared by all store types.

use std::path::Path;

use anyhow::{Context, Result};
use redb::Database;

/// Creates parent directories (if needed), opens (or creates) the redb
/// database at `path`, and sets owner-only permissions on both the
/// directory and the database file.
///
/// `label` is a human-readable store name used in error messages
/// (e.g. `"problem-log"`, `"session"`).
pub(crate) fn open_or_create_db(path: &Path, label: &str) -> Result<Database> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create {label} directory: {}", parent.display())
            })?;
            crate::permissions::set_owner_only_dir_permissions(parent);
        }
    }

    let db = Database::create(path)
        .with_context(|| format!("Failed to open {label} database: {}", path.display()))?;
    crate::permissions::set_owner_only_file_permissions(path);

    Ok(db)
}

/// Opens a read transaction and table, binding the table to `$table`.
///
/// The body block has access to the table and should return a `Result<T>`.
/// Any `?` inside the body propagates out of the enclosing function.
///
/// # Example
/// ```ignore
/// let entries = read_table!(self.db, MY_TABLE, |table| {
///     let mut v = Vec::new();
///     for item in table.iter().context("iterate")? {
///         v.push(item?);
///     }
///     Ok(v)
/// })?;
/// ```
macro_rules! read_table {
    ($db:expr, $table_def:expr, |$table:ident| $body:block) => {{
        let __read_txn = $db
            .begin_read()
            .context("Failed to begin read transaction")?;
        let $table = __read_txn
            .open_table($table_def)
            .context("Failed to open table")?;
        $body
    }};
}
pub(crate) use read_table;

/// Opens a write transaction and mutable table, executes the body, then commits.
///
/// The body block has access to a mutable table reference and should return
/// a `Result<T>`. If the body succeeds the transaction is committed; if it
/// fails (via `?`) the transaction is rolled back automatically.
///
/// # Example
/// ```ignore
/// write_table!(self.db, MY_TABLE, |table| {
///     table.insert(key, value).context("insert")?;
///     Ok(())
/// })?;
/// ```
macro_rules! write_table {
    ($db:expr, $table_def:expr, |$table:ident| $body:block) => {{
        let __write_txn = $db
            .begin_write()
            .context("Failed to begin write transaction")?;
        let __result;
        {
            let mut $table = __write_txn
                .open_table($table_def)
                .context("Failed to open table")?;
            __result = $body;
        }
        __write_txn
            .commit()
            .context("Failed to commit transaction")?;
        __result
    }};
}
pub(crate) use write_table;
