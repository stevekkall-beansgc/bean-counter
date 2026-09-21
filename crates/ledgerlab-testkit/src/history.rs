use crate::oracle::{check, HarnessError, Result};
use std::collections::BTreeMap;

/// Full row inventory read from storage. Keys and values are deterministic,
/// lossless per-dialect encodings of primary keys and ALL persisted columns.
/// Empty tables must be present. Never obtain these from an acceptance plan.
pub type RowInventory = BTreeMap<String, BTreeMap<Vec<u8>, Vec<u8>>>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Canonical envelopes assembled from original stored body bytes and metadata,
    /// ordered by (kind UTF-8, JCS(id)). Contains seed + accepted immutable records.
    pub journal: Vec<Vec<u8>>,
    /// Canonical projection objects defined by oracle/indexed_projections. All
    /// actual fields come from stored columns, never decoded body fields.
    pub indexes: Vec<Vec<u8>>,
    /// Frozen preseed-state shape, with current chain head substituted.
    pub state: Vec<u8>,
    /// Frozen post-acceptance operational shape, absent before any acceptance.
    pub operational: Option<Vec<u8>>,
    /// Original mappings are in journal; later operational aliases are separate.
    pub aliases: Vec<Alias>,
    pub rows: RowInventory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alias {
    pub scope: [String; 2],
    pub source: String,
    pub external_id: String,
    pub canonical_receipt: Vec<u8>,
    pub ingress: Vec<u8>,
    pub ingress_hash: String,
    pub observed_at: String,
}

impl Snapshot {
    pub fn validate_inventory(&self) -> Result<()> {
        check(!self.rows.is_empty(), "missing physical row inventory")?;
        for (table, rows) in &self.rows {
            check(!table.is_empty(), "empty table name")?;
            check(
                rows.keys().all(|k| !k.is_empty()),
                format!("empty primary key: {table}"),
            )?;
        }
        Ok(())
    }

    pub fn row_count(&self, table: &str) -> Result<usize> {
        self.rows
            .get(table)
            .map(BTreeMap::len)
            .ok_or_else(|| HarnessError(format!("missing table inventory: {table}")))
    }

    /// Exact no-delete/no-rewrite check, including every unlisted physical table.
    /// Expected exceptions are specific mutable tables, not blanket exclusions.
    pub fn assert_delta(
        &self,
        after: &Self,
        deltas: &BTreeMap<String, usize>,
        mutable: &[&str],
    ) -> Result<()> {
        after.validate_inventory()?;
        check(
            self.rows.keys().eq(after.rows.keys()),
            "table inventory changed",
        )?;
        for (table, old) in &self.rows {
            let new = &after.rows[table];
            let expected = deltas.get(table).copied().unwrap_or(0);
            check(
                new.len() == old.len() + expected,
                format!(
                    "unexpected row delta in {table}: {} -> {}, expected +{expected}",
                    old.len(),
                    new.len()
                ),
            )?;
            let mut updates = 0;
            for (key, body) in old {
                let current = new
                    .get(key)
                    .ok_or_else(|| HarnessError(format!("deleted {table} row")))?;
                if current != body {
                    updates += 1;
                }
                check(
                    current == body || mutable.contains(&table.as_str()),
                    format!("rewrote existing {table} row"),
                )?;
            }
            if mutable.contains(&table.as_str()) {
                check(
                    updates == 1,
                    format!("expected exactly one changed row in {table}, got {updates}"),
                )?;
            }
        }
        for table in deltas.keys() {
            check(
                self.rows.contains_key(table),
                format!("delta references missing table {table}"),
            )?;
        }
        Ok(())
    }

    pub fn assert_exact(&self, actual: &Self, context: &str) -> Result<()> {
        check(
            self == actual,
            format!("{context}: full reopened state differs"),
        )
    }
}
