//! Trusted installation resources. These settings are never commercial input.
use super::*;
use crate::{
    local as io,
    store::{
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::SqliteTx,
    },
};
use ledgerlab_core::{
    canonical::{self, audit_arena::AuditArena},
    validation_work::ResourceLimits,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tokio::time::Instant;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Settings {
    pub schema: String,
    pub semantic_work: Option<u64>,
    pub compile_work: Option<u64>,
    pub resolved_records: Option<usize>,
    pub resolved_bytes: Option<usize>,
    pub new_body_bytes: Option<usize>,
    pub arena_bytes: Option<usize>,
    pub retained_operations: Option<i64>,
    pub retained_bytes: Option<i64>,
    pub attempt_timeout_ms: Option<u64>,
}
impl Settings {
    fn parse(v: &Value) -> Result<Self> {
        const FIELDS: &[&str] = &[
            "schema",
            "semantic_work",
            "compile_work",
            "resolved_records",
            "resolved_bytes",
            "new_body_bytes",
            "arena_bytes",
            "retained_operations",
            "retained_bytes",
            "attempt_timeout_ms",
        ];
        let o = v.as_object().ok_or_else(|| reject("INVALID_SHAPE"))?;
        require(
            o.len() == FIELDS.len() && FIELDS.iter().all(|k| o.contains_key(*k)),
            "INVALID_SHAPE",
        )?;
        for k in &FIELDS[1..] {
            let v = &o[*k];
            require(
                v.is_null()
                    || v.as_u64()
                        .is_some_and(|n| n > 0 && n <= 9_007_199_254_740_991),
                "INVALID_SCALAR",
            )?;
        }
        let s: Self = serde_json::from_value(v.clone()).map_err(|_| reject("INVALID_SCALAR"))?;
        require(
            s.schema == "ledger-local-resource-settings/1",
            "UNSUPPORTED_SCHEMA_PROFILE",
        )?;
        s.core().validate().map_err(core_error)?;
        if let Some(ms) = s.attempt_timeout_ms {
            require(
                Instant::now()
                    .checked_add(std::time::Duration::from_millis(ms))
                    .is_some(),
                "INVALID_SCALAR",
            )?;
        }
        Ok(s)
    }
    fn core(&self) -> ResourceLimits {
        ResourceLimits {
            semantic_work: self.semantic_work,
            compile_work: self.compile_work,
            resolved_records: self.resolved_records,
            resolved_bytes: self.resolved_bytes,
            new_body_bytes: self.new_body_bytes,
            arena_bytes: self.arena_bytes,
        }
    }
    fn value(&self) -> Value {
        serde_json::to_value(self).expect("resource settings serialize")
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}
fn identity(path: &Path) -> Result<FileIdentity> {
    io::private_existing(path, false).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
    let m = fs::symlink_metadata(path).map_err(|_| ServiceError::Unavailable)?;
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(FileIdentity {
        len: m.len(),
        modified: m.modified().ok(),
        #[cfg(unix)]
        device: m.dev(),
        #[cfg(unix)]
        inode: m.ino(),
    })
}
#[derive(Clone)]
pub(crate) struct Snapshot {
    path: PathBuf,
    raw: Vec<u8>,
    identity: FileIdentity,
    pub settings: Option<Settings>,
    at: Instant,
}
impl Snapshot {
    pub(crate) fn capture(path: &Path, store: &str, scope: &[String; 2]) -> Result<Self> {
        let before = identity(path)?;
        let raw =
            io::read_file(path, io::CONFIG_LIMIT).map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        let v = canonical::parse(&raw).map_err(|_| reject("INVALID_SHAPE"))?;
        let o = v.as_object().ok_or_else(|| reject("INVALID_SHAPE"))?;
        require(
            o.len() == 3 + usize::from(o.contains_key("resources"))
                && ["schema", "store_id", "scope"]
                    .iter()
                    .all(|k| o.contains_key(*k)),
            "INVALID_SHAPE",
        )?;
        require(
            v["schema"] == "ledger-billing-installation/1"
                && v["store_id"] == store
                && v["scope"] == json!(scope),
            "AUTHORITY_REQUIRED",
        )?;
        let settings = v.get("resources").map(Settings::parse).transpose()?;
        require(identity(path)? == before, "STALE_REVISION")?;
        Ok(Self {
            path: path.into(),
            raw,
            identity: before,
            settings,
            at: Instant::now(),
        })
    }
    pub(crate) fn recheck(&self) -> Result<()> {
        require(identity(&self.path)? == self.identity, "STALE_REVISION")?;
        let now = io::read_file(&self.path, io::CONFIG_LIMIT)
            .map_err(|_| reject("AUTHORITY_REQUIRED"))?;
        require(now == self.raw, "STALE_REVISION")
    }
    pub(crate) fn timeout_error(&self) -> Option<crate::store::errors::StoreError> {
        let limit = self.settings.as_ref()?.attempt_timeout_ms?;
        if Instant::now() < self.deadline().ok()?? {
            return None;
        }
        Some(crate::store::errors::StoreError::ResourceQuota {
            setting: "attempt_timeout_ms",
            limit,
            observed: self.at.elapsed().as_millis().min(u64::MAX as u128) as u64,
        })
    }
    pub(crate) fn core_arena(&self) -> ledgerlab_core::Result<AuditArena> {
        match &self.settings {
            Some(s) => AuditArena::with_resource_limits(s.core()),
            None => AuditArena::new(),
        }
    }
    pub(crate) fn arena(&self) -> Result<AuditArena> {
        self.core_arena().map_err(core_error)
    }
    pub(crate) fn deadline(&self) -> Result<Option<Instant>> {
        let ms = self
            .settings
            .as_ref()
            .map(|s| s.attempt_timeout_ms)
            .unwrap_or(Some(20_000));
        ms.map(|n| {
            self.at
                .checked_add(std::time::Duration::from_millis(n))
                .ok_or_else(|| reject("INVALID_SCALAR"))
        })
        .transpose()
    }
    pub(crate) async fn begin(
        &self,
        store: &crate::store::sqlite::SqliteStore,
        s: &mut ledgerlab_core::validation_work::OriginalAuditSession,
    ) -> Result<SqliteTx> {
        let mut tx = if self.settings.is_some() {
            store.begin_with_operator_session(self.deadline()?, s).await
        } else {
            store
                .begin_with_resource_session(self.deadline()?.expect("legacy deadline"), s)
                .await
        }
        .map_err(crate::service::workflow::agreement::financial::management_read_error)?;
        if let Err(e) = self.bind(&mut tx).await {
            tx.rollback().await.map_err(crate::service::store_error)?;
            return Err(e);
        }
        Ok(tx)
    }
    pub(crate) async fn bind(&self, tx: &mut SqliteTx) -> Result<()> {
        self.recheck()?;
        if self.settings.is_some() {
            require(
                tx.billing_schema_version()
                    .await
                    .map_err(crate::service::store_error)?
                    == 17,
                "UNSUPPORTED_SCHEMA_PROFILE",
            )?;
        }
        if let Some(old) = &tx.workflow_snapshot {
            require(
                old.raw == self.raw && old.identity == self.identity,
                "STALE_REVISION",
            )?;
            return old.recheck();
        }
        if let Some(s) = &self.settings {
            tx.workflow_bind_resources(s.retained_operations, s.retained_bytes, self.deadline()?);
        }
        tx.workflow_snapshot = Some(self.clone());
        Ok(())
    }
    fn shown(&self) -> Result<Value> {
        let value = self.settings.as_ref().map(Settings::value);
        let hash = value
            .as_ref()
            .map(|v| {
                canonical::CanonicalBytes::from_value(v)
                    .map(|b| format!("sha256:{}", canonical::hex(&Sha256::digest(b.as_slice()))))
            })
            .transpose()
            .map_err(core_error)?;
        Ok(
            json!({"schema":"ledger-cli/1","status":"resource_settings","origin":if value.is_some(){"billing.json/resources"}else{"legacy_defaults"},"resources":value,"resources_sha256":hash,"legacy_defaults":{"semantic_work":1048576,"compile_work":1048576,"resolved_records":4096,"resolved_bytes":8388608,"new_body_bytes":8388608,"arena_bytes":67108864,"retained_operations":100000,"retained_bytes":268435456,"attempt_timeout_ms":20000},"retained_constraints":"accepted-format byte/cardinality/ID/depth rules; exact money and financial 32-decision/128-action limits; native contention/cleanup bounds"}),
        )
    }
}
impl local::LocalWorkflowLedger {
    pub(crate) fn resource_snapshot(&self) -> Result<Snapshot> {
        Snapshot::capture(
            &self.admin.configuration,
            &self.admin.logical_store_id,
            &self.admin.scope,
        )
    }
    pub async fn resource_settings(&self) -> Result<Value> {
        let snap = self.resource_snapshot()?;
        let mut tx = self
            .store
            .begin(local::deadline())
            .await
            .map_err(crate::service::store_error)?;
        let result = async {
            let setup = local::setup_identity(&mut tx).await?;
            require(setup.operator == self.admin.operator, "AUTHORITY_REQUIRED")?;
            snap.recheck()?;
            snap.shown()
        }
        .await;
        tx.rollback().await.map_err(crate::service::store_error)?;
        result
    }
    pub async fn set_resource_settings(&self, raw: &[u8]) -> Result<Value> {
        let v = canonical::parse_bounded(raw, io::CONFIG_LIMIT as usize)
            .map_err(|_| reject("INVALID_SHAPE"))?;
        let setting = Settings::parse(&v)?;
        let snap = self.resource_snapshot()?;
        let mut tx = self
            .store
            .begin(local::deadline())
            .await
            .map_err(crate::service::store_error)?;
        let result = async {
            let setup = local::setup_identity(&mut tx).await?;
            require(setup.operator == self.admin.operator, "AUTHORITY_REQUIRED")?;
            snap.recheck()?;
            tx.workflow_enable_resource_schema()
                .await
                .map_err(crate::service::store_error)?;
            Ok(())
        }
        .await;
        if let Err(e) = result {
            tx.rollback().await.map_err(crate::service::store_error)?;
            return Err(e);
        }
        tx.commit().await.map_err(|e| match e {
            crate::store::errors::CommitError::RolledBack(e) => crate::service::store_error(e),
            crate::store::errors::CommitError::OutcomeUnknown => ServiceError::OutcomeUnknown {
                scope: self.admin.scope.clone(),
                source: "local-operator".into(),
                external_id: "resource-schema-upgrade".into(),
            },
        })?;
        // The idempotent storage migration is durable before changing the private
        // file. A second held writer serializes its atomic replacement; no SQL
        // mutation is coupled to that replacement.
        let mut tx = self
            .store
            .begin(local::deadline())
            .await
            .map_err(crate::service::store_error)?;
        let result = async {
            let setup = local::setup_identity(&mut tx).await?;
            require(setup.operator == self.admin.operator, "AUTHORITY_REQUIRED")?;
            snap.recheck()?;
            let mut config = canonical::parse(&snap.raw).map_err(core_error)?;
            config["resources"] = setting.value();
            let bytes = canonical::CanonicalBytes::from_value(&config).map_err(core_error)?;
            let parent = snap.path.parent().ok_or(ServiceError::IntegrityFailure)?;
            let nonce = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| ServiceError::Unavailable)?
                .as_nanos();
            let temp = parent.join(format!(".billing-resources-{}-{nonce}", std::process::id()));
            io::write_new(&temp, bytes.as_slice()).map_err(|_| ServiceError::Unavailable)?;
            if let Err(e) = snap.recheck() {
                let _ = fs::remove_file(&temp);
                return Err(e);
            }
            if let Err(_) = fs::rename(&temp, &snap.path) {
                let _ = fs::remove_file(&temp);
                return Err(ServiceError::Unavailable);
            }
            fs::File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|_| ServiceError::Unavailable)?;
            Ok(())
        }
        .await;
        tx.rollback().await.map_err(crate::service::store_error)?;
        result?;
        self.resource_snapshot()?.shown()
    }
}
pub(crate) fn diagnostic(error: &ledgerlab_core::Error) -> Option<ServiceError> {
    let detail = error.detail.strip_prefix("resource limit: ")?;
    let mut words = detail.split(' ');
    let setting = words.next()?;
    let setting = match setting {
        "semantic_work" => "semantic_work",
        "compile_work" => "compile_work",
        "resolved_records" => "resolved_records",
        "resolved_bytes" => "resolved_bytes",
        "new_body_bytes" => "new_body_bytes",
        "arena_bytes" => "arena_bytes",
        "retained_operations" => "retained_operations",
        "retained_bytes" => "retained_bytes",
        _ => return None,
    };
    let limit = words.next()?.strip_prefix("limit=")?.parse().ok()?;
    let observed = words.next()?.strip_prefix("observed=")?.parse().ok()?;
    if words.next().is_some() {
        return None;
    }
    Some(ServiceError::ResourceLimit {
        setting,
        limit,
        observed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn unlimited() -> Value {
        json!({"schema":"ledger-local-resource-settings/1","semantic_work":null,"compile_work":null,"resolved_records":null,"resolved_bytes":null,"new_body_bytes":null,"arena_bytes":null,"retained_operations":null,"retained_bytes":null,"attempt_timeout_ms":null})
    }
    #[test]
    fn operator_settings_are_closed_safe_json_and_null_is_unlimited() {
        let value = unlimited();
        let s = Settings::parse(&value).unwrap();
        assert_eq!(s.core(), ResourceLimits::unlimited());
        for bad in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("100"),
            json!(9_007_199_254_740_992u64),
        ] {
            let mut v = value.clone();
            v["semantic_work"] = bad;
            assert!(Settings::parse(&v).is_err());
        }
        let mut v = value.clone();
        v.as_object_mut().unwrap().remove("semantic_work");
        assert!(Settings::parse(&v).is_err());
        let mut v = value;
        v["other"] = json!(1);
        assert!(Settings::parse(&v).is_err());
    }
    #[tokio::test]
    async fn operator_settings_migrate_reset_reopen_and_detect_stale_snapshot() {
        use crate::billing::BillingLedger;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().canonicalize().unwrap().join("billing");
        let mut setup: Value =
            serde_json::from_slice(include_bytes!("../../../../../examples/billing/setup.json"))
                .unwrap();
        setup["store_id"] = json!("operator-settings-test");
        BillingLedger::init(
            &path,
            &canonical::CanonicalBytes::from_value(&setup)
                .unwrap()
                .into_vec(),
        )
        .await
        .unwrap();
        let ledger = local::LocalWorkflowLedger::open_local(&path).await.unwrap();
        ledger.enable_profile_b().await.unwrap();
        assert_eq!(
            ledger.resource_settings().await.unwrap()["origin"],
            "legacy_defaults"
        );
        let mut low = unlimited();
        low["arena_bytes"] = json!(1);
        low["semantic_work"] = json!(1);
        low["retained_operations"] = json!(1);
        low["retained_bytes"] = json!(1);
        low["attempt_timeout_ms"] = json!(1);
        let bytes = canonical::CanonicalBytes::from_value(&low).unwrap();
        ledger
            .set_resource_settings(bytes.as_slice())
            .await
            .unwrap();
        assert_eq!(ledger.resource_settings().await.unwrap()["resources"], low);
        let old = ledger.resource_snapshot().unwrap();
        assert!(matches!(
            old.arena(),
            Err(ServiceError::ResourceLimit {
                setting: "arena_bytes",
                limit: 1,
                observed: 1024
            })
        ));
        let value = unlimited();
        ledger
            .set_resource_settings(
                canonical::CanonicalBytes::from_value(&value)
                    .unwrap()
                    .as_slice(),
            )
            .await
            .unwrap();
        assert!(old.recheck().is_err());
        let fresh = ledger.resource_snapshot().unwrap();
        assert!(fresh.deadline().unwrap().is_none());
        let arena = fresh.arena().unwrap();
        let mut s =
            ledgerlab_core::validation_work::OriginalAuditSession::for_resource_profile_v2(&arena)
                .unwrap();
        let mut tx = fresh.begin(&ledger.store, &mut s).await.unwrap();
        assert!(tx.workflow_deadline().is_none());
        assert_eq!(tx.billing_schema_version().await.unwrap(), 17);
        tx.rollback().await.unwrap();
        let key = request::OperationKey {
            logical_store_id: "operator-settings-test".into(),
            scope: ["example-company".into(), "local".into()],
            source: "urn:example:work".into(),
            operation_id: "not-present".into(),
        };
        assert_eq!(
            ledger.status(&key.bytes().unwrap()).await.unwrap()["status"],
            "not_observed"
        );
        ledger.close().await;
        let ordinary = BillingLedger::open(&path).await.unwrap();
        ordinary
            .permission_status("customer-1", "urn:example:work")
            .await
            .unwrap();
        ordinary.close().await;
        let reopened = local::LocalWorkflowLedger::open_local(&path).await.unwrap();
        assert_eq!(
            reopened.resource_settings().await.unwrap()["resources"],
            value
        );
        reopened.close().await;
    }
}

/// Shared strict extension accepted by ordinary billing opens and workflow.
pub(crate) fn installation_shape(v: &Value) -> bool {
    v.as_object().is_some_and(|o| {
        o.len() == 3 + usize::from(o.contains_key("resources"))
            && ["schema", "store_id", "scope"]
                .iter()
                .all(|k| o.contains_key(*k))
    }) && v["schema"] == "ledger-billing-installation/1"
        && v.get("resources")
            .is_none_or(|r| Settings::parse(r).is_ok())
}
