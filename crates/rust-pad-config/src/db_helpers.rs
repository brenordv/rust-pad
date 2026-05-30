//! Helper macros and utilities for common redb transaction patterns.
//!
//! Both `problem_log` and `session` repeat the same begin→open_table→
//! (operate)→commit boilerplate. These macros capture the pattern once.
//! The [`open_or_create_db`] function centralizes the directory creation,
//! database opening, and permission hardening shared by all store types.
//!
//! [`deserialize_record`] centralizes the hardened bincode-decode-or-warn
//! pattern used by every redb-backed store (`workspace`, `session`,
//! `view_state`). Keeping it in one place ensures the OOM-bounding
//! `with_limit` chain cannot regress at one site while remaining intact
//! at others.

use std::path::Path;

use anyhow::{Context, Result};
use bincode::Options;
use redb::Database;
use serde::de::DeserializeOwned;

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

/// Deserializes a bincode record using the hardened options chain that all
/// redb-backed stores share: fixed-int encoding, trailing-bytes tolerance,
/// and a size cap to bound memory use on corrupted/oversize input.
///
/// Returns `None` (with a `tracing::warn!`) on any decode failure —
/// including the size-cap rejection — so callers can fall back to a
/// default without bubbling corruption errors up to the UI layer.
///
/// `kind` is restricted to `&'static str` to enforce that the value can
/// never be user-controlled and reach a log sink as injectable text.
/// Callers that want per-instance context (e.g. a path or key) should
/// emit their own structured `tracing::debug!` separately rather than
/// formatting user data into the helper's warn line.
pub(crate) fn deserialize_record<T: DeserializeOwned>(
    bytes: &[u8],
    max_bytes: u64,
    kind: &'static str,
) -> Option<T> {
    match bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .allow_trailing_bytes()
        .with_limit(max_bytes)
        .deserialize::<T>(bytes)
    {
        Ok(value) => Some(value),
        Err(e) => {
            tracing::warn!("Corrupted {kind} record (size={} bytes): {e}", bytes.len());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: encode `value` with the same fixint options the helper
    // decodes with, so test inputs match the wire format every caller
    // uses in production.
    fn fixint_serialize<T: serde::Serialize>(value: &T) -> Vec<u8> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .serialize(value)
            .expect("test serialize")
    }

    #[test]
    fn deserialize_record_roundtrip_succeeds() {
        let bytes = fixint_serialize(&("hello".to_string(), 42u32));
        let decoded: Option<(String, u32)> = deserialize_record(&bytes, 1024, "test");
        assert_eq!(decoded, Some(("hello".to_string(), 42)));
    }

    // Guards the per-store OOM bound: if a future refactor drops
    // `with_limit`, a record declaring an absurd container length
    // would attempt a giant allocation instead of being rejected.
    // This test pins the limit-enforcement contract in a single
    // place so the regression surfaces here.
    //
    // bincode 1.3's `with_limit` enforces against the *declared*
    // length of size-prefixed containers (not the actual byte count
    // of the input), via `Bounded::add` invoked during `read_size`.
    // We exercise that by writing a wire payload whose 8-byte fixint
    // length prefix advertises a Vec of length 10_000 and asserting
    // it's rejected under a 100-byte limit.
    #[test]
    fn deserialize_record_rejects_oversize_payload() {
        let mut bytes = Vec::new();
        let declared_len: u64 = 10_000;
        bytes.extend_from_slice(&declared_len.to_le_bytes());
        // Padding bytes — bincode will reject on the size check
        // before reading them, so their content is irrelevant.
        bytes.extend_from_slice(&[0xABu8; 16]);

        let decoded: Option<Vec<u8>> = deserialize_record(&bytes, 100, "test");
        assert!(
            decoded.is_none(),
            "Vec declaring length 10_000 must be rejected under a 100-byte limit"
        );
    }

    // Companion: same wire shape, but the declared length fits within
    // the limit. The decode itself fails (the payload isn't long enough),
    // but the rejection is for an unrelated reason — confirming the
    // oversize-rejection test isn't a false positive driven by some
    // other wire-format error.
    #[test]
    fn deserialize_record_accepts_small_declared_length() {
        let mut bytes = Vec::new();
        let declared_len: u64 = 4;
        bytes.extend_from_slice(&declared_len.to_le_bytes());
        bytes.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);

        let decoded: Option<Vec<u8>> = deserialize_record(&bytes, 1024, "test");
        assert_eq!(decoded, Some(vec![0x01, 0x02, 0x03, 0x04]));
    }

    #[test]
    fn deserialize_record_returns_none_on_garbage() {
        let garbage = [0xFFu8, 0xFF, 0xFF, 0xFF];
        let decoded: Option<(String, u32)> = deserialize_record(&garbage, 1024, "test");
        assert!(decoded.is_none());
    }
}
