//! Local filesystem-authorized ordinary SQLite billing. Not a remote API.
use crate::{
    local::{self, LocalError},
    service::{billing as service, store_error},
    store::{
        errors::CommitError,
        ports::{AcceptanceStore, AcceptanceTx},
        records::Installation,
        sqlite::{BillingEntry, SqliteStore, SqliteTx},
    },
    ServiceError,
};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Duration};
use tokio::time::Instant;

/// Version of the local billing facade and JSON contract family.
/// Frozen economic record profiles have their own independent versions.
pub const CONTRACT_VERSION: &str = "v0.3";

mod export;

pub struct BillingLedger {
    store: SqliteStore,
}
impl BillingLedger {
    /// Validate operator setup input and return a safe summary for confirmation.
    /// Evidence text is deliberately omitted from the returned summary.
    pub fn setup_summary(raw: &[u8]) -> local::Result<Value> {
        let setup = service::Setup::parse(raw)?;
        let family = &setup.outcome_policy["families"][0];
        let codes = family["codes"]
            .as_array()
            .expect("validated outcome codes")
            .iter()
            .map(|code| {
                json!({
                    "code": code["code"],
                    "currency": code["amount"]["money"]["currency"],
                    "scale": code["amount"]["money"]["scale"],
                    "atoms": code["amount"]["money"]["atoms"]
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "profile": "local-retail",
            "schema": setup.schema,
            "scope": setup.scope,
            "store_id": setup.store_id,
            "source": setup.source,
            "customer": setup.customer,
            "host": setup.host,
            "agreement": setup.agreement,
            "binding": setup.binding,
            "price_usd": setup.price,
            "accepted_at": setup.accepted_at,
            "outcome_family": family["family"],
            "outcome_codes": codes,
            "ordinary_window": family["ordinary"],
            "correction_window": family["corrections"],
            "permissions": setup.permissions,
            "replacement_codes": family["replacement_codes"],
            "allow_reversal": family["allow_reversal"],
            "premium_ceiling": setup.outcome_policy["limits"][0]["premium"],
            "operator_evidence": {
                "assent": !setup.assent_evidence.is_empty(),
                "authority_attestation": !setup.operator_attestation.is_empty(),
                "finality_attestation": !setup.finality_attestation.is_empty()
            }
        }))
    }

    /// Create a separate real-terms installation. Existing destinations refuse;
    /// initialization stores no accepted work or synthetic economic history.
    pub async fn init(path: &Path, setup: &[u8]) -> local::Result<()> {
        let setup = service::Setup::parse(setup)?;
        let path = local::normalize_path(path)?;
        if path.exists() {
            if !path.is_dir() || fs::read_dir(&path)?.next().is_some() {
                return Err(LocalError::Config(
                    "billing init requires a new or empty directory",
                ));
            }
            local::private_existing(&path, true)?;
        } else {
            local::private_dir(&path)?;
        }
        let data = path.join(".ledger");
        local::private_dir(&data)?;
        let store = SqliteStore::create(
            &data,
            Installation {
                scope: crate::store::records::Scope {
                    tenant: setup.scope.tenant().into(),
                    environment: setup.scope.environment().into(),
                },
                logical_store_id: setup.store_id.clone(),
                mode: "real".into(),
                admission: "open".into(),
                dispatch_hold: true,
                dispatch_enabled: false,
                generation: 0,
            },
        )
        .await
        .map_err(store_error)?;
        let result=async {
            let raw=ledgerlab_core::canonical::outcome::bytes(&json!(setup)).map_err(|_|ServiceError::IntegrityFailure)?;
            let mut tx=store.begin(Instant::now()+Duration::from_secs(5)).await.map_err(store_error)?;
            tx.billing_setup(&raw).await.map_err(store_error)?;
            tx.billing_m2_initialize(crate::store::sqlite::BillingM2Initialization {
                customer: &setup.customer,
                tenant: setup.scope.tenant(),
                environment: setup.scope.environment(),
                source: &setup.source,
                agreement_id: &setup.agreement,
                accepted_at_us: setup.accepted_at.micros(),
                setup: &raw,
            }).await.map_err(store_error)?;
            // Initialization has no economic delivery to resolve. A failed init
            // is left visible and never silently replaced or opened as ready.
            tx.commit().await.map_err(|_|ServiceError::Unavailable)?;
            let config=serde_json::to_vec_pretty(&json!({"schema":"ledger-billing-installation/1","scope":setup.scope,"store_id":setup.store_id})).map_err(|_|ServiceError::IntegrityFailure)?;
            local::write_new(&path.join("billing.json"),&config)?;
            local::write_new(&path.join(".gitignore"),b".ledger/\n")?;
            Ok::<_,LocalError>(())
        }.await;
        store.close().await;
        result
    }
    pub async fn open(path: &Path) -> local::Result<Self> {
        let path = local::normalize_path(path)?;
        local::private_existing(&path, true)?;
        let raw = local::read_file(&path.join("billing.json"), local::CONFIG_LIMIT)?;
        let config = ledgerlab_core::canonical::parse(&raw)
            .map_err(|_| LocalError::Config("invalid billing.json"))?;
        if config.as_object().is_none_or(|o| o.len() != 3)
            || config["schema"] != "ledger-billing-installation/1"
        {
            return Err(LocalError::Config("invalid billing installation"));
        }
        let data = path.join(".ledger");
        local::private_existing(&data, true)?;
        local::private_existing(&data.join("local.db"), false)?;
        let store = SqliteStore::open(&data).await.map_err(store_error)?;
        let result = async {
            let mut tx = store
                .begin(Instant::now() + Duration::from_secs(5))
                .await
                .map_err(store_error)?;
            let installation = tx.load_installation().await.map_err(store_error)?;
            let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
            service::validate_snapshot(&snapshot)?;
            let setup = service::Setup::parse(&snapshot.setup)?;
            service::require(
                config["scope"] == json!(setup.scope)
                    && config["store_id"] == setup.store_id
                    && installation.logical_store_id == setup.store_id
                    && installation.scope.tenant == setup.scope.tenant()
                    && installation.scope.environment == setup.scope.environment()
                    && installation.mode == "real"
                    && installation.admission == "open"
                    && installation.dispatch_hold
                    && !installation.dispatch_enabled,
                "BILLING_INSTALLATION",
            )?;
            tx.rollback().await.map_err(store_error)?;
            Ok::<_, ServiceError>(())
        }
        .await;
        if let Err(e) = result {
            store.close().await;
            return Err(e.into());
        }
        Ok(Self { store })
    }

    /// Explicitly upgrade a schema-8 or schema-9 billing installation after
    /// validating its complete retained history against the exact snapshot to
    /// be migrated.
    pub async fn upgrade(path: &Path) -> local::Result<Value> {
        let path = local::normalize_path(path)?;
        local::private_existing(&path, true)?;
        let raw_config = local::read_file(&path.join("billing.json"), local::CONFIG_LIMIT)?;
        let config = ledgerlab_core::canonical::parse(&raw_config)
            .map_err(|_| LocalError::Config("invalid billing.json"))?;
        if config.as_object().is_none_or(|object| object.len() != 3)
            || config["schema"] != "ledger-billing-installation/1"
        {
            return Err(LocalError::Config("invalid billing installation"));
        }
        let data = path.join(".ledger");
        local::private_existing(&data, true)?;
        local::private_existing(&data.join("local.db"), false)?;

        // A store that opens at the current schema is already reconciled: a
        // fully validated schema-10 open proves the previous unknown result
        // committed, and no migration work is left to do.
        let open_error = match Self::open(&path).await {
            Ok(ledger) => {
                ledger.close().await;
                return Ok(json!({"status":"already_current","from_schema":10,"to_schema":10}));
            }
            Err(error) => error,
        };

        let preflight = crate::store::sqlite::migrate::preflight_billing(&data)
            .await
            .map_err(|error| match error {
                crate::maintenance::UpgradeError::Refused => {
                    service::reject("BILLING_UPGRADE_REFUSED")
                }
                crate::maintenance::UpgradeError::OutcomeUnknown => ServiceError::Unavailable,
            })?;
        let crate::store::sqlite::migrate::BillingUpgradePreflight {
            version,
            store_version,
            mut snapshot,
            digest,
        } = preflight;
        // Schema 10 is already the migration target. Its upgrade retry is
        // successful only when the complete billing open above validated it;
        // a pre-M3 row reconciliation cannot stand in for current M3 checks.
        if store_version == 10 {
            return Err(open_error);
        }
        // `version` is the frozen pre-M3 schema the digest and the derived index
        // belong to, which on a schema-10 store is the recorded source schema.
        service::require(version == 8 || version == 9, "BILLING_UPGRADE_REFUSED")?;
        let migration_at = local::now()?;
        // Each source schema is validated against its own complete state. A
        // schema-8 store has no customer, agreement, control or scoped-permission
        // rows yet, so the migration seeds its first customer/agreement from the
        // setup and proves the history against that. A schema-9 store already
        // carries that state from M2, so it is validated where it stands and
        // nothing about it is seeded, defaulted or repriced.
        let setup = if version == 8 {
            service::validate_legacy_upgrade(&mut snapshot, &migration_at)?
        } else {
            service::validate_m2_upgrade(&snapshot, &migration_at)?
        };
        service::require(
            config["scope"] == json!(setup.scope) && config["store_id"] == setup.store_id,
            "BILLING_INSTALLATION",
        )?;
        // The index is derived from the accepted history, so a migration never
        // invents a target, a kind or an acceptance time.
        let index = service::upgrade_index(&snapshot)?
            .into_iter()
            .map(|row| crate::store::sqlite::migrate::BillingUpgradeIndex {
                ordinal: row.ordinal,
                customer: row.customer,
                source: row.source,
                external_id: row.external_id,
                semantic_key: row.semantic_key,
                target: row.target,
                kind: row.kind,
                accepted_at_us: row.accepted_at_us,
            })
            .collect();
        let seed = crate::store::sqlite::migrate::BillingUpgradeSeed {
            store_id: setup.store_id.clone(),
            tenant: setup.scope.tenant().to_owned(),
            environment: setup.scope.environment().to_owned(),
            customer: setup.customer.clone(),
            source: setup.source.clone(),
            agreement_id: setup.agreement.clone(),
            effective_at_us: setup.accepted_at.micros(),
            recorded_at_us: migration_at.micros(),
            setup_bytes: snapshot.setup.clone(),
            snapshot_digest: digest,
            source_version: version,
            index,
        };
        let result = crate::store::sqlite::migrate::upgrade_billing(&data, &seed)
            .await
            .map_err(|error| match error {
                crate::maintenance::UpgradeError::Refused => {
                    service::reject("BILLING_UPGRADE_REFUSED")
                }
                crate::maintenance::UpgradeError::OutcomeUnknown => {
                    service::reject("BILLING_UPGRADE_OUTCOME_UNKNOWN")
                }
            })?;
        // The storage layer may reconcile a schema-10 index committed by a
        // concurrent upgrader. Its `already_current` result is not a full
        // billing-health claim; the response path below validates the profile.
        match result {
            crate::maintenance::UpgradeResult::Upgraded => {
                Ok(json!({"status":"upgraded","from_schema":store_version,"to_schema":10}))
            }
            crate::maintenance::UpgradeResult::AlreadyCurrent => {
                Self::confirm_already_current(&path).await
            }
        }
    }
    /// A concurrent upgrader may have advanced the store after our schema-9
    /// preflight. Re-open through the full billing validator before claiming
    /// that its schema-10 result is current; the migration's index reconciliation
    /// alone does not validate every M3 billing invariant.
    pub(crate) async fn confirm_already_current(path: &Path) -> local::Result<Value> {
        let ledger = Self::open(path).await?;
        ledger.close().await;
        Ok(json!({"status":"already_current","from_schema":10,"to_schema":10}))
    }
    pub async fn permission_status(&self, customer: &str, source: &str) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let setup = service::permissions::effective_for(&snapshot, customer, source)?;
        let initial = service::Setup::parse(&snapshot.setup)?;
        let mut changes = Vec::new();
        if customer == initial.customer && source == initial.source {
            changes.extend(
                snapshot
                    .permissions
                    .iter()
                    .map(|row| serde_json::from_slice::<Value>(row).expect("validated permission")),
            );
        }
        changes.extend(
            snapshot
                .scoped_permissions
                .iter()
                .filter(|row| row.customer == customer && row.source == source)
                .map(|row| {
                    serde_json::from_slice::<Value>(&row.canonical_bytes)
                        .expect("validated scoped permission")
                }),
        );
        tx.rollback().await.map_err(store_error)?;
        Ok(
            json!({"schema":"ledger-billing-permission-status/2","status":"ok","customer":customer,"source":source,"revision":setup.grant_revision.to_string(),"permissions":setup.permissions,"changes":changes}),
        )
    }
    /// Local filesystem administrator control, even when all event rights are revoked.
    pub async fn permissions(
        &self,
        customer: &str,
        source: &str,
        raw: &[u8],
    ) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let at = local::now()?;
        let (result, plan) =
            service::permissions::prepare_scoped(&snapshot, customer, source, raw, &at)?;
        if let Some(plan) = plan {
            tx.billing_m2_permissions(&plan)
                .await
                .map_err(store_error)?;
            match tx.commit().await {
                Ok(()) => Ok(result),
                Err(CommitError::RolledBack(e)) => Err(store_error(e).into()),
                Err(CommitError::OutcomeUnknown) => {
                    Err(service::reject("BILLING_CONTROL_OUTCOME_UNKNOWN").into())
                }
            }
        } else {
            tx.rollback().await.map_err(store_error)?;
            Ok(result)
        }
    }
    /// Agreement registration, amendment, end, and restart are local admin controls.
    pub async fn agreement_control(
        &self,
        customer: &str,
        source: &str,
        raw: &[u8],
    ) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let at = local::now()?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let (result, plan) = service::control::prepare(&snapshot, raw, &at, customer, source)?;
        if let Some(plan) = plan {
            tx.billing_m2_agreement(&plan).await.map_err(store_error)?;
            match tx.commit().await {
                Ok(()) => Ok(result),
                Err(CommitError::RolledBack(e)) => Err(store_error(e).into()),
                Err(CommitError::OutcomeUnknown) => {
                    Err(service::reject("BILLING_CONTROL_OUTCOME_UNKNOWN").into())
                }
            }
        } else {
            tx.rollback().await.map_err(store_error)?;
            Ok(result)
        }
    }
    pub async fn accept(&self, customer: &str, source: &str, raw: &[u8]) -> local::Result<Value> {
        self.submit(customer, source, raw, None).await
    }
    pub async fn outcome(&self, customer: &str, source: &str, raw: &[u8]) -> local::Result<Value> {
        self.submit(customer, source, raw, Some(false)).await
    }
    pub async fn correct(&self, customer: &str, source: &str, raw: &[u8]) -> local::Result<Value> {
        self.submit(customer, source, raw, Some(true)).await
    }
    /// Decision deadlines. A write verifies the retained-history meter, so its
    /// guard work grows with retained history even though it does not decode
    /// every receipt. A complete statement or explanation also scans that
    /// history and gets the same report budget. Neither path ever truncates: a
    /// report that cannot finish inside its budget fails rather than returning
    /// a partial ledger.
    const WRITE_BUDGET: Duration = Duration::from_secs(300);
    const REPORT_BUDGET: Duration = Duration::from_secs(300);
    async fn submit(
        &self,
        customer: &str,
        source: &str,
        raw: &[u8],
        correction: Option<bool>,
    ) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        let at = local::now()?;
        // Metadata only. The coordinator reads no retained bundle until the
        // service has proved this submission is not already retained.
        let meta = tx.billing_meta().await.map_err(store_error)?;
        let (result, plan) = if let Some(correction) = correction {
            let submission =
                service::adjustment::begin(&meta, customer, source, raw, &at, correction)?;
            let dedup = self
                .resolve_dedup(
                    &mut tx,
                    submission.customer(),
                    submission.source(),
                    submission.external_id(),
                    submission.semantic_key(),
                )
                .await?;
            let target = self
                .target_history(
                    &mut tx,
                    submission.customer(),
                    submission.source(),
                    submission.target(),
                )
                .await?;
            service::adjustment::finish(&meta, &submission, &dedup, &target)?
        } else {
            let submission = service::begin(&meta, customer, source, raw, &at)?;
            let dedup = self
                .resolve_dedup(
                    &mut tx,
                    submission.customer(),
                    submission.source(),
                    submission.external_id(),
                    submission.semantic_key(),
                )
                .await?;
            service::finish(&meta, &submission, &dedup)?
        };
        if let Some(plan) = plan {
            tx.append_billing_m3(&plan).await.map_err(store_error)?;
            match tx.commit().await {
                Ok(()) => (),
                Err(CommitError::RolledBack(e)) => return Err(store_error(e).into()),
                Err(CommitError::OutcomeUnknown) => {
                    let scope = service::control::customer_scope(&meta, customer)?;
                    return Err(ServiceError::OutcomeUnknown {
                        scope: [scope.tenant().into(), scope.environment().into()],
                        source: plan.source().into(),
                        external_id: plan.external_id().into(),
                    }
                    .into());
                }
            }
        } else {
            tx.rollback().await.map_err(store_error)?;
        }
        Ok(result)
    }
    /// Exact retained identity and semantic matches for one submission. The
    /// coordinator only fetches rows; the service compares them and decides
    /// whether the submission is a duplicate or a conflict.
    async fn resolve_dedup(
        &self,
        tx: &mut SqliteTx,
        customer: &str,
        source: &str,
        external: &str,
        semantic: &[u8],
    ) -> local::Result<service::Dedup> {
        let mut dedup = service::Dedup::empty();
        if let Some(hit) = tx
            .billing_identity_lookup(customer, source, external)
            .await
            .map_err(store_error)?
        {
            // Identity is decided first on every retained tier, alias rows
            // included. An alias row stores the ordinal of the entry it aliases,
            // so both tiers resolve to the entry that produced the original
            // receipt, and the matched row's own retained ingress is the
            // comparison: the alias's ingress for an alias. An exact alias
            // replay is therefore a duplicate with the original receipt, and
            // changed ingress under that identity refuses with
            // IDENTITY_CONFLICT instead of reserving a second alias. A new
            // delivery id with the same semantic key never reaches here: it has
            // no retained identity, so the semantic lookup below still books its
            // one alias.
            let entry = tx.billing_entry(hit.ordinal).await.map_err(store_error)?;
            let target = tx
                .billing_index_target(hit.ordinal)
                .await
                .map_err(store_error)?;
            let target_entries = self.target_history(tx, customer, source, &target).await?;
            dedup.identity = Some(service::DedupHit::entry(
                entry,
                external.to_owned(),
                hit.ingress,
                hit.facts,
                target_entries,
            ));
            return Ok(dedup);
        }
        if let Some(hit) = tx
            .billing_semantic_lookup(customer, source, semantic)
            .await
            .map_err(store_error)?
        {
            let entry = tx.billing_entry(hit.ordinal).await.map_err(store_error)?;
            let target = tx
                .billing_index_target(hit.ordinal)
                .await
                .map_err(store_error)?;
            let target_entries = self.target_history(tx, customer, source, &target).await?;
            // A semantic hit is compared on its facts only, so it carries no
            // ingress of its own.
            dedup.semantic = Some(service::DedupHit::entry(
                entry,
                external.to_owned(),
                vec![],
                hit.facts,
                target_entries,
            ));
        }
        Ok(dedup)
    }
    /// The retained decisions of exactly one target, selected by the durable
    /// index, for the economic replay an outcome or correction depends on.
    async fn target_history(
        &self,
        tx: &mut SqliteTx,
        customer: &str,
        source: &str,
        target: &str,
    ) -> local::Result<Vec<BillingEntry>> {
        let entries = tx
            .billing_target_entries(customer, source, target)
            .await
            .map_err(store_error)?;
        Ok(entries)
    }
    pub async fn statement(&self, customer: &str, target: Option<&str>) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Self::REPORT_BUDGET)
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let result = service::statement(&snapshot, customer, target)?;
        tx.rollback().await.map_err(store_error)?;
        Ok(result)
    }
    pub async fn explain(&self, customer: &str, target: &str) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Self::REPORT_BUDGET)
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let result = service::statement(&snapshot, customer, Some(target))?;
        tx.rollback().await.map_err(store_error)?;
        Ok(result)
    }
    pub async fn close(self) {
        self.store.close().await;
    }
}

#[cfg(test)]
mod tests {
    use super::BillingLedger;

    #[test]
    fn setup_summary_validates_terms_without_exposing_evidence_text() {
        let raw = include_bytes!("../../../examples/integration/setup-synthetic.json");
        let summary = BillingLedger::setup_summary(raw).unwrap();
        assert_eq!(summary["customer"], "synthetic-customer");
        assert_eq!(summary["price_usd"], "0.02");
        assert_eq!(summary["outcome_codes"][0]["atoms"], "98");
        assert_eq!(summary["outcome_codes"][1]["atoms"], "-2");
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("No real customer assent"));
        assert!(!serialized.contains("No real authority attested"));
    }
}
