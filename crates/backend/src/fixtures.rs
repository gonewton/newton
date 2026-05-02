//! Test-only fixtures loaded from `openapi/newton-backend-parity.fixtures.json`.
//!
//! Production deployments do NOT load fixtures — migrations create empty
//! tables and real data accumulates from user actions. This module is only
//! exercised by integration tests and `POST /api/testing/reset`.

use crate::err_internal;
use indexmap::IndexMap;
use newton_types::ApiError;
use serde_json::Value;
use sqlx::{Executor, Sqlite, Transaction};
use std::sync::OnceLock;

/// Parsed fixture payload: table name -> rows. `IndexMap` preserves the JSON
/// declaration order so we insert in FK-safe order (parents before children).
pub type Fixtures = IndexMap<String, Vec<IndexMap<String, Value>>>;

const FIXTURES_RAW: &str = include_str!("../../../openapi/newton-backend-parity.fixtures.json");

/// Lazily parse the embedded fixtures. Parse errors panic at first use,
/// loud and intentional — a malformed fixtures file is a build-time bug.
pub fn fixtures() -> &'static Fixtures {
    static CACHED: OnceLock<Fixtures> = OnceLock::new();
    CACHED.get_or_init(|| {
        let mut raw: IndexMap<String, Value> = serde_json::from_str(FIXTURES_RAW)
            .expect("fixtures JSON must parse; this is a build-time invariant");
        // Discard non-table metadata keys (anything starting with '_').
        raw.retain(|k, _| !k.starts_with('_'));
        raw.into_iter()
            .map(|(table, rows)| {
                let rows: Vec<IndexMap<String, Value>> = serde_json::from_value(rows)
                    .unwrap_or_else(|e| panic!("fixtures table {table} malformed: {e}"));
                (table, rows)
            })
            .collect()
    })
}

/// Insert all fixture rows into the given transaction. Tables are inserted in
/// JSON declaration order; the file MUST list parents before children.
pub async fn load_fixtures(tx: &mut Transaction<'_, Sqlite>) -> Result<(), ApiError> {
    for (table, rows) in fixtures() {
        for row in rows {
            insert_row(tx, table, row).await?;
        }
    }
    Ok(())
}

async fn insert_row(
    tx: &mut Transaction<'_, Sqlite>,
    table: &str,
    row: &IndexMap<String, Value>,
) -> Result<(), ApiError> {
    if !is_safe_ident(table) {
        return Err(err_internal(&format!("unsafe table name: {table}")));
    }
    let cols: Vec<&str> = row.keys().map(String::as_str).collect();
    for c in &cols {
        if !is_safe_ident(c) {
            return Err(err_internal(&format!("unsafe column name: {c}")));
        }
    }

    let placeholders = vec!["?"; cols.len()].join(", ");
    let cols_list = cols.join(", ");
    let sql = format!("INSERT INTO {table} ({cols_list}) VALUES ({placeholders})");

    let mut q = sqlx::query(&sql);
    for col in &cols {
        let v = &row[*col];
        q = match v {
            Value::Null => q.bind(Option::<String>::None),
            Value::Bool(b) => q.bind(if *b { 1_i64 } else { 0_i64 }),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    q.bind(i)
                } else if let Some(f) = n.as_f64() {
                    q.bind(f)
                } else {
                    return Err(err_internal(&format!(
                        "unsupported numeric in fixture {table}.{col}: {n}"
                    )));
                }
            }
            Value::String(s) => q.bind(s.clone()),
            Value::Array(_) | Value::Object(_) => q.bind(v.to_string()),
        };
    }

    tx.execute(q)
        .await
        .map_err(|e| err_internal(&format!("seed {table} error: {e}")))?;
    Ok(())
}

/// Permissive but safe identifier check — table/column names from the
/// fixtures file are interpolated into SQL without parameterization.
/// Allow ASCII letters, digits, and underscore.
fn is_safe_ident(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_parse() {
        let f = fixtures();
        // Spot-check: every parity-resident SQLite table from the spec must
        // have at least one row, otherwise §10.1 coverage is incomplete.
        for required in [
            "Product",
            "Component",
            "Repo",
            "Module",
            "ModuleDependency",
            "PendingApproval",
            "Indicator",
            "Regression",
            "Opportunity",
            "Request",
            "Plan",
            "PlanSection",
            "PlanPolicyCheck",
            "PlanApprover",
            "ExecutionRecord",
            "RecentAction",
            "SavedView",
            "Operator",
            "Persistence",
        ] {
            assert!(
                f.get(required).map(|v| !v.is_empty()).unwrap_or(false),
                "fixtures missing required table: {required}"
            );
        }
    }

    #[test]
    fn ident_validator_rejects_injection() {
        assert!(!is_safe_ident("Product; DROP TABLE Plan"));
        assert!(!is_safe_ident(""));
        assert!(is_safe_ident("Plan"));
        assert!(is_safe_ident("plan_id"));
    }
}
