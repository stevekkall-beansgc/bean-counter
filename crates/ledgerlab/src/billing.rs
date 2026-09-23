//! Local filesystem-authorized ordinary SQLite billing. Not a remote API.
use crate::{
    local::{self, LocalError},
    service::{billing as service, store_error},
    store::{
        errors::CommitError,
        ports::{AcceptanceStore, AcceptanceTx},
        records::Installation,
        sqlite::SqliteStore,
    },
    ServiceError,
};
use serde_json::{json, Value};
use std::{fs, path::Path, time::Duration};
use tokio::time::Instant;

/// Version of the local billing facade and JSON contract family.
/// Frozen economic record profiles have their own independent versions.
pub const CONTRACT_VERSION: &str = "v0.2";

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
    pub async fn permission_status(&self) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let setup = service::permissions::effective(&snapshot)?;
        tx.rollback().await.map_err(store_error)?;
        Ok(
            json!({"status":"ok","revision":setup.grant_revision.to_string(),"permissions":setup.permissions,"changes":snapshot.permissions.iter().map(|r|serde_json::from_slice::<Value>(r).expect("validated permission")).collect::<Vec<_>>()}),
        )
    }
    /// Local filesystem administrator control, even when all event rights are revoked.
    pub async fn permissions(&self, raw: &[u8]) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let (result, plan) = service::permissions::prepare(&snapshot, raw)?;
        tx.billing_permissions(&plan).await.map_err(store_error)?;
        match tx.commit().await {
            Ok(()) => Ok(result),
            Err(CommitError::RolledBack(e)) => Err(store_error(e).into()),
            Err(CommitError::OutcomeUnknown) => {
                Err(service::reject("BILLING_CONTROL_OUTCOME_UNKNOWN").into())
            }
        }
    }
    pub async fn accept(&self, raw: &[u8]) -> local::Result<Value> {
        self.submit(raw, None).await
    }
    pub async fn outcome(&self, raw: &[u8]) -> local::Result<Value> {
        self.submit(raw, Some(false)).await
    }
    pub async fn correct(&self, raw: &[u8]) -> local::Result<Value> {
        self.submit(raw, Some(true)).await
    }
    async fn submit(&self, raw: &[u8], correction: Option<bool>) -> local::Result<Value> {
        let at = local::now()?;
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let (result, plan) = if let Some(correction) = correction {
            service::adjustment::prepare(&snapshot, raw, &at, correction)?
        } else {
            service::prepare(&snapshot, raw, &at)?
        };
        if let Some(plan) = plan {
            tx.append_billing(&plan).await.map_err(store_error)?;
            match tx.commit().await {
                Ok(()) => (),
                Err(CommitError::RolledBack(e)) => return Err(store_error(e).into()),
                Err(CommitError::OutcomeUnknown) => {
                    let setup = service::Setup::parse(&snapshot.setup)?;
                    return Err(ServiceError::OutcomeUnknown {
                        scope: [
                            setup.scope.tenant().into(),
                            setup.scope.environment().into(),
                        ],
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
    pub async fn statement(&self, customer: &str, target: Option<&str>) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let result = service::statement(&snapshot, customer, target)?;
        tx.rollback().await.map_err(store_error)?;
        Ok(result)
    }
    pub async fn explain(&self, target: &str) -> local::Result<Value> {
        let mut tx = self
            .store
            .begin(Instant::now() + Duration::from_secs(5))
            .await
            .map_err(store_error)?;
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        let setup = service::Setup::parse(&snapshot.setup)?;
        let result = service::statement(&snapshot, &setup.customer, Some(target))?;
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
