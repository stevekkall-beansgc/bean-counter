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
mod term;

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
        let mut summary = json!({
            "profile": "local-retail",
            "schema": setup.schema,
            "scope": setup.scope,
            "store_id": setup.store_id,
            "source": setup.source,
            "customer": setup.customer,
            "host": setup.host,
            "agreement": setup.agreement,
            "binding": setup.binding,
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
        });
        if setup.schema == "ledger-local-billing/2" {
            summary["rate_usd_per_unit"] = json!(setup.price);
            summary["unit"] = json!(setup.unit);
            summary["maximum_quantity"] = json!(setup.maximum_quantity);
            summary["currency_scale"] = json!(18);
        } else {
            summary["price_usd"] = json!(setup.price);
        }
        Ok(summary)
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

        // A fully validated schema-11 open proves an earlier M5 upgrade
        // committed. A schema-10 open is valid M3 state but still needs the
        // explicit M5 transition.
        let open_error = match Self::open(&path).await {
            Ok(ledger) => {
                ledger.close().await;
                let store_id = config["store_id"]
                    .as_str()
                    .ok_or(LocalError::Config("invalid billing installation"))?;
                let seed = crate::store::sqlite::migrate::preflight_m5(&data, store_id)
                    .await
                    .map_err(|error| match error {
                        crate::maintenance::UpgradeError::Refused => {
                            service::reject("BILLING_UPGRADE_REFUSED")
                        }
                        crate::maintenance::UpgradeError::OutcomeUnknown => {
                            ServiceError::Unavailable
                        }
                    })?;
                if seed.source_version == 11 {
                    return Ok(json!({"status":"already_current","from_schema":11,"to_schema":11}));
                }
                service::require(seed.source_version == 10, "BILLING_UPGRADE_REFUSED")?;
                let result = crate::store::sqlite::migrate::upgrade_m5(&data, &seed)
                    .await
                    .map_err(|error| match error {
                        crate::maintenance::UpgradeError::Refused => {
                            service::reject("BILLING_UPGRADE_REFUSED")
                        }
                        crate::maintenance::UpgradeError::OutcomeUnknown => {
                            service::reject("BILLING_UPGRADE_OUTCOME_UNKNOWN")
                        }
                    })?;
                return match result {
                    crate::maintenance::UpgradeResult::Upgraded => {
                        Ok(json!({"status":"upgraded","from_schema":10,"to_schema":11}))
                    }
                    crate::maintenance::UpgradeResult::AlreadyCurrent => {
                        Self::confirm_already_current(&path).await
                    }
                };
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
    /// Reopen through the full billing validator before claiming that an
    /// earlier M2/M3 or M5 upgrade is current. The storage migration's
    /// structural reconciliation alone does not validate billing invariants.
    pub(crate) async fn confirm_already_current(path: &Path) -> local::Result<Value> {
        let ledger = Self::open(path).await?;
        ledger.close().await;
        let data = path.join(".ledger");
        let raw = local::read_file(&path.join("billing.json"), local::CONFIG_LIMIT)?;
        let config = ledgerlab_core::canonical::parse(&raw)
            .map_err(|_| LocalError::Config("invalid billing.json"))?;
        let store_id = config["store_id"]
            .as_str()
            .ok_or(LocalError::Config("invalid billing installation"))?;
        let seed = crate::store::sqlite::migrate::preflight_m5(&data, store_id)
            .await
            .map_err(|error| match error {
                crate::maintenance::UpgradeError::Refused => {
                    service::reject("BILLING_UPGRADE_REFUSED")
                }
                crate::maintenance::UpgradeError::OutcomeUnknown => ServiceError::Unavailable,
            })?;
        if seed.source_version == 11 {
            Ok(json!({"status":"already_current","from_schema":11,"to_schema":11}))
        } else {
            Ok(json!({"status":"already_current","from_schema":10,"to_schema":10}))
        }
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
    use serde_json::json;
    use sqlx::Connection;

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

    #[tokio::test]
    async fn first_term_commits_two_children_and_replays_exact_response() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        let raw_setup = include_bytes!("../../../examples/billing/setup.json");
        BillingLedger::init(&path, raw_setup).await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let request = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"first-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = serde_json::to_vec_pretty(&request).unwrap();
        let first = ledger.term_set(&bytes).await.unwrap();
        assert_eq!(first["status"], "term_updated");
        assert_eq!(first["receipt"]["record_ids"].as_array().unwrap().len(), 2);
        assert_eq!(ledger.term_set(&bytes).await.unwrap(), first);
        let same_semantics_different_bytes =
            ledgerlab_core::canonical::CanonicalBytes::from_value(&request)
                .unwrap()
                .into_vec();
        assert!(
            matches!(ledger.term_set(&same_semantics_different_bytes).await,
            Err(super::LocalError::Service(super::ServiceError::Rejection(code))) if code == "IDENTITY_CONFLICT")
        );
        let mut changed = request;
        changed["term"]["interval"] = json!(2);
        let changed = ledgerlab_core::canonical::CanonicalBytes::from_value(&changed)
            .unwrap()
            .into_vec();
        assert!(ledger.term_set(&changed).await.is_err());
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        assert_eq!(reopened.term_set(&bytes).await.unwrap(), first);
        reopened.close().await;
    }

    #[tokio::test]
    async fn active_term_assigns_new_m3_history_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"active-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let term = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&term).await.unwrap();
        let accepted = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                include_bytes!("../../../examples/billing/event.json"),
            )
            .await
            .unwrap();
        let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
        let mut outcome: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/outcome.json"))
                .unwrap();
        outcome["target"] = json!(target);
        let outcome = ledgerlab_core::canonical::CanonicalBytes::from_value(&outcome)
            .unwrap()
            .into_vec();
        ledger
            .outcome("customer-1", "urn:example:work", &outcome)
            .await
            .unwrap();
        let mut correction: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/correction.json"))
                .unwrap();
        correction["target"] = json!(target);
        let correction = ledgerlab_core::canonical::CanonicalBytes::from_value(&correction)
            .unwrap()
            .into_vec();
        ledger
            .correct("customer-1", "urn:example:work", &correction)
            .await
            .unwrap();
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let rows: Vec<(i64, i64, String)> = sqlx::query_as(
            "SELECT term_version,period_index,assignment_basis FROM billing_m5_assignments WHERE source_stream='m3' ORDER BY source_sequence")
            .fetch_all(&mut conn).await.unwrap();
        assert_eq!(
            rows,
            vec![
                (1, 0, "acceptance-time".into()),
                (1, 0, "acceptance-time".into()),
                (1, 0, "linked-open-period".into())
            ]
        );
        let counts: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM billing_m3_index), (SELECT count(*) FROM billing_m5_records), (SELECT count(*) FROM billing_m5_snapshot_boundaries)")
            .fetch_one(&mut conn).await.unwrap();
        assert_eq!(counts, (3, 2, 5));
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn backfilled_prior_tier_outcome_supports_later_correction() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let accepted = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                include_bytes!("../../../examples/billing/event.json"),
            )
            .await
            .unwrap();
        let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
        let mut outcome: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/outcome.json"))
                .unwrap();
        outcome["target"] = json!(target);
        let outcome = ledgerlab_core::canonical::CanonicalBytes::from_value(&outcome)
            .unwrap()
            .into_vec();
        ledger
            .outcome("customer-1", "urn:example:work", &outcome)
            .await
            .unwrap();
        ledger.close().await;
        // Place the two already validated retained rows in the frozen M2 tier.
        // Their global M3 index ordinals and exact canonical bytes stay intact.
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        sqlx::query("INSERT INTO billing_m2_entries SELECT * FROM billing_m3_entries")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("DROP TRIGGER billing_m3_entries_immutable_delete")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("DELETE FROM billing_m3_entries")
            .execute(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"prior-tier","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&bytes).await.unwrap();
        let mut correction: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/correction.json"))
                .unwrap();
        correction["target"] = json!(target);
        let correction = ledgerlab_core::canonical::CanonicalBytes::from_value(&correction)
            .unwrap()
            .into_vec();
        ledger
            .correct("customer-1", "urn:example:work", &correction)
            .await
            .unwrap();
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        reopened.close().await;
    }

    #[tokio::test]
    async fn reopen_refuses_unreconciled_close_projection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"phantom-close","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&bytes).await.unwrap();
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let boundary_id: i64 =
            sqlx::query_scalar("SELECT max(boundary_id) FROM billing_m5_snapshot_boundaries")
                .fetch_one(&mut conn)
                .await
                .unwrap();
        sqlx::query("INSERT INTO billing_m5_period_closes(customer,term_version,period_index,boundary_resolution_id,snapshot_boundary_id,m3_high_water,m5_high_water,statement_hash,close_sequence,statement_bytes) VALUES('customer-1',1,0,'resolution-1-0',?,0,2,?,2,x'7b7d')")
            .bind(boundary_id).bind("0".repeat(64)).execute(&mut conn).await.unwrap();
        conn.close().await.unwrap();
        assert!(BillingLedger::open(&path).await.is_err());
    }

    #[tokio::test]
    async fn reopen_refuses_post_activation_correction_relabelled_acceptance_time() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"basis-test","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&bytes).await.unwrap();
        let accepted = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                include_bytes!("../../../examples/billing/event.json"),
            )
            .await
            .unwrap();
        let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
        let mut outcome: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/outcome.json"))
                .unwrap();
        outcome["target"] = json!(target);
        let outcome = ledgerlab_core::canonical::CanonicalBytes::from_value(&outcome)
            .unwrap()
            .into_vec();
        ledger
            .outcome("customer-1", "urn:example:work", &outcome)
            .await
            .unwrap();
        let mut correction: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/correction.json"))
                .unwrap();
        correction["target"] = json!(target);
        let correction = ledgerlab_core::canonical::CanonicalBytes::from_value(&correction)
            .unwrap()
            .into_vec();
        ledger
            .correct("customer-1", "urn:example:work", &correction)
            .await
            .unwrap();
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        sqlx::query("DROP TRIGGER billing_m5_assignments_no_update")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("UPDATE billing_m5_assignments SET assignment_basis='acceptance-time' WHERE source_stream='m3' AND source_sequence=3")
            .execute(&mut conn).await.unwrap();
        conn.close().await.unwrap();
        assert!(BillingLedger::open(&path).await.is_err());
    }

    #[tokio::test]
    async fn pre_activation_correction_backfill_keeps_acceptance_time_basis() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let accepted = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                include_bytes!("../../../examples/billing/event.json"),
            )
            .await
            .unwrap();
        let target = accepted["receipt"]["body"]["target"].as_str().unwrap();
        let mut outcome: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/outcome.json"))
                .unwrap();
        outcome["target"] = json!(target);
        let outcome = ledgerlab_core::canonical::CanonicalBytes::from_value(&outcome)
            .unwrap()
            .into_vec();
        ledger
            .outcome("customer-1", "urn:example:work", &outcome)
            .await
            .unwrap();
        let mut correction: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/correction.json"))
                .unwrap();
        correction["target"] = json!(target);
        let correction = ledgerlab_core::canonical::CanonicalBytes::from_value(&correction)
            .unwrap()
            .into_vec();
        ledger
            .correct("customer-1", "urn:example:work", &correction)
            .await
            .unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"backfill-correction","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&bytes).await.unwrap();
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let basis: String = sqlx::query_scalar("SELECT assignment_basis FROM billing_m5_assignments WHERE source_stream='m3' AND source_sequence=3")
            .fetch_one(&mut conn).await.unwrap();
        assert_eq!(basis, "acceptance-time");
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn term_end_outside_timestamp_range_refuses_before_append() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"max-year","expected_revision":"0",
            "effective":{"mode":"initial","at":"9999-12-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"9999-12-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        assert!(matches!(ledger.term_set(&bytes).await,
            Err(super::LocalError::Service(super::ServiceError::Rejection(code))) if code == "BILLING_M5_BOUNDS"));
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        reopened.close().await;
    }

    #[tokio::test]
    async fn future_initial_term_refuses_pre_effective_m3_write_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"future-term","expected_revision":"0",
            "effective":{"mode":"initial","at":"9999-01-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"9999-01-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&bytes).await.unwrap();
        assert!(matches!(ledger.accept("customer-1", "urn:example:work",
            include_bytes!("../../../examples/billing/event.json")).await,
            Err(super::LocalError::Service(super::ServiceError::Rejection(code))) if code == "BILLING_M5_PERIOD"));
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let counts: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM billing_m3_index),(SELECT count(*) FROM billing_m5_assignments)")
            .fetch_one(&mut conn).await.unwrap();
        assert_eq!(counts, (0, 0));
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn reopen_refuses_phantom_m5_source_projections() {
        for mutation in [
            "INSERT INTO billing_m5_assignments(customer,source_scope,source_record_kind,source_record_id,source_stream,source_sequence,term_version,period_index,assignment_basis,assignment_at_us) VALUES('customer-1','urn:example:work','activity','phantom','m5',999,1,0,'acceptance-time',0)",
            "INSERT INTO billing_m5_adjustments(customer,source_scope,adjustment_id,cause_kind,cause_id,target_id,original_term_version,original_period_index,assigned_term_version,assigned_period_index,source_stream,source_sequence,signed_delta_atoms) VALUES('customer-1','urn:example:work','phantom','outcome-correction','phantom','target',1,0,1,0,'m5',999,'0')",
        ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"phantom-test","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        ledger.term_set(&bytes).await.unwrap();
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        sqlx::query(mutation)
            .execute(&mut conn).await.unwrap();
        conn.close().await.unwrap();
        assert!(BillingLedger::open(&path).await.is_err());
        }
    }

    #[tokio::test]
    async fn initial_term_after_retained_acceptance_refuses_without_m5_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let accepted = ledger
            .accept(
                "customer-1",
                "urn:example:work",
                include_bytes!("../../../examples/billing/event.json"),
            )
            .await
            .unwrap();
        let request = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"too-late","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-10-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-10-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let bytes = ledgerlab_core::canonical::CanonicalBytes::from_value(&request)
            .unwrap()
            .into_vec();
        assert!(matches!(ledger.term_set(&bytes).await,
            Err(super::LocalError::Service(super::ServiceError::Rejection(code))) if code == "BILLING_M5_PERIOD"));
        ledger.close().await;
        let reopened = BillingLedger::open(&path).await.unwrap();
        // Refusal did not reserve the identity or advance the term revision.
        let mut fixed = request;
        fixed["effective"]["at"] = json!("2026-09-01T00:00:00.000000Z");
        let fixed = ledgerlab_core::canonical::CanonicalBytes::from_value(&fixed)
            .unwrap()
            .into_vec();
        assert_eq!(reopened.term_set(&fixed).await.unwrap()["revision"], "1");
        reopened.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let assigned: (String, String, i64, i64) = sqlx::query_as(
            "SELECT source_record_kind,source_record_id,term_version,period_index FROM billing_m5_assignments WHERE source_stream='m3' AND source_sequence=1",
        )
        .fetch_one(&mut conn)
        .await
        .unwrap();
        assert_eq!(assigned.0, "base-acceptance");
        assert_eq!(assigned.1, accepted["receipt"]["id"]);
        assert_eq!((assigned.2, assigned.3), (1, 0));
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn two_customers_get_distinct_term_and_resolution_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        let setup: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../examples/billing/setup.json")).unwrap();
        BillingLedger::init(
            &path,
            include_bytes!("../../../examples/billing/setup.json"),
        )
        .await
        .unwrap();
        let ledger = BillingLedger::open(&path).await.unwrap();
        let mut second_setup = setup;
        second_setup["customer"] = json!("customer-2");
        second_setup["agreement"] = json!("agreement-2");
        let registration = json!({
            "schema":"ledger-billing-registration/2","customer":"customer-2",
            "source":"urn:example:work","change_id":"register-2",
            "expected_revision":"0","effective_at":"2026-09-01T00:00:00.000000Z",
            "setup":second_setup
        });
        let registration = ledgerlab_core::canonical::CanonicalBytes::from_value(&registration)
            .unwrap()
            .into_vec();
        ledger
            .agreement_control("customer-2", "urn:example:work", &registration)
            .await
            .unwrap();
        let mut term = json!({
            "schema":"ledger-billing-term/1","customer":"customer-1",
            "change_id":"same-change","expected_revision":"0",
            "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
            "term":{"interval":1,"unit":"month","alignment":"anchored",
                "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                "timezone_rules_version":"IANA-2025b","proration":"none"}
        });
        let first_request = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        let first = ledger.term_set(&first_request).await.unwrap();
        term["customer"] = json!("customer-2");
        let second_request = ledgerlab_core::canonical::CanonicalBytes::from_value(&term)
            .unwrap()
            .into_vec();
        let second = ledger.term_set(&second_request).await.unwrap();
        assert_eq!(first["revision"], "1");
        assert_eq!(second["revision"], "1");
        assert_eq!(first["term_version"], "1");
        assert_eq!(second["term_version"], "2");
        assert_eq!(ledger.term_set(&first_request).await.unwrap(), first);
        assert_eq!(ledger.term_set(&second_request).await.unwrap(), second);
        ledger.close().await;
        let mut conn = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(path.join(".ledger/local.db"))
                .create_if_missing(false),
        )
        .await
        .unwrap();
        let resolutions: Vec<(String,String)> = sqlx::query_as(
            "SELECT customer,resolution_id FROM billing_m5_period_resolutions ORDER BY term_version"
        ).fetch_all(&mut conn).await.unwrap();
        assert_eq!(
            resolutions,
            vec![
                ("customer-1".into(), "resolution-1-0".into()),
                ("customer-2".into(), "resolution-2-0".into())
            ]
        );
        conn.close().await.unwrap();
    }

    #[tokio::test]
    async fn reopen_refuses_deleted_or_changed_initial_term_projections() {
        let mutations = [
            (
                "DROP TRIGGER billing_m5_term_versions_no_delete",
                "DELETE FROM billing_m5_term_versions",
            ),
            (
                "DROP TRIGGER billing_m5_period_resolutions_no_delete",
                "DELETE FROM billing_m5_period_resolutions",
            ),
            (
                "DROP TRIGGER billing_m5_assignments_no_delete",
                "DELETE FROM billing_m5_assignments",
            ),
            (
                "DROP TRIGGER billing_m5_assignments_no_update",
                "UPDATE billing_m5_assignments SET period_index=1",
            ),
            (
                "DROP TRIGGER billing_m5_assignments_no_update",
                "UPDATE billing_m5_assignments SET source_record_id='forged'",
            ),
        ];
        for (trigger, mutation) in mutations {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().canonicalize().unwrap().join("billing");
            BillingLedger::init(
                &path,
                include_bytes!("../../../examples/billing/setup.json"),
            )
            .await
            .unwrap();
            let ledger = BillingLedger::open(&path).await.unwrap();
            ledger
                .accept(
                    "customer-1",
                    "urn:example:work",
                    include_bytes!("../../../examples/billing/event.json"),
                )
                .await
                .unwrap();
            let request = json!({
                "schema":"ledger-billing-term/1","customer":"customer-1",
                "change_id":"first","expected_revision":"0",
                "effective":{"mode":"initial","at":"2026-09-01T00:00:00.000000Z"},
                "term":{"interval":1,"unit":"month","alignment":"anchored",
                    "anchor":{"date":"2026-09-01","time":"00:00:00"},"timezone":"UTC",
                    "month_end_rule":"preserve_anchor_and_clamp","boundary_rule_version":"billing-boundary/1",
                    "timezone_rules_version":"IANA-2025b","proration":"none"}
            });
            let request = ledgerlab_core::canonical::CanonicalBytes::from_value(&request)
                .unwrap()
                .into_vec();
            ledger.term_set(&request).await.unwrap();
            ledger.close().await;
            let mut conn = sqlx::SqliteConnection::connect_with(
                &sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(path.join(".ledger/local.db"))
                    .create_if_missing(false),
            )
            .await
            .unwrap();
            sqlx::query(trigger).execute(&mut conn).await.unwrap();
            sqlx::query(mutation).execute(&mut conn).await.unwrap();
            conn.close().await.unwrap();
            assert!(BillingLedger::open(&path).await.is_err(), "{mutation}");
        }
    }
}
