//! Mechanical storage for the ordinary retail profile. No pricing or authority.
use super::*;
use crate::service::billing::{
    ValidatedEntry, MAX_RETAINED_ALIASES, MAX_RETAINED_ALIAS_BYTES, MAX_RETAINED_ENTRIES,
    MAX_RETAINED_ENTRY_BYTES,
};

fn would_exceed_byte_limit(current: i64, additional: usize, limit: i64) -> bool {
    i64::try_from(additional)
        .ok()
        .and_then(|additional| current.checked_add(additional))
        .is_none_or(|total| total > limit)
}

pub(crate) struct BillingEntry {
    pub ordinal: i64,
    /// `None` identifies an unchanged schema-8 row, which belongs to the
    /// installation's original customer and terms profile.
    pub customer: Option<String>,
    pub source: String,
    pub external_id: String,
    pub semantic_key: Vec<u8>,
    pub ingress: Vec<u8>,
    pub facts: Vec<u8>,
    pub bundle: Vec<u8>,
    pub accepted_at_us: Option<i64>,
    pub agreement_id: Option<String>,
    pub agreement_version: Option<i64>,
}
pub(crate) struct BillingAlias {
    pub customer: Option<String>,
    pub source: String,
    pub external_id: String,
    pub ingress: Vec<u8>,
    pub ordinal: i64,
}
pub(crate) struct BillingCustomer {
    pub customer: String,
    pub tenant: String,
    pub environment: String,
}
pub(crate) struct BillingAgreement {
    pub customer: String,
    pub source: String,
    pub revision: i64,
    pub transition: String,
    pub agreement_id: String,
    pub agreement_version: i64,
    pub effective_at_us: i64,
    pub recorded_at_us: i64,
    pub setup: Option<Vec<u8>>,
}
pub(crate) struct BillingControl {
    pub customer: String,
    pub source: String,
    pub change_id: String,
    pub operation: String,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    pub recorded_at_us: i64,
}
pub(crate) struct BillingScopedPermission {
    pub customer: String,
    pub source: String,
    pub revision: i64,
    pub canonical_bytes: Vec<u8>,
    pub recorded_at_us: i64,
}
pub(crate) struct BillingSnapshot {
    pub permissions: Vec<Vec<u8>>,
    pub scoped_permissions: Vec<BillingScopedPermission>,
    pub customers: Vec<BillingCustomer>,
    pub agreements: Vec<BillingAgreement>,
    pub controls: Vec<BillingControl>,
    pub aliases: Vec<BillingAlias>,
    pub setup: Vec<u8>,
    pub entries: Vec<BillingEntry>,
    /// `true` only when `entries` and `aliases` carry their complete original
    /// payload. A metadata snapshot leaves them empty so a write never loads or
    /// decodes a retained bundle.
    pub retained: bool,
    /// `true` when the durable M3 index is present, so its clock must agree with
    /// the retained receipts. A schema-8 or schema-9 preflight reader has no
    /// index yet and leaves this `false`.
    pub indexed: bool,
    /// Installation-wide retained counts, including the schema-8 and schema-9
    /// tables. Never inferred from a truncated list.
    pub entry_count: i64,
    pub alias_count: i64,
    /// Latest accepted ledger time over every retained decision.
    pub ledger_time_max: Option<i64>,
    /// The durable index rows a complete retained snapshot carries, in ordinal
    /// order. Only the fields the entry row cannot prove are kept: identity,
    /// scope and semantic key are cross-checked against the retained entry
    /// itself, while the derived target, kind and acceptance time are re-derived
    /// from the retained receipt by the service.
    pub index: Vec<BillingIndexRow>,
}
/// One durable index row as the store read it back. The store never decides
/// whether these fields are correct; it only refuses to hand out a snapshot
/// whose index disagrees with the retained entries it read beside it.
pub(crate) struct BillingIndexRow {
    pub ordinal: i64,
    pub target: String,
    pub kind: String,
    pub accepted_at_us: i64,
}
/// One retained decision found by exact identity, across every retained tier. An
/// alias row answers with the ordinal of the entry it aliases and with the
/// alias's own retained ingress, so the caller compares the same identity
/// whether a decision or a delivery alias matched.
pub(crate) struct BillingIdentityHit {
    pub ordinal: i64,
    pub ingress: Vec<u8>,
    pub facts: Vec<u8>,
}
/// One retained decision found by exact semantic key.
pub(crate) struct BillingSemanticHit {
    pub ordinal: i64,
    pub facts: Vec<u8>,
}
pub(crate) struct BillingM2Initialization<'a> {
    pub customer: &'a str,
    pub tenant: &'a str,
    pub environment: &'a str,
    pub source: &'a str,
    pub agreement_id: &'a str,
    pub accepted_at_us: i64,
    pub setup: &'a [u8],
}

type AgreementRow = (
    String,
    String,
    i64,
    String,
    String,
    i64,
    i64,
    i64,
    Option<Vec<u8>>,
);
type ControlRow = (String, String, String, String, Vec<u8>, Vec<u8>, i64);

impl SqliteTx {
    pub(crate) async fn billing_schema_version(&mut self) -> Result<i64, StoreError> {
        sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await
            .map_err(StoreError::from)
    }

    pub(crate) async fn billing_m2_initialize(
        &mut self,
        initialization: BillingM2Initialization<'_>,
    ) -> Result<(), StoreError> {
        let BillingM2Initialization {
            customer,
            tenant,
            environment,
            source,
            agreement_id,
            accepted_at_us,
            setup,
        } = initialization;
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.require_m5_billing_schema().await?;
            let counts: (i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM billing_customers),(SELECT count(*) FROM billing_agreements)",
            )
            .fetch_one(self.conn())
            .await?;
            if counts != (0, 0) || setup.len() > 65_536 {
                return Err(StoreError::InvalidStore("M2 billing initialization state"));
            }
            sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES(?,?,?)")
                .bind(customer)
                .bind(tenant)
                .bind(environment)
                .execute(self.conn())
                .await?;
            sqlx::query("INSERT INTO billing_agreements(customer,source,revision,agreement_id,agreement_version,transition,effective_at_us,recorded_at_us,setup_bytes) VALUES(?,?,1,?,1,'start',?,?,?)")
                .bind(customer)
                .bind(source)
                .bind(agreement_id)
                .bind(accepted_at_us)
                .bind(accepted_at_us)
                .bind(setup)
                .execute(self.conn())
                .await?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }

    pub(crate) async fn billing_setup(&mut self, setup: &[u8]) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            self.require_m5_billing_schema().await?;
            let count:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM billing_setup)+(SELECT count(*) FROM events)+(SELECT count(*) FROM outcome_records)+(SELECT count(*) FROM r3_commit_witness)").fetch_one(self.conn()).await?;
            if count!=0 || setup.len()>65536 {return Err(StoreError::InvalidStore("billing installation is not empty"));}
            sqlx::query("INSERT INTO billing_setup VALUES(1,?)").bind(setup).execute(self.conn()).await?;
            Ok::<_,StoreError>(())
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
    async fn require_m3_billing_schema(&mut self) -> Result<(), StoreError> {
        let version = self.billing_schema_version().await?;
        if version != 10 && version != 11 {
            return Err(StoreError::BillingUpgradeRequired);
        }
        Ok(())
    }

    async fn require_m5_billing_schema(&mut self) -> Result<(), StoreError> {
        let version = self.billing_schema_version().await?;
        if version != 11 {
            return Err(StoreError::BillingUpgradeRequired);
        }
        Ok(())
    }

    /// M3 remains the authoritative work ledger under schema 11. Record a
    /// complete cross-stream cut in the same transaction after every M3
    /// mutation so fiscal and billing projections cannot observe a torn state.
    async fn append_m5_boundary_if_current(&mut self) -> Result<(), StoreError> {
        let version: i64 = sqlx::query_scalar("PRAGMA user_version")
            .fetch_one(self.conn())
            .await?;
        match version {
            11 => {
                super::m5::append_boundary(self.conn()).await?;
                Ok(())
            }
            _ => Err(StoreError::BillingUpgradeRequired),
        }
    }

    /// Read the retained-history meter and the durable target index under the
    /// same transaction. Every bound below is proved against the counted rows,
    /// so a tampered or partial meter refuses the store instead of silently
    /// admitting work past the ceiling.
    async fn billing_bounds(
        &mut self,
    ) -> Result<(i64, i64, i64, i64, i64, Option<i64>, i64), StoreError> {
        let (guard, entry_count, alias_count, entry_bytes, alias_bytes): (i64, i64, i64, i64, i64) =
            sqlx::query_as("SELECT guard,entry_count,alias_count,entry_bytes,alias_bytes FROM billing_m3_bounds WHERE singleton=1")
                .fetch_optional(self.conn())
                .await?
                .ok_or(StoreError::InvalidStore("billing history meter"))?;
        let counted: (i64, i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM billing_entries)+(SELECT count(*) FROM billing_m2_entries)+(SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_aliases)+(SELECT count(*) FROM billing_m2_aliases)+(SELECT count(*) FROM billing_m3_aliases),(COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m2_entries),0)+COALESCE((SELECT sum(length(bundle)+length(ingress)+length(facts)+length(semantic_key)) FROM billing_m3_entries),0)),(COALESCE((SELECT sum(length(ingress)) FROM billing_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m2_aliases),0)+COALESCE((SELECT sum(length(ingress)) FROM billing_m3_aliases),0)),(SELECT count(*) FROM billing_m3_index)",
        )
        .fetch_one(self.conn())
        .await?;
        let clock: Option<i64> =
            sqlx::query_scalar("SELECT max(accepted_at_us) FROM billing_m3_index")
                .fetch_one(self.conn())
                .await?;
        if guard != counted.0 + counted.1
            || entry_count != counted.0
            || alias_count != counted.1
            || counted.2 != entry_bytes
            || counted.3 != alias_bytes
            || counted.4 != entry_count
            || entry_count > MAX_RETAINED_ENTRIES
            || alias_count > MAX_RETAINED_ALIASES
            || entry_bytes > MAX_RETAINED_ENTRY_BYTES
            || alias_bytes > MAX_RETAINED_ALIAS_BYTES
        {
            return Err(StoreError::InvalidStore("billing history bound"));
        }
        Ok((
            entry_count,
            alias_count,
            entry_bytes,
            alias_bytes,
            guard,
            clock,
            counted.0,
        ))
    }

    /// Metadata for a write decision. It never decodes retained bundles, but
    /// verifies the retained-history meter against stored rows, so this check
    /// grows with history size.
    pub(crate) async fn billing_meta(&mut self) -> Result<BillingSnapshot, StoreError> {
        let failed = self.failed;
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.require_m5_billing_schema().await?;
            let setup: Vec<u8> =
                sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup WHERE singleton=1")
                    .fetch_one(self.conn())
                    .await?;
            let (entry_count, alias_count, _, _, _, clock, _) = self.billing_bounds().await?;
            let (permissions, scoped) = self.billing_rights().await?;
            let (customers, agreements, controls) = self.billing_controls().await?;
            // A metadata snapshot carries no retained row and no index row, so
            // nothing that depends on a decoded receipt may read from it.
            Ok::<_, StoreError>(BillingSnapshot {
                setup,
                permissions,
                scoped_permissions: scoped,
                customers,
                agreements,
                controls,
                aliases: vec![],
                entries: vec![],
                retained: false,
                indexed: true,
                entry_count,
                alias_count,
                ledger_time_max: clock,
                index: vec![],
            })
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = failed;
        }
        result
    }

    async fn billing_rights(
        &mut self,
    ) -> Result<(Vec<Vec<u8>>, Vec<BillingScopedPermission>), StoreError> {
        let permissions: Vec<Vec<u8>> =
            sqlx::query_scalar("SELECT canonical_bytes FROM billing_permissions ORDER BY revision")
                .fetch_all(self.conn())
                .await?;
        let scoped_permission_rows: Vec<(String, String, i64, Vec<u8>, i64)> = sqlx::query_as(
            "SELECT customer,source,revision,canonical_bytes,recorded_at_us FROM billing_m2_permissions ORDER BY customer,source,revision",
        )
        .fetch_all(self.conn())
        .await?;
        if permissions.len() + scoped_permission_rows.len() > 1000 {
            return Err(StoreError::InvalidStore("billing permission history bound"));
        }
        Ok((
            permissions,
            scoped_permission_rows
                .into_iter()
                .map(
                    |(customer, source, revision, canonical_bytes, recorded_at_us)| {
                        BillingScopedPermission {
                            customer,
                            source,
                            revision,
                            canonical_bytes,
                            recorded_at_us,
                        }
                    },
                )
                .collect(),
        ))
    }

    async fn billing_controls(
        &mut self,
    ) -> Result<
        (
            Vec<BillingCustomer>,
            Vec<BillingAgreement>,
            Vec<BillingControl>,
        ),
        StoreError,
    > {
        let customer_rows: Vec<(String, String, String)> = sqlx::query_as(
            "SELECT customer,tenant,environment FROM billing_customers ORDER BY customer",
        )
        .fetch_all(self.conn())
        .await?;
        let agreement_rows: Vec<AgreementRow> = sqlx::query_as("SELECT customer,source,revision,transition,agreement_id,agreement_version,effective_at_us,recorded_at_us,setup_bytes FROM billing_agreements ORDER BY customer,source,revision").fetch_all(self.conn()).await?;
        let control_rows: Vec<ControlRow> = sqlx::query_as("SELECT customer,source,change_id,operation,request,response,recorded_at_us FROM billing_m2_changes ORDER BY customer,source,change_id").fetch_all(self.conn()).await?;
        if customer_rows.len() > 1000 || agreement_rows.len() > 1000 || control_rows.len() > 2000 {
            return Err(StoreError::InvalidStore(
                "billing customer/control history bound",
            ));
        }
        Ok((
            customer_rows
                .into_iter()
                .map(|(customer, tenant, environment)| BillingCustomer {
                    customer,
                    tenant,
                    environment,
                })
                .collect(),
            agreement_rows
                .into_iter()
                .map(
                    |(
                        customer,
                        source,
                        revision,
                        transition,
                        agreement_id,
                        agreement_version,
                        effective_at_us,
                        recorded_at_us,
                        setup,
                    )| BillingAgreement {
                        customer,
                        source,
                        revision,
                        transition,
                        agreement_id,
                        agreement_version,
                        effective_at_us,
                        recorded_at_us,
                        setup,
                    },
                )
                .collect(),
            control_rows
                .into_iter()
                .map(
                    |(
                        customer,
                        source,
                        change_id,
                        operation,
                        request,
                        response,
                        recorded_at_us,
                    )| {
                        BillingControl {
                            customer,
                            source,
                            change_id,
                            operation,
                            request,
                            response,
                            recorded_at_us,
                        }
                    },
                )
                .collect(),
        ))
    }

    /// The (customer, source) pair whose revision-1 agreement carries the
    /// installation's original setup bytes. Only that pair owns schema-8 rows.
    async fn billing_legacy_owner(&mut self) -> Result<Option<String>, StoreError> {
        sqlx::query_scalar("SELECT a.customer FROM billing_agreements a JOIN billing_setup s ON a.setup_bytes=s.canonical_bytes WHERE a.revision=1")
            .fetch_optional(self.conn())
            .await
            .map_err(StoreError::from)
    }

    /// Exact identity lookup across every retained tier. Entries take precedence
    /// over delivery aliases for one scope, matching the accepted history. An
    /// alias hit carries the aliased entry's ordinal, because an alias is
    /// answered with that entry's original receipt, and the alias's own retained
    /// ingress, because that is the identity the caller submitted under.
    pub(crate) async fn billing_identity_lookup(
        &mut self,
        customer: &str,
        source: &str,
        external_id: &str,
    ) -> Result<Option<BillingIdentityHit>, StoreError> {
        let legacy = self.billing_legacy_owner().await?.as_deref() == Some(customer);
        let query = sqlx::query_as::<_, (i64, Vec<u8>, Vec<u8>)>(
            "SELECT ordinal,ingress,facts FROM billing_m3_entries WHERE customer=? AND source=? AND external_id=?",
        )
        .bind(customer)
        .bind(source)
        .bind(external_id);
        if let Some((ordinal, ingress, facts)) = query.fetch_optional(self.conn()).await? {
            return Ok(Some(BillingIdentityHit {
                ordinal,
                ingress,
                facts,
            }));
        }
        let query = sqlx::query_as::<_, (i64, Vec<u8>, Vec<u8>)>(
            "SELECT ordinal,ingress,facts FROM billing_m2_entries WHERE customer=? AND source=? AND external_id=?",
        )
        .bind(customer)
        .bind(source)
        .bind(external_id);
        if let Some((ordinal, ingress, facts)) = query.fetch_optional(self.conn()).await? {
            return Ok(Some(BillingIdentityHit {
                ordinal,
                ingress,
                facts,
            }));
        }
        if legacy {
            let query = sqlx::query_as::<_, (i64, Vec<u8>, Vec<u8>)>(
                "SELECT ordinal,ingress,facts FROM billing_entries WHERE source=? AND external_id=?",
            )
            .bind(source)
            .bind(external_id);
            if let Some((ordinal, ingress, facts)) = query.fetch_optional(self.conn()).await? {
                return Ok(Some(BillingIdentityHit {
                    ordinal,
                    ingress,
                    facts,
                }));
            }
        }
        let query = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT ordinal,ingress FROM billing_m3_aliases WHERE customer=? AND source=? AND external_id=?",
        )
        .bind(customer)
        .bind(source)
        .bind(external_id);
        if let Some((ordinal, ingress)) = query.fetch_optional(self.conn()).await? {
            return Ok(Some(BillingIdentityHit {
                ordinal,
                ingress,
                facts: vec![],
            }));
        }
        let query = sqlx::query_as::<_, (i64, Vec<u8>)>(
            "SELECT ordinal,ingress FROM billing_m2_aliases WHERE customer=? AND source=? AND external_id=?",
        )
        .bind(customer)
        .bind(source)
        .bind(external_id);
        if let Some((ordinal, ingress)) = query.fetch_optional(self.conn()).await? {
            return Ok(Some(BillingIdentityHit {
                ordinal,
                ingress,
                facts: vec![],
            }));
        }
        if legacy {
            let query = sqlx::query_as::<_, (i64, Vec<u8>)>(
                "SELECT ordinal,ingress FROM billing_aliases WHERE source=? AND external_id=?",
            )
            .bind(source)
            .bind(external_id);
            if let Some((ordinal, ingress)) = query.fetch_optional(self.conn()).await? {
                return Ok(Some(BillingIdentityHit {
                    ordinal,
                    ingress,
                    facts: vec![],
                }));
            }
        }
        Ok(None)
    }

    /// Exact semantic-key lookup. Only entries carry a semantic key; a delivery
    /// alias inherits its target's.
    pub(crate) async fn billing_semantic_lookup(
        &mut self,
        customer: &str,
        source: &str,
        semantic_key: &[u8],
    ) -> Result<Option<BillingSemanticHit>, StoreError> {
        let legacy = self.billing_legacy_owner().await?;
        type Hit = (i64, Vec<u8>);
        let rows: Vec<Hit> = sqlx::query_as("SELECT ordinal,facts FROM billing_m3_entries WHERE customer=? AND source=? AND semantic_key=?")
            .bind(customer).bind(source).bind(semantic_key)
            .fetch_all(self.conn()).await?;
        if let Some((ordinal, facts)) = rows.first() {
            return Ok(Some(BillingSemanticHit {
                ordinal: *ordinal,
                facts: facts.clone(),
            }));
        }
        let m2: Vec<Hit> = sqlx::query_as("SELECT ordinal,facts FROM billing_m2_entries WHERE customer=? AND source=? AND semantic_key=?")
            .bind(customer).bind(source).bind(semantic_key)
            .fetch_all(self.conn()).await?;
        if let Some((ordinal, facts)) = m2.first() {
            return Ok(Some(BillingSemanticHit {
                ordinal: *ordinal,
                facts: facts.clone(),
            }));
        }
        if legacy.as_deref() == Some(customer) {
            let rows: Vec<Hit> = sqlx::query_as(
                "SELECT ordinal,facts FROM billing_entries WHERE source=? AND semantic_key=?",
            )
            .bind(source)
            .bind(semantic_key)
            .fetch_all(self.conn())
            .await?;
            if let Some((ordinal, facts)) = rows.first() {
                return Ok(Some(BillingSemanticHit {
                    ordinal: *ordinal,
                    facts: facts.clone(),
                }));
            }
        }
        Ok(None)
    }

    /// The exact original retained row of one decision, from whichever tier
    /// holds its ordinal.
    pub(crate) async fn billing_entry(&mut self, ordinal: i64) -> Result<BillingEntry, StoreError> {
        for tier in [3, 2, 1] {
            let rows = self.billing_tier_rows(&[ordinal], tier).await?;
            if let Some(entry) = rows.into_iter().next() {
                return Ok(entry);
            }
        }
        Err(StoreError::InvalidStore("billing entry ordinal"))
    }

    /// The target recorded for one retained decision. The M3 index was already
    /// reconciled with the accepted receipt when this installation was opened.
    pub(crate) async fn billing_index_target(
        &mut self,
        ordinal: i64,
    ) -> Result<String, StoreError> {
        sqlx::query_scalar("SELECT target FROM billing_m3_index WHERE ordinal=?")
            .bind(ordinal)
            .fetch_optional(self.conn())
            .await?
            .ok_or(StoreError::InvalidStore("billing target index"))
    }

    /// Every retained decision of one target, from the durable index. The
    /// coordinator decodes only this target's bundles.
    pub(crate) async fn billing_target_entries(
        &mut self,
        customer: &str,
        source: &str,
        target: &str,
    ) -> Result<Vec<BillingEntry>, StoreError> {
        let ordinals: Vec<i64> = sqlx::query_scalar(
            "SELECT ordinal FROM billing_m3_index WHERE customer=? AND source=? AND target=? ORDER BY ordinal",
        )
        .bind(customer)
        .bind(source)
        .bind(target)
        .fetch_all(self.conn())
        .await?;
        if ordinals.is_empty() {
            return Ok(vec![]);
        }
        let (m3_rows, m2_rows, legacy_rows) = (
            self.billing_tier_rows(&ordinals, 3).await?,
            self.billing_tier_rows(&ordinals, 2).await?,
            self.billing_tier_rows(&ordinals, 1).await?,
        );
        let mut entries: Vec<BillingEntry> = legacy_rows
            .into_iter()
            .chain(m2_rows)
            .chain(m3_rows)
            .collect();
        entries.sort_by_key(|entry| entry.ordinal);
        if entries
            .iter()
            .enumerate()
            .any(|(index, entry)| entry.ordinal != ordinals[index])
        {
            return Err(StoreError::InvalidStore("billing target index"));
        }
        Ok(entries)
    }

    /// Read one retained tier for the requested ordinals. Tier 1 is the frozen
    /// schema-8 table, 2 the schema-9 sidecar and 3 the M3 sidecar.
    async fn billing_tier_rows(
        &mut self,
        ordinals: &[i64],
        tier: u8,
    ) -> Result<Vec<BillingEntry>, StoreError> {
        let list = ordinals
            .iter()
            .map(|ordinal| ordinal.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let sql = match tier {
            1 => format!("SELECT ordinal,NULL,source,external_id,semantic_key,ingress,facts,bundle,NULL,NULL,NULL FROM billing_entries WHERE ordinal IN ({list}) ORDER BY ordinal"),
            2 => format!("SELECT ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version FROM billing_m2_entries WHERE ordinal IN ({list}) ORDER BY ordinal"),
            _ => format!("SELECT ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version FROM billing_m3_entries WHERE ordinal IN ({list}) ORDER BY ordinal"),
        };
        type Row = (
            i64,
            Option<String>,
            String,
            String,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            Vec<u8>,
            Option<i64>,
            Option<String>,
            Option<i64>,
        );
        let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
            .fetch_all(self.conn())
            .await?;
        Ok(rows
            .into_iter()
            .map(
                |(
                    ordinal,
                    customer,
                    source,
                    external_id,
                    semantic_key,
                    ingress,
                    facts,
                    bundle,
                    accepted_at_us,
                    agreement_id,
                    agreement_version,
                )| {
                    BillingEntry {
                        ordinal,
                        customer,
                        source,
                        external_id,
                        semantic_key,
                        ingress,
                        facts,
                        bundle,
                        accepted_at_us,
                        agreement_id,
                        agreement_version,
                    }
                },
            )
            .collect())
    }

    pub(crate) async fn billing_snapshot(&mut self) -> Result<BillingSnapshot, StoreError> {
        self.billing_snapshot_inner(false).await
    }

    pub(crate) async fn billing_snapshot_for_upgrade(
        &mut self,
    ) -> Result<BillingSnapshot, StoreError> {
        self.billing_snapshot_inner(true).await
    }

    async fn billing_snapshot_inner(
        &mut self,
        allow_schema10: bool,
    ) -> Result<BillingSnapshot, StoreError> {
        let failed = self.failed;
        self.failed = true;
        let result=timeout_at(self.deadline,async {
            if allow_schema10 {
                self.require_m3_billing_schema().await?;
            } else {
                self.require_m5_billing_schema().await?;
            }
            let setup:Vec<u8>=sqlx::query_scalar("SELECT canonical_bytes FROM billing_setup WHERE singleton=1").fetch_one(self.conn()).await?;
            let (entry_count, alias_count, _, _, _, clock, _) = self.billing_bounds().await?;
            let (permissions, scoped_permission_rows) = self.billing_rights().await?;
            let (customers, agreements, controls) = self.billing_controls().await?;
            let aliases:Vec<(String,String,Vec<u8>,i64)>=sqlx::query_as("SELECT source,external_id,ingress,ordinal FROM billing_aliases ORDER BY source,external_id").fetch_all(self.conn()).await?;
            let m2_aliases:Vec<(String,String,String,Vec<u8>,i64)>=sqlx::query_as("SELECT customer,source,external_id,ingress,ordinal FROM billing_m2_aliases ORDER BY customer,source,external_id").fetch_all(self.conn()).await?;
            let m3_aliases:Vec<(String,String,String,Vec<u8>,i64)>=sqlx::query_as("SELECT customer,source,external_id,ingress,ordinal FROM billing_m3_aliases ORDER BY customer,source,external_id").fetch_all(self.conn()).await?;
            type Row=(i64,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>);
            let rows:Vec<Row>=sqlx::query_as("SELECT ordinal,source,external_id,semantic_key,ingress,facts,bundle FROM billing_entries ORDER BY ordinal").fetch_all(self.conn()).await?;
            type M2Row=(i64,String,String,String,Vec<u8>,Vec<u8>,Vec<u8>,Vec<u8>,i64,String,i64);
            let m2_rows:Vec<M2Row>=sqlx::query_as("SELECT ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version FROM billing_m2_entries ORDER BY ordinal").fetch_all(self.conn()).await?;
            let m3_rows = self.billing_tier_rows(&(1..=entry_count).collect::<Vec<i64>>(), 3).await?;
            let mut entries:Vec<BillingEntry>=rows.into_iter().map(|(ordinal,source,external_id,semantic_key,ingress,facts,bundle)|BillingEntry {ordinal,customer:None,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us:None,agreement_id:None,agreement_version:None}).collect();
            entries.extend(m2_rows.into_iter().map(|(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version)|BillingEntry {ordinal,customer:Some(customer),source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us:Some(accepted_at_us),agreement_id:Some(agreement_id),agreement_version:Some(agreement_version)}));
            entries.extend(m3_rows);
            entries.sort_by_key(|e|e.ordinal);
            if entries.len() != entry_count as usize {
                return Err(StoreError::InvalidStore("billing retained entry count"));
            }
            let mut combined_aliases:Vec<BillingAlias>=aliases.into_iter().map(|(source,external_id,ingress,ordinal)|BillingAlias {customer:None,source,external_id,ingress,ordinal}).collect();
            combined_aliases.extend(m2_aliases.into_iter().map(|(customer,source,external_id,ingress,ordinal)|BillingAlias {customer:Some(customer),source,external_id,ingress,ordinal}));
            combined_aliases.extend(m3_aliases.into_iter().map(|(customer,source,external_id,ingress,ordinal)|BillingAlias {customer:Some(customer),source,external_id,ingress,ordinal}));
            if combined_aliases.len() != alias_count as usize {
                return Err(StoreError::InvalidStore("billing retained alias count"));
            }
            let index = self.billing_index_rows().await?;
            if index.len() != entry_count as usize {
                return Err(StoreError::InvalidStore("billing target index"));
            }
            // A legacy row has no customer column of its own, so its index
            // customer is proved against the revision-1 agreement that seeded the
            // installation rather than accepted on trust.
            let owner = self.billing_legacy_owner().await?;
            let mut derived = Vec::with_capacity(index.len());
            for (row, entry) in index.iter().zip(entries.iter()) {
                let expected = entry.customer.as_deref().or(owner.as_deref());
                if row.0 != entry.ordinal
                    || expected != Some(row.1.as_str())
                    || row.2 != entry.source
                    || row.3 != entry.external_id
                    || row.4 != entry.semantic_key
                    || row.5.is_empty()
                    || !matches!(row.6.as_str(), "base" | "outcome" | "correction")
                    || entry.accepted_at_us.is_some_and(|at| at != row.7)
                {
                    return Err(StoreError::InvalidStore("billing target index"));
                }
                derived.push(BillingIndexRow {
                    ordinal: row.0,
                    target: row.5.clone(),
                    kind: row.6.clone(),
                    accepted_at_us: row.7,
                });
            }
            Ok::<_, StoreError>(BillingSnapshot {
                setup,
                permissions,
                scoped_permissions: scoped_permission_rows,
                customers,
                agreements,
                controls,
                aliases: combined_aliases,
                entries,
                retained: true,
                indexed: true,
                entry_count,
                alias_count,
                ledger_time_max: clock,
                index: derived,
            })
        }).await.map_err(|_|StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = failed;
        }
        result
    }

    /// The durable target index, ordered by retained ordinal. The agreement-
    /// scoped backfill already recorded the schema-8 owner, so a frozen row is
    /// compared through the same customer column as every other tier.
    async fn billing_index_rows(
        &mut self,
    ) -> Result<Vec<(i64, String, String, String, Vec<u8>, String, String, i64)>, StoreError> {
        sqlx::query_as("SELECT ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us FROM billing_m3_index ORDER BY ordinal")
            .fetch_all(self.conn())
            .await
            .map_err(StoreError::from)
    }
    pub(crate) async fn billing_m2_permissions(
        &mut self,
        plan: &crate::service::billing::permissions::ValidatedScopedPermissions,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.require_m5_billing_schema().await?;
            let (legacy, current): (i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM billing_permissions),(SELECT count(*) FROM billing_m2_permissions)",
            )
            .fetch_one(self.conn())
            .await?;
            if legacy + current >= 1000 || plan.request.len() > 65_536 || plan.response.len() > 65_536 {
                return Err(StoreError::InvalidStore("billing permission history bound"));
            }
            sqlx::query("INSERT INTO billing_m2_changes(customer,source,change_id,operation,request,response,recorded_at_us) VALUES(?,?,?,'permissions',?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(&plan.change_id)
                .bind(&plan.request)
                .bind(&plan.response)
                .bind(plan.recorded_at_us)
                .execute(self.conn())
                .await?;
            sqlx::query("INSERT INTO billing_m2_permissions(customer,source,revision,canonical_bytes,recorded_at_us) VALUES(?,?,?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(plan.revision)
                .bind(&plan.request)
                .bind(plan.recorded_at_us)
                .execute(self.conn())
                .await?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }

    pub(crate) async fn billing_m2_agreement(
        &mut self,
        plan: &crate::service::billing::control::ValidatedControl,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.require_m5_billing_schema().await?;
            if let Some((tenant, environment)) = &plan.new_customer_scope {
                sqlx::query("INSERT INTO billing_customers(customer,tenant,environment) VALUES(?,?,?)")
                    .bind(&plan.customer)
                    .bind(tenant)
                    .bind(environment)
                    .execute(self.conn())
                    .await?;
            }
            sqlx::query("INSERT INTO billing_m2_changes(customer,source,change_id,operation,request,response,recorded_at_us) VALUES(?,?,?,?,?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(&plan.change_id)
                .bind(&plan.operation)
                .bind(&plan.request)
                .bind(&plan.response)
                .bind(plan.recorded_at_us)
                .execute(self.conn())
                .await?;
            sqlx::query("INSERT INTO billing_agreements(customer,source,revision,agreement_id,agreement_version,transition,effective_at_us,recorded_at_us,setup_bytes) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(&plan.customer)
                .bind(&plan.source)
                .bind(plan.revision)
                .bind(&plan.agreement_id)
                .bind(plan.agreement_version)
                .bind(&plan.transition)
                .bind(plan.effective_at_us)
                .bind(plan.recorded_at_us)
                .bind(&plan.setup_bytes)
                .execute(self.conn())
                .await?;
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }

    /// Append one validated M3 decision. The store only enforces the durable
    /// bounds and the cross-tier identity and ordinal guards; every economic and
    /// authority decision was already made by the coordinator under the ordered
    /// locks this transaction already holds.
    pub(crate) async fn append_billing_m3(
        &mut self,
        plan: &ValidatedEntry,
    ) -> Result<(), StoreError> {
        self.append_billing_m3_with_limits(
            plan,
            MAX_RETAINED_ENTRY_BYTES,
            MAX_RETAINED_ALIAS_BYTES,
            true,
        )
        .await
    }

    /// The occurrence bridge appends its M5 link in the same transaction and
    /// writes one complete boundary after both streams have advanced.
    pub(crate) async fn append_billing_m3_for_occurrence(
        &mut self,
        plan: &ValidatedEntry,
    ) -> Result<(), StoreError> {
        self.append_billing_m3_with_limits(
            plan,
            MAX_RETAINED_ENTRY_BYTES,
            MAX_RETAINED_ALIAS_BYTES,
            false,
        )
        .await
    }

    async fn append_billing_m3_with_limits(
        &mut self,
        plan: &ValidatedEntry,
        entry_byte_limit: i64,
        alias_byte_limit: i64,
        append_boundary: bool,
    ) -> Result<(), StoreError> {
        if self.failed {
            return Err(StoreError::InvalidStore("poisoned billing transaction"));
        }
        self.failed = true;
        let result = timeout_at(self.deadline, async {
            self.require_m5_billing_schema().await?;
            let customer = plan
                .customer()
                .ok_or(StoreError::InvalidStore("M3 billing customer"))?;
            let (entry_count, alias_count, entry_bytes, alias_bytes, _guard, _clock, _counted) =
                self.billing_bounds().await?;
            if entry_count != plan.expected_count() {
                return Err(StoreError::InvalidStore("billing current count"));
            }
            if let Some(ordinal) = plan.alias() {
                if alias_count >= MAX_RETAINED_ALIASES
                    || would_exceed_byte_limit(
                        alias_bytes,
                        plan.ingress().len(),
                        alias_byte_limit,
                    )
                    || plan.ingress().len() > 262_144
                {
                    return Err(StoreError::BillingHistoryLimit);
                }
                sqlx::query("INSERT INTO billing_m3_aliases(customer,source,external_id,ingress,ordinal) VALUES(?,?,?,?,?)")
                    .bind(customer)
                    .bind(plan.source())
                    .bind(plan.external_id())
                    .bind(plan.ingress())
                    .bind(ordinal)
                    .execute(self.conn())
                    .await?;
                if append_boundary { self.append_m5_boundary_if_current().await?; }
                return Ok(());
            }
            let accepted_at_us = plan
                .accepted_at_us()
                .ok_or(StoreError::InvalidStore("M3 billing acceptance time"))?;
            let agreement_id = plan
                .agreement_id()
                .ok_or(StoreError::InvalidStore("M3 billing agreement"))?;
            let agreement_version = plan
                .agreement_version()
                .ok_or(StoreError::InvalidStore("M3 billing agreement version"))?;
            let target = plan
                .target()
                .ok_or(StoreError::InvalidStore("M3 billing target"))?;
            let kind = plan
                .kind()
                .ok_or(StoreError::InvalidStore("M3 billing kind"))?;
            if entry_count >= MAX_RETAINED_ENTRIES
                || would_exceed_byte_limit(entry_bytes, plan.byte_len(), entry_byte_limit)
                || plan.semantic_key().len() > 262_144
            {
                return Err(StoreError::BillingHistoryLimit);
            }
            let ordinal = entry_count + 1;
            let schema: i64 = sqlx::query_scalar("PRAGMA user_version")
                .fetch_one(self.conn()).await?;
            let assignment = if schema == 11 {
                super::m5::assignment_for_m3(self.conn(), plan).await?
            } else {
                None
            };
            sqlx::query("INSERT INTO billing_m3_entries(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
                .bind(ordinal)
                .bind(customer)
                .bind(plan.source())
                .bind(plan.external_id())
                .bind(plan.semantic_key())
                .bind(plan.ingress())
                .bind(plan.facts())
                .bind(plan.bundle())
                .bind(accepted_at_us)
                .bind(agreement_id)
                .bind(agreement_version)
                .execute(self.conn())
                .await?;
            // The unique constraints on this row are the only place a legacy,
            // schema-9 or M3 identity collision can still be caught for the first
            // row in a transaction, so it is inserted inside the same statement
            // set and any failure rolls the whole decision back.
            sqlx::query("INSERT INTO billing_m3_index(ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us) VALUES(?,?,?,?,?,?,?,?)")
                .bind(ordinal)
                .bind(customer)
                .bind(plan.source())
                .bind(plan.external_id())
                .bind(plan.semantic_key())
                .bind(target)
                .bind(kind)
                .bind(accepted_at_us)
                .execute(self.conn())
                .await?;
            if let Some(assignment) = assignment {
                sqlx::query("INSERT INTO billing_m5_assignments(customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us) VALUES(?,?,?,?,'m3',?,?,?,?,?)")
                    .bind(customer).bind(plan.source()).bind(assignment.receipt_kind)
                    .bind(&assignment.receipt_id).bind(ordinal).bind(assignment.term_version)
                    .bind(assignment.period_index).bind(assignment.basis).bind(accepted_at_us)
                    .execute(self.conn()).await?;
                if let Some(adjustment) = assignment.adjustment {
                    sqlx::query("INSERT INTO billing_m5_adjustments(customer,source_scope,adjustment_id,cause_kind,cause_id,target_id,original_term_version,original_period_index,assigned_term_version,assigned_period_index,source_stream,source_sequence,signed_delta_atoms) VALUES(?,? ,?,'outcome-correction',?,?,?,?,?,?,'m3',?,?)")
                        .bind(customer).bind(plan.source()).bind(plan.external_id())
                        .bind(plan.external_id()).bind(target)
                        .bind(adjustment.original_term_version).bind(adjustment.original_period_index)
                        .bind(assignment.term_version).bind(assignment.period_index)
                        .bind(ordinal).bind(adjustment.signed_delta_atoms)
                        .execute(self.conn()).await?;
                }
            }
            if append_boundary { self.append_m5_boundary_if_current().await?; }
            Ok::<_, StoreError>(())
        })
        .await
        .map_err(|_| StoreError::Deadline)?;
        if result.is_ok() {
            self.failed = false;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        billing::BillingLedger,
        service::billing as service,
        store::errors::CommitError,
        store::ports::{AcceptanceStore, AcceptanceTx},
    };
    const SETUP: &[u8] = include_bytes!("../../../../../examples/billing/setup.json");
    const EVENT: &[u8] = include_bytes!("../../../../../examples/billing/event.json");
    /// The production two-phase shape: a metadata snapshot, the exact retained
    /// lookups, then the service decision.
    async fn plan(store: &SqliteStore) -> (SqliteTx, service::ValidatedEntry) {
        plan_with_event(store, EVENT).await
    }
    async fn plan_with_event(
        store: &SqliteStore,
        event: &[u8],
    ) -> (SqliteTx, service::ValidatedEntry) {
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(300))
            .await
            .unwrap();
        let meta = tx.billing_meta().await.unwrap();
        let at = crate::local::now().unwrap();
        let submission =
            service::begin(&meta, "customer-1", "urn:example:work", event, &at).unwrap();
        let mut dedup = service::Dedup::empty();
        let identity = tx
            .billing_identity_lookup(
                submission.customer(),
                submission.source(),
                submission.external_id(),
            )
            .await
            .unwrap();
        if let Some(hit) = identity {
            let entry = tx.billing_entry(hit.ordinal).await.unwrap();
            let target = tx.billing_index_target(hit.ordinal).await.unwrap();
            let target_entries = tx
                .billing_target_entries(submission.customer(), submission.source(), &target)
                .await
                .unwrap();
            dedup.identity = Some(service::DedupHit::entry(
                entry,
                submission.external_id().into(),
                hit.ingress,
                hit.facts,
                target_entries,
            ));
        } else {
            let hit = tx
                .billing_semantic_lookup(
                    submission.customer(),
                    submission.source(),
                    submission.semantic_key(),
                )
                .await
                .unwrap();
            if let Some(hit) = hit {
                let entry = tx.billing_entry(hit.ordinal).await.unwrap();
                let target = tx.billing_index_target(hit.ordinal).await.unwrap();
                let target_entries = tx
                    .billing_target_entries(submission.customer(), submission.source(), &target)
                    .await
                    .unwrap();
                dedup.semantic = Some(service::DedupHit::entry(
                    entry,
                    submission.external_id().into(),
                    vec![],
                    hit.facts,
                    target_entries,
                ));
            }
        }
        let (_, plan) = service::finish(&meta, &submission, &dedup).unwrap();
        (tx, plan.unwrap())
    }

    #[tokio::test]
    async fn billing_entry_byte_limit_refuses_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let (mut tx, plan) = plan(&store).await;
        let plan_bytes = i64::try_from(plan.byte_len()).unwrap();
        assert!(!would_exceed_byte_limit(0, plan.byte_len(), plan_bytes));
        let error = tx
            .append_billing_m3_with_limits(&plan, plan_bytes - 1, MAX_RETAINED_ALIAS_BYTES, true)
            .await
            .unwrap_err();
        assert!(matches!(error, StoreError::BillingHistoryLimit));
        tx.rollback().await.unwrap();
        store.close().await;

        let ledger = BillingLedger::open(&path).await.unwrap();
        let statement = ledger.statement("customer-1", None).await.unwrap();
        assert!(statement["entries"].as_array().unwrap().is_empty());
        ledger.close().await;
    }

    #[tokio::test]
    async fn billing_alias_byte_limit_refuses_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        ledger.close().await;

        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let mut alias: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        alias["id"] = serde_json::json!("semantic-alias");
        let alias = serde_json::to_vec(&alias).unwrap();
        let (mut tx, plan) = plan_with_event(&store, &alias).await;
        assert!(plan.alias().is_some());
        let ingress_bytes = i64::try_from(plan.ingress().len()).unwrap();
        assert!(!would_exceed_byte_limit(
            0,
            plan.ingress().len(),
            ingress_bytes
        ));
        let error = tx
            .append_billing_m3_with_limits(&plan, MAX_RETAINED_ENTRY_BYTES, ingress_bytes - 1, true)
            .await
            .unwrap_err();
        assert!(matches!(error, StoreError::BillingHistoryLimit));
        tx.rollback().await.unwrap();
        store.close().await;

        let ledger = BillingLedger::open(&path).await.unwrap();
        let statement = ledger.statement("customer-1", None).await.unwrap();
        assert_eq!(statement["entries"].as_array().unwrap().len(), 1);
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(300))
            .await
            .unwrap();
        assert_eq!(tx.billing_meta().await.unwrap().alias_count, 0);
        tx.rollback().await.unwrap();
        store.close().await;
    }

    #[tokio::test]
    async fn billing_write_failure_cancel_and_unknown_commit_reopen() {
        for cut in [0, 1, 2, 3, 4] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(&path, SETUP).await.unwrap();
            let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
            if cut == 0 {
                sqlx::raw_sql("CREATE TRIGGER injected_billing_failure BEFORE INSERT ON billing_m3_entries BEGIN SELECT RAISE(ABORT,'test write failure'); END;").execute(&store.inner.writer).await.unwrap();
            }
            let old_limit: i64 = sqlx::query_scalar("PRAGMA max_page_count")
                .fetch_one(&store.inner.writer)
                .await
                .unwrap();
            if cut == 4 {
                let pages: i64 = sqlx::query_scalar("PRAGMA page_count")
                    .fetch_one(&store.inner.writer)
                    .await
                    .unwrap();
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "PRAGMA max_page_count={pages}"
                )))
                .execute(&store.inner.writer)
                .await
                .unwrap();
            }
            let (mut tx, plan) = plan(&store).await;
            if cut == 0 || cut == 4 {
                let error = tx.append_billing_m3(&plan).await.unwrap_err();
                if cut == 4 {
                    assert!(error.to_string().contains("full"), "{error}");
                }
                assert!(matches!(tx.commit().await, Err(CommitError::RolledBack(_))));
                if cut == 0 {
                    sqlx::raw_sql("DROP TRIGGER injected_billing_failure")
                        .execute(&store.inner.writer)
                        .await
                        .unwrap();
                } else {
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "PRAGMA max_page_count={old_limit}"
                    )))
                    .execute(&store.inner.writer)
                    .await
                    .unwrap();
                }
            } else {
                tx.append_billing_m3(&plan).await.unwrap();
                if cut == 3 {
                    drop(tx);
                } else {
                    store.inner.fence_cut.store(cut, Ordering::Release);
                    assert!(matches!(
                        tx.commit().await,
                        Err(CommitError::OutcomeUnknown)
                    ));
                }
            }
            store.close().await;
            let ledger = BillingLedger::open(&path).await.unwrap();
            let statement = ledger.statement("customer-1", None).await.unwrap();
            assert_eq!(
                statement["entries"].as_array().unwrap().len(),
                usize::from(cut == 2)
            );
            let accepted = ledger
                .accept("customer-1", "urn:example:work", EVENT)
                .await
                .unwrap();
            assert_eq!(
                accepted["status"],
                if cut == 2 { "duplicate" } else { "accepted" }
            );
            assert_eq!(
                ledger.statement("customer-1", None).await.unwrap()["net_atoms"],
                "250"
            );
            ledger.close().await;
        }
    }

    #[tokio::test]
    async fn billing_control_unknown_commit_and_rollback_reconcile_exactly() {
        for permission_change in [false, true] {
            for cut in [1, 2] {
                let temp = tempfile::tempdir().unwrap();
                let path = temp.path().canonicalize().unwrap().join("billing");
                BillingLedger::init(&path, SETUP).await.unwrap();
                let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
                let mut tx = store
                    .begin(Instant::now() + Duration::from_secs(5))
                    .await
                    .unwrap();
                let snapshot = tx.billing_snapshot().await.unwrap();
                // Match the production ordering: begin the serialized write,
                // read the snapshot, then sample ledger time.
                let at = crate::local::now().unwrap();
                let raw = if permission_change {
                    serde_json::to_vec(&serde_json::json!({
                        "schema":"ledger-billing-permissions/2",
                        "customer":"customer-1",
                        "source":"urn:example:work",
                        "change_id":"unknown-permission-result",
                        "expected_revision":"1",
                        "permissions":["submit"],
                        "reason":"Revoke event reads and recover a lost acknowledgement"
                    }))
                    .unwrap()
                } else {
                    let mut setup: serde_json::Value = serde_json::from_slice(SETUP).unwrap();
                    setup["price"] = serde_json::json!("5.00");
                    serde_json::to_vec(&serde_json::json!({
                        "schema":"ledger-billing-amendment/2",
                        "customer":"customer-1",
                        "source":"urn:example:work",
                        "change_id":"unknown-agreement-result",
                        "expected_revision":"1",
                        "effective_at":"9999-12-31T23:59:59.999999Z",
                        "setup":setup
                    }))
                    .unwrap()
                };
                let expected = if permission_change {
                    let (result, plan) = service::permissions::prepare_scoped(
                        &snapshot,
                        "customer-1",
                        "urn:example:work",
                        &raw,
                        &at,
                    )
                    .unwrap();
                    tx.billing_m2_permissions(&plan.unwrap()).await.unwrap();
                    result
                } else {
                    let (result, plan) = service::control::prepare(
                        &snapshot,
                        &raw,
                        &at,
                        "customer-1",
                        "urn:example:work",
                    )
                    .unwrap();
                    tx.billing_m2_agreement(&plan.unwrap()).await.unwrap();
                    result
                };
                store.inner.fence_cut.store(cut, Ordering::Release);
                assert!(matches!(
                    tx.commit().await,
                    Err(CommitError::OutcomeUnknown)
                ));
                store.close().await;

                let ledger = BillingLedger::open(&path).await.unwrap();
                let retry = if permission_change {
                    ledger
                        .permissions("customer-1", "urn:example:work", &raw)
                        .await
                        .unwrap()
                } else {
                    ledger
                        .agreement_control("customer-1", "urn:example:work", &raw)
                        .await
                        .unwrap()
                };
                assert_eq!(retry, expected);
                if permission_change {
                    assert_eq!(
                        ledger
                            .permission_status("customer-1", "urn:example:work")
                            .await
                            .unwrap()["permissions"],
                        serde_json::json!(["submit"])
                    );
                }
                ledger.close().await;

                let reopened = SqliteStore::open(&path.join(".ledger")).await.unwrap();
                let mut tx = reopened
                    .begin(Instant::now() + Duration::from_secs(5))
                    .await
                    .unwrap();
                let snapshot = tx.billing_snapshot().await.unwrap();
                assert_eq!(snapshot.controls.len(), 1);
                if permission_change {
                    assert_eq!(snapshot.scoped_permissions.len(), 1);
                    assert_eq!(snapshot.agreements.len(), 1);
                } else {
                    assert_eq!(snapshot.scoped_permissions.len(), 0);
                    assert_eq!(snapshot.agreements.len(), 2);
                }
                tx.rollback().await.unwrap();
                reopened.close().await;
            }
        }
    }

    /// Identity is resolved before the semantic key, and a retained delivery
    /// alias is an already-reserved identity rather than a route back into
    /// semantic lookup. Reserving an alias under a new delivery id is a semantic
    /// duplicate, but every later retry of that same delivery id is an identity
    /// duplicate of the original entry and its original receipt, across a reopen,
    /// appending no second alias row; changed input under that delivery id
    /// refuses with IDENTITY_CONFLICT instead of reaching the semantic key.
    #[tokio::test]
    async fn retained_alias_retries_resolve_as_identity_duplicates_without_a_second_row() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let accepted = ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        assert_eq!(accepted["status"], "accepted");
        let renamed = |id: &str| {
            let mut event: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
            event["id"] = serde_json::json!(id);
            serde_json::to_vec(&event).unwrap()
        };
        // A new delivery id with the same semantic key reserves its one alias and
        // answers with the original receipt.
        let alias_bytes = renamed("alias");
        let reserved = ledger
            .accept("customer-1", "urn:example:work", &alias_bytes)
            .await
            .unwrap();
        assert_eq!(reserved["status"], "duplicate");
        assert_eq!(reserved["kind"], "semantic");
        assert_eq!(reserved["receipt"], accepted["receipt"]);
        // The same delivery id arriving again is that same retained identity, so
        // it is answered as an identity duplicate, not a second semantic lookup.
        let replay = ledger
            .accept("customer-1", "urn:example:work", &alias_bytes)
            .await
            .unwrap();
        assert_eq!(replay["status"], "duplicate");
        assert_eq!(replay["kind"], "identity");
        assert_eq!(replay["receipt"], accepted["receipt"]);
        // A second new delivery id is still a semantic-key retry of its own.
        let second_bytes = renamed("alias-two");
        let second = ledger
            .accept("customer-1", "urn:example:work", &second_bytes)
            .await
            .unwrap();
        assert_eq!(second["status"], "duplicate");
        assert_eq!(second["kind"], "semantic");
        assert_eq!(second["receipt"], accepted["receipt"]);
        let identity = ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        assert_eq!(identity["status"], "duplicate");
        assert_eq!(identity["kind"], "identity");
        assert_eq!(identity["receipt"], accepted["receipt"]);

        // A second open needs the directory owner, so the counts are taken
        // between two ledger lifetimes rather than beside a live one.
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let (entries, aliases): (i64, i64) =
            sqlx::query_as("SELECT (SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_m3_aliases)")
                .fetch_one(&store.inner.readers)
                .await
                .unwrap();
        assert_eq!(entries, 1, "an alias reserves no second decision");
        assert_eq!(aliases, 2, "an alias retry appends no second alias row");
        store.close().await;

        // The same identity-first answer survives a reopen, because the alias is
        // resolved from the retained store rather than from one connection.
        let ledger = BillingLedger::open(&path).await.unwrap();
        for retained in [&alias_bytes, &second_bytes] {
            let replay = ledger
                .accept("customer-1", "urn:example:work", retained)
                .await
                .unwrap();
            assert_eq!(replay["status"], "duplicate");
            assert_eq!(replay["kind"], "identity");
            assert_eq!(replay["receipt"], accepted["receipt"]);
        }
        // Changed ingress under a retained delivery id refuses as an identity
        // conflict. A different quantity keeps the identity and the semantic key
        // and changes the retained facts; a different operation_id keeps the
        // identity and moves the semantic key. Neither may fall through to the
        // semantic lookup, and neither may be booked as new work.
        let mut tampered: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        tampered["id"] = serde_json::json!("alias");
        tampered["quantity"] = serde_json::json!("2");
        let mut renamed_key: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        renamed_key["id"] = serde_json::json!("alias");
        renamed_key["operation_id"] = serde_json::json!("different-operation");
        for changed in [
            serde_json::to_vec(&tampered).unwrap(),
            serde_json::to_vec(&renamed_key).unwrap(),
        ] {
            let error = ledger
                .accept("customer-1", "urn:example:work", &changed)
                .await
                .unwrap_err();
            assert_eq!(error.to_string(), "Rejection(\"IDENTITY_CONFLICT\")");
        }
        // The original identity and both refusals left the history untouched.
        let identity = ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        assert_eq!(identity["status"], "duplicate");
        assert_eq!(identity["kind"], "identity");
        assert_eq!(identity["receipt"], accepted["receipt"]);
        let statement = ledger.statement("customer-1", None).await.unwrap();
        assert_eq!(statement["entries"].as_array().unwrap().len(), 1);
        assert_eq!(statement["net_atoms"], "250");
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let (entries, aliases): (i64, i64) =
            sqlx::query_as("SELECT (SELECT count(*) FROM billing_m3_entries),(SELECT count(*) FROM billing_m3_aliases)")
                .fetch_one(&store.inner.readers)
                .await
                .unwrap();
        assert_eq!(entries, 1);
        assert_eq!(aliases, 2);
        store.close().await;
    }

    /// A forged durable index row must not survive a validated open. The store
    /// re-proves identity against the retained entry; the service re-derives the
    /// target, kind and acceptance time from the retained receipt.
    #[tokio::test]
    async fn forged_target_index_is_refused_instead_of_replayed() {
        for (column, value) in [
            ("target", "'urn:example:forged-target'"),
            ("kind", "'correction'"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(&path, SETUP).await.unwrap();
            let ledger = BillingLedger::open(&path).await.unwrap();
            ledger
                .accept("customer-1", "urn:example:work", EVENT)
                .await
                .unwrap();
            ledger.close().await;
            let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
            // Drop the immutability trigger for the tamper only: a real forgery
            // needs raw write authority, and the refusal must come from
            // validation rather than from the trigger that normally prevents it.
            sqlx::raw_sql(sqlx::AssertSqlSafe(
                "DROP TRIGGER billing_m3_index_immutable_update",
            ))
            .execute(&store.inner.writer)
            .await
            .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE billing_m3_index SET {column}={value}"
            )))
            .execute(&store.inner.writer)
            .await
            .unwrap();
            store.close().await;
            assert!(
                BillingLedger::open(&path).await.is_err(),
                "a forged {column} must not open"
            );
        }
    }

    /// M3 lifts the schema-9 ceiling of 1000 retained decisions. The store keeps
    /// exact indexed lookups at that size, still enforces the new bound, and
    /// still finds the oldest identity after later writes and a reopen.
    #[tokio::test]
    async fn m3_retained_history_crosses_the_schema9_ceiling_with_exact_lookups() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        // Synthetic retained rows: the store owns ordering, bounds and exact
        // lookups, not the economics of an opaque bundle.
        let mut seed = store.inner.writer.begin().await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(
            "WITH RECURSIVE n(i) AS (VALUES(2) UNION ALL SELECT i+1 FROM n WHERE i<1001)
             INSERT INTO billing_m3_index(ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us)
             SELECT i,'customer-1','urn:example:work',printf('synthetic-%d',i),CAST(printf('semantic-%d',i) AS BLOB),'urn:example:work','base',1 FROM n",
        ))
        .execute(&mut *seed)
        .await
        .unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(
            "WITH RECURSIVE n(i) AS (VALUES(2) UNION ALL SELECT i+1 FROM n WHERE i<1001)
             INSERT INTO billing_m3_entries(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version)
             SELECT i,'customer-1','urn:example:work',printf('synthetic-%d',i),CAST(printf('semantic-%d',i) AS BLOB),x'01',x'02',x'03',1,'agreement-1',1 FROM n",
        ))
        .execute(&mut *seed)
        .await
        .unwrap();
        super::m5::append_boundary(&mut seed).await.unwrap();
        seed.commit().await.unwrap();
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(300))
            .await
            .unwrap();
        let meta = tx.billing_meta().await.unwrap();
        assert_eq!(meta.entry_count, 1001);
        assert!(!meta.retained, "a metadata snapshot loads no bundle");
        // The oldest identity is still an exact indexed hit after 1000 later
        // decisions, and so is the newest.
        let oldest = tx
            .billing_identity_lookup("customer-1", "urn:example:work", "work-1")
            .await
            .unwrap()
            .expect("the first decision stays addressable");
        assert_eq!(oldest.ordinal, 1);
        // The first decision is the accepted event itself, so its indexed hit
        // still carries the retained facts from the entry, not an empty alias.
        assert!(!oldest.facts.is_empty());
        let newest = tx
            .billing_identity_lookup("customer-1", "urn:example:work", "synthetic-1001")
            .await
            .unwrap()
            .expect("the newest decision is addressable");
        assert_eq!(newest.ordinal, 1001);
        // An exact semantic lookup stays exact at the raised ceiling.
        assert!(tx
            .billing_semantic_lookup("customer-1", "urn:example:work", b"semantic-777")
            .await
            .unwrap()
            .is_some());
        assert!(tx
            .billing_semantic_lookup("customer-1", "urn:example:work", b"semantic-7777")
            .await
            .unwrap()
            .is_none());
        tx.rollback().await.unwrap();
        store.close().await;
        // The same holds after a reopen, so nothing depended on one connection.
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let mut tx = store
            .begin(Instant::now() + Duration::from_secs(300))
            .await
            .unwrap();
        assert_eq!(tx.billing_meta().await.unwrap().entry_count, 1001);
        assert_eq!(
            tx.billing_identity_lookup("customer-1", "urn:example:work", "work-1")
                .await
                .unwrap()
                .map(|hit| hit.ordinal),
            Some(1)
        );
        tx.rollback().await.unwrap();
        // The raised ceiling is still a durable bound, not just a wider number.
        assert!(
            sqlx::query(sqlx::AssertSqlSafe(
                "INSERT INTO billing_m3_index(ordinal,customer,source,external_id,semantic_key,target,kind,accepted_at_us) VALUES(100001,'customer-1','urn:example:work','over',x'01','urn:example:work','base',1)"
            ))
            .execute(&store.inner.writer)
            .await
            .is_err()
        );
        assert!(
            sqlx::query(sqlx::AssertSqlSafe(
                "INSERT INTO billing_m3_entries(ordinal,customer,source,external_id,semantic_key,ingress,facts,bundle,accepted_at_us,agreement_id,agreement_version) VALUES(100001,'customer-1','urn:example:work','over',x'01',x'01',x'02',x'03',1,'agreement-1',1)"
            ))
            .execute(&store.inner.writer)
            .await
            .is_err()
        );
        store.close().await;
    }

    /// The oldest identity must still answer exactly after later writes, an
    /// adjustment against its target, a reopen, and a second report. The retry
    /// appends nothing, and the complete statement is byte-identical before and
    /// after it: no truncation, no cutoff drift, no hidden growth.
    #[tokio::test]
    async fn oldest_identity_retry_after_later_writes_keeps_the_complete_report_stable() {
        const OUTCOME: &[u8] = include_bytes!("../../../../../examples/billing/outcome.json");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let first = ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        let first_target = first["receipt"]["body"]["target"]
            .as_str()
            .expect("an accepted base names its target")
            .to_owned();
        let mut second: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        second["id"] = serde_json::json!("work-2");
        second["operation_id"] = serde_json::json!("operation-2");
        second["occurred_at"] = serde_json::json!("2026-08-31T00:00:00.000000Z");
        let second = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&second).unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(second["receipt"]["body"]["target"], first_target);
        let mut outcome: serde_json::Value = serde_json::from_slice(OUTCOME).unwrap();
        outcome["target"] = serde_json::json!(first_target);
        let adjusted = ledger
            .outcome(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&outcome).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(adjusted["status"], "accepted");

        let before = ledger.statement("customer-1", None).await.unwrap();
        assert_eq!(before["complete"], true);
        assert_eq!(before["cutoff"], "3");
        assert_eq!(before["entries"].as_array().unwrap().len(), 3);
        // The explanation of the oldest target keeps that target's own history
        // complete, not just the customer-wide report.
        let explained = ledger.explain("customer-1", &first_target).await.unwrap();
        assert_eq!(explained["cutoff"], "2");
        ledger.close().await;

        let ledger = BillingLedger::open(&path).await.unwrap();
        let retry = ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        assert_eq!(retry["status"], "duplicate");
        assert_eq!(retry["kind"], "identity");
        assert_eq!(retry["receipt"], first["receipt"], "the original receipt");
        let after = ledger.statement("customer-1", None).await.unwrap();
        assert_eq!(after, before, "a duplicate retry changes no report bytes");
        assert_eq!(after["snapshot_hash"], before["snapshot_hash"]);
        ledger.close().await;

        // Three decisions were retained, so the retry appended no fourth row.
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let indexed: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m3_index")
            .fetch_one(&store.inner.readers)
            .await
            .unwrap();
        assert_eq!(indexed, 3);
        let entries: i64 = sqlx::query_scalar("SELECT count(*) FROM billing_m3_entries")
            .fetch_one(&store.inner.readers)
            .await
            .unwrap();
        assert_eq!(entries, 3);
        store.close().await;
    }

    /// One target's own history is not consecutive installation-wide. The
    /// durable index selects a single target's ordinals and deliberately leaves
    /// out every other target, so a base accepted at ordinal 1, with its outcome
    /// at 3 and its correction at 4, is a complete and valid history that has to
    /// be replayed in ordinal order rather than by adjacency. Ordinal order is
    /// proved, the per-target and customer reports reconcile exactly, and the
    /// selected rows are still the ones the index named after a reopen.
    #[tokio::test]
    async fn interleaved_target_history_replays_in_ordinal_order() {
        const OUTCOME: &[u8] = include_bytes!("../../../../../examples/billing/outcome.json");
        const CORRECTION: &[u8] = include_bytes!("../../../../../examples/billing/correction.json");
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        // Ordinal 1: the base acceptance of target A.
        let base_a = ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        assert_eq!(base_a["status"], "accepted");
        let target_a = base_a["receipt"]["body"]["target"]
            .as_str()
            .expect("an accepted base names its target")
            .to_owned();
        // Ordinal 2: an unrelated target B, so A's next own decision is 3.
        let mut second: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        second["id"] = serde_json::json!("work-2");
        second["operation_id"] = serde_json::json!("operation-2");
        second["occurred_at"] = serde_json::json!("2026-08-31T00:00:00.000000Z");
        let base_b = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&second).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(base_b["status"], "accepted");
        let target_b = base_b["receipt"]["body"]["target"]
            .as_str()
            .expect("an accepted base names its target")
            .to_owned();
        assert_ne!(target_a, target_b);
        // Ordinal 3: an outcome against A, whose own retained ordinals are the
        // non-contiguous 1 and 3.
        let mut outcome: serde_json::Value = serde_json::from_slice(OUTCOME).unwrap();
        outcome["target"] = serde_json::json!(target_a);
        let adjusted = ledger
            .outcome(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&outcome).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(adjusted["status"], "accepted");
        assert_eq!(adjusted["receipt"]["kind"], "receipt");
        // Ordinal 4: a correction of A, whose own retained ordinals are now the
        // non-contiguous 1, 3 and 4.
        let mut correction: serde_json::Value = serde_json::from_slice(CORRECTION).unwrap();
        correction["target"] = serde_json::json!(target_a);
        let corrected = ledger
            .correct(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&correction).unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(corrected["status"], "accepted");
        assert_eq!(corrected["receipt"]["kind"], "receipt");

        // Each target report covers exactly that target's own decisions, so its
        // cutoff counts A's three rows and B's one, not the four installation
        // ordinals between them.
        let report_a = ledger.explain("customer-1", &target_a).await.unwrap();
        let report_b = ledger.explain("customer-1", &target_b).await.unwrap();
        let customer = ledger.statement("customer-1", None).await.unwrap();
        for report in [&report_a, &report_b, &customer] {
            assert_eq!(report["complete"], true, "no report may be truncated");
        }
        assert_eq!(report_a["cutoff"], "3");
        assert_eq!(report_b["cutoff"], "1");
        assert_eq!(customer["cutoff"], "4");
        let rows = |report: &serde_json::Value| {
            report["entries"]
                .as_array()
                .expect("a report entry list")
                .clone()
        };
        let charged = |report: &serde_json::Value| {
            rows(report)
                .iter()
                .map(|entry| (entry["external_id"].clone(), entry["net_atoms"].clone()))
                .collect::<Vec<_>>()
        };
        // The rebate is booked once against A and exactly reversed by its
        // correction, so A is back to its base charge and the two target
        // reports reconcile to the customer report.
        assert_eq!(
            charged(&report_a),
            vec![
                (serde_json::json!("work-1"), serde_json::json!("250")),
                (serde_json::json!("quality-1"), serde_json::json!("-50")),
                (serde_json::json!("correction-1"), serde_json::json!("50")),
            ]
        );
        assert_eq!(
            charged(&report_b),
            vec![(serde_json::json!("work-2"), serde_json::json!("250"))]
        );
        assert_eq!(report_a["net_atoms"], "250");
        assert_eq!(report_b["net_atoms"], "250");
        assert_eq!(customer["net_atoms"], "500");
        let summed: i128 = [&report_a, &report_b]
            .iter()
            .map(|report| {
                report["net_atoms"]
                    .as_str()
                    .expect("a signed atom total")
                    .parse::<i128>()
                    .unwrap()
            })
            .sum();
        assert_eq!(summed.to_string(), customer["net_atoms"].as_str().unwrap());
        // Every receipt a caller received is the exact receipt its own target's
        // report carries, and the customer report carries the same receipts for
        // the same decisions, in installation ordinal order.
        let a_rows = rows(&report_a);
        assert!(
            a_rows
                .iter()
                .all(|entry| entry["target"] == serde_json::json!(target_a)),
            "A's own report explains A alone"
        );
        assert!(rows(&report_b)
            .iter()
            .all(|entry| entry["target"] == serde_json::json!(target_b)));
        assert_eq!(
            a_rows
                .iter()
                .map(|entry| entry["receipt"].clone())
                .collect::<Vec<_>>(),
            vec![
                base_a["receipt"].clone(),
                adjusted["receipt"].clone(),
                corrected["receipt"].clone(),
            ]
        );
        let customer_rows = rows(&customer);
        assert_eq!(
            charged(&customer),
            vec![
                (serde_json::json!("work-1"), serde_json::json!("250")),
                (serde_json::json!("work-2"), serde_json::json!("250")),
                (serde_json::json!("quality-1"), serde_json::json!("-50")),
                (serde_json::json!("correction-1"), serde_json::json!("50")),
            ]
        );
        let b_rows = rows(&report_b);
        for entry in a_rows.iter().chain(b_rows.iter()) {
            assert!(
                customer_rows
                    .iter()
                    .any(|retained| retained["receipt"] == entry["receipt"]),
                "a per-target receipt is the customer report's own receipt"
            );
        }
        // Every target is reported on the same retained rows after a reopen, so
        // the replay came from the index rather than from one connection.
        ledger.close().await;
        let ledger = BillingLedger::open(&path).await.unwrap();
        assert_eq!(
            ledger.explain("customer-1", &target_a).await.unwrap(),
            report_a
        );
        assert_eq!(
            ledger.explain("customer-1", &target_b).await.unwrap(),
            report_b
        );
        assert_eq!(
            ledger.statement("customer-1", None).await.unwrap(),
            customer
        );
        ledger.close().await;

        // Four decisions were retained, and A's three of them are selected from
        // ordinals 1, 3 and 4 rather than from a consecutive run.
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        let (indexed, entries): (i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM billing_m3_index),(SELECT count(*) FROM billing_m3_entries)",
        )
        .fetch_one(&store.inner.readers)
        .await
        .unwrap();
        assert_eq!((indexed, entries), (4, 4));
        let ordinals: Vec<i64> = sqlx::query_scalar(
            "SELECT ordinal FROM billing_m3_index WHERE target=? ORDER BY ordinal",
        )
        .bind(&target_a)
        .fetch_all(&store.inner.readers)
        .await
        .unwrap();
        assert_eq!(ordinals, vec![1, 3, 4]);
        store.close().await;
    }

    /// Two concurrent identical submissions of one identity must agree on a
    /// single retained entry: the same receipt, one row, no conflict.
    #[tokio::test]
    async fn concurrent_identical_submissions_retain_one_entry() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = std::sync::Arc::new(BillingLedger::open(&path).await.unwrap());
        let (left, right) = tokio::join!(
            {
                let ledger = ledger.clone();
                async move { ledger.accept("customer-1", "urn:example:work", EVENT).await }
            },
            {
                let ledger = ledger.clone();
                async move { ledger.accept("customer-1", "urn:example:work", EVENT).await }
            }
        );
        let left = left.unwrap();
        let right = right.unwrap();
        let accepted = [left.clone(), right.clone()]
            .into_iter()
            .filter(|value| value["status"] == "accepted")
            .count();
        assert_eq!(accepted, 1, "exactly one submission may be accepted");
        for value in [left, right] {
            assert!(
                value["status"] == "accepted" || value["status"] == "duplicate",
                "a concurrent identical submission is never a conflict: {value}"
            );
            if value["status"] == "duplicate" {
                assert_eq!(value["kind"], "identity");
            }
        }
        let statement = ledger.statement("customer-1", None).await.unwrap();
        assert_eq!(statement["cutoff"], "1");
        std::sync::Arc::try_unwrap(ledger)
            .map_err(|_| "a ledger handle outlives the test")
            .unwrap()
            .close()
            .await;
    }

    /// The retained-decision meter is derived, never trusted. A forged guard,
    /// count or byte total must be refused on read instead of being believed.
    #[tokio::test]
    async fn forged_retained_bounds_are_refused_instead_of_believed() {
        for (column, value) in [
            ("guard", "9"),
            ("entry_count", "9"),
            ("alias_count", "9"),
            ("entry_bytes", "999999"),
            ("alias_bytes", "999999"),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(&path, SETUP).await.unwrap();
            let ledger = BillingLedger::open(&path).await.unwrap();
            ledger
                .accept("customer-1", "urn:example:work", EVENT)
                .await
                .unwrap();
            ledger.close().await;
            let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
            // Only the immutability guard is stood down; the refusal under test
            // must come from deriving the meter from the retained rows.
            sqlx::raw_sql(sqlx::AssertSqlSafe("DROP TRIGGER billing_m3_bounds_guard"))
                .execute(&store.inner.writer)
                .await
                .unwrap();
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "UPDATE billing_m3_bounds SET {column}={value}"
            )))
            .execute(&store.inner.writer)
            .await
            .unwrap();
            let mut tx = store
                .begin(Instant::now() + Duration::from_secs(300))
                .await
                .unwrap();
            assert!(
                tx.billing_meta().await.is_err(),
                "a forged {column} must not be believed"
            );
            assert!(tx.billing_snapshot().await.is_err());
            tx.rollback().await.unwrap();
            store.close().await;
            assert!(
                BillingLedger::open(&path).await.is_err(),
                "a forged {column} must not open"
            );
        }
    }

    #[tokio::test]
    async fn populated_billing_tables_refuse_mutation_without_changing_history() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(&path, SETUP).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        ledger
            .accept("customer-1", "urn:example:work", EVENT)
            .await
            .unwrap();
        let mut alias: serde_json::Value = serde_json::from_slice(EVENT).unwrap();
        alias["id"] = serde_json::json!("alias");
        ledger
            .accept(
                "customer-1",
                "urn:example:work",
                &serde_json::to_vec(&alias).unwrap(),
            )
            .await
            .unwrap();
        ledger.permissions("customer-1", "urn:example:work", br#"{"schema":"ledger-billing-permissions/2","customer":"customer-1","source":"urn:example:work","change_id":"revoke-submit","expected_revision":"1","permissions":["read"],"reason":"test revocation"}"#).await.unwrap();
        let before = ledger.statement("customer-1", None).await.unwrap();
        let permissions = ledger
            .permission_status("customer-1", "urn:example:work")
            .await
            .unwrap();
        ledger.close().await;
        let store = SqliteStore::open(&path.join(".ledger")).await.unwrap();
        for (table, column) in [
            ("billing_setup", "canonical_bytes"),
            ("billing_customers", "customer"),
            ("billing_agreements", "agreement_id"),
            ("billing_m3_entries", "bundle"),
            ("billing_m3_index", "semantic_key"),
            ("billing_m3_aliases", "ingress"),
            ("billing_m2_changes", "request"),
            ("billing_m2_permissions", "canonical_bytes"),
        ] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&store.inner.readers)
                    .await
                    .unwrap();
            assert!(count > 0);
            for sql in [
                format!("UPDATE {table} SET {column}={column}"),
                format!("DELETE FROM {table}"),
            ] {
                let error = sqlx::query(sqlx::AssertSqlSafe(sql))
                    .execute(&store.inner.writer)
                    .await
                    .unwrap_err();
                assert!(error.to_string().contains("immutable billing"), "{error}");
            }
        }
        // The frozen schema-9 decision tables stay empty here: a fresh
        // schema-10 store never appends to them, so their row immutability is
        // proven by the migration upgrade tests instead.
        for table in ["billing_m2_entries", "billing_m2_aliases"] {
            let count: i64 =
                sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                    .fetch_one(&store.inner.readers)
                    .await
                    .unwrap();
            assert_eq!(count, 0, "{table} must stay empty on a schema-10 store");
        }
        store.close().await;
        let ledger = BillingLedger::open(&path).await.unwrap();
        assert_eq!(ledger.statement("customer-1", None).await.unwrap(), before);
        assert_eq!(
            ledger
                .permission_status("customer-1", "urn:example:work")
                .await
                .unwrap(),
            permissions
        );
        assert_eq!(
            ledger
                .accept(
                    "customer-1",
                    "urn:example:work",
                    &serde_json::to_vec(&alias).unwrap(),
                )
                .await
                .unwrap()["status"],
            "duplicate"
        );
        ledger.close().await;
    }
    #[test]
    #[ignore = "subprocess crash helper; invoked by billing_process_exit_reopens_atomically"]
    fn billing_crash_child() {
        let path = std::env::var_os("LEDGER_BILLING_CRASH_PATH").expect("parent supplies path");
        let cut: u8 = std::env::var("LEDGER_BILLING_CRASH_CUT")
            .unwrap()
            .parse()
            .unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let store = SqliteStore::open(&std::path::PathBuf::from(path).join(".ledger"))
                    .await
                    .unwrap();
                let (mut tx, plan) = plan(&store).await;
                tx.append_billing_m3(&plan).await.unwrap();
                store.inner.fence_cut.store(cut, Ordering::Release);
                tx.commit().await.unwrap();
            });
        panic!("crash cut not reached");
    }
    #[tokio::test]
    async fn billing_process_exit_reopens_atomically() {
        for cut in [11, 12] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(&path, SETUP).await.unwrap();
            let out = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "store::sqlite::billing::tests::billing_crash_child",
                    "--ignored",
                ])
                .env("LEDGER_BILLING_CRASH_PATH", &path)
                .env("LEDGER_BILLING_CRASH_CUT", cut.to_string())
                .output()
                .unwrap();
            assert_eq!(
                out.status.code(),
                Some(77),
                "{}",
                String::from_utf8_lossy(&out.stdout)
            );
            let ledger = BillingLedger::open(&path).await.unwrap();
            let result = ledger
                .accept("customer-1", "urn:example:work", EVENT)
                .await
                .unwrap();
            assert_eq!(
                result["status"],
                if cut == 12 { "duplicate" } else { "accepted" }
            );
            assert_eq!(
                ledger.statement("customer-1", None).await.unwrap()["net_atoms"],
                "250"
            );
            ledger.close().await;
        }
    }
}
