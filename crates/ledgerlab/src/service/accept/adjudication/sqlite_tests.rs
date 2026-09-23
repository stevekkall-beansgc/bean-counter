//! Actual SQLite acceptance tests. Synthetic documents are trusted host inputs;
//! journal observations, capabilities, retained proofs and retries come from SQLite.
use super::*;
use crate::{
    store::sqlite::{SqliteStore, SqliteTx},
    ServiceError,
};
use ledgerlab_core::adjudication::{
    self as r3,
    runtime::{self as rt, accounting::Worksheet, points::State},
};
use serde_json::Value;
use std::time::Duration;
use tokio::time::Instant;

struct Host {
    // A real setup observation is used only to discover the current-head key.
    // Every accepting attempt reuses the value actually read under its transaction.
    discovery: ObservedHead,
    observed_at: r3::types::Time,
}
impl AdjudicationAuthority for Host {
    fn current(
        &self,
        command: &ParsedCommand,
        inputs: &LockedInputs,
        access: AuthorityAccess,
    ) -> Result<AuthorityObservation, ServiceError> {
        let head = inputs
            .heads
            .iter()
            .find(|h| h.key == self.discovery.key)
            .unwrap_or(&self.discovery);
        let state: State = serde_json::from_slice(
            head.value
                .as_deref()
                .ok_or(ServiceError::IntegrityFailure)?,
        )
        .map_err(|_| ServiceError::IntegrityFailure)?;
        let State::AuthorityCurrent(source) = state else {
            return Err(ServiceError::IntegrityFailure);
        };
        let raw = r3::proofs::decode_base64(&source.body, 16384)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let body: Value =
            serde_json::from_slice(&raw).map_err(|_| ServiceError::IntegrityFailure)?;
        AuthorityObservation::from_backend(
            serde_json::from_value(body["principal"].clone()).unwrap(),
            rt::command_digest(command.command()).unwrap(),
            source.body_hash.clone(),
            serde_json::from_value(body["revision"].clone()).unwrap(),
            self.observed_at.clone(),
            match access {
                AuthorityAccess::NewTransition => wire::AuthorityPermission::Capacity,
                AuthorityAccess::ReadSavedResult => wire::AuthorityPermission::Read,
            },
            vec![source],
            vec![head.clone()],
        )
        .map_err(|e| ServiceError::Rejection(e.code.into()))
    }
}
impl AdjudicationHost<SqliteTx> for Host {
    fn guards(&self, _: &ParsedCommand, _: &JournalIdentity) -> Result<Vec<Guard>, ServiceError> {
        Ok(vec![])
    }
    async fn source(&self, _: &SourceRequest) -> Result<VerifiedSource, ServiceError> {
        Err(ServiceError::IntegrityFailure)
    }
    async fn fresh_base(
        &self,
        _: &mut SqliteTx,
        _: &wire::Enroll,
        _: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        Err(ServiceError::IntegrityFailure)
    }
}
fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json")).unwrap()
}
fn parsed(value: &Value) -> ParsedCommand {
    ParsedCommand::parse(&r3::canonical_bytes(value, r3::COMMAND_BYTES).unwrap()).unwrap()
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(30)
}
fn gateway_budget() -> wire::Resource {
    let ws = Worksheet::frozen().unwrap();
    let mut result = ws.template("PREPARE_ENROLL").unwrap().resources().unwrap();
    for _ in 0..32 {
        for kind in ws.bundle("finish_gateway").unwrap() {
            result = result
                .checked_add(&ws.template(kind).unwrap().resources().unwrap())
                .unwrap();
        }
    }
    result
}
#[tokio::test]
async fn actual_prepare_enroll_saved_retry_and_reopen() {
    let input = fixture();
    let command = input["commands"][0].clone();
    let journal = JournalIdentity {
        store: Id::parse("center").unwrap(),
        scope: serde_json::from_value(command["payload"]["scope"].clone()).unwrap(),
        registration: Id::parse("registration").unwrap(),
        host: Id::parse("g0").unwrap(),
    };
    let mut installation = crate::store::sqlite::tests::installation();
    installation.logical_store_id = journal.store.as_str().into();
    installation.scope.tenant = journal.scope.0.as_str().into();
    installation.scope.environment = journal.scope.1.as_str().into();
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), installation).await.unwrap();
    let logical = gateway_budget();
    // Explicit finite synthetic host assignment; this test is not a physical G2 proof.
    let backing = Count::new(1u128 << 40).unwrap();
    let configured = store
        .provision_adjudication(journal.clone(), logical.clone(), 65536, backing)
        .await
        .unwrap();
    let source: wire::AuthoritySource =
        serde_json::from_value(input["initial"]["authority_sources"][0].clone()).unwrap();
    let discovery = store
        .provision_adjudication_authority(&journal, &source, None)
        .await
        .unwrap();
    let host = Host {
        discovery,
        observed_at: serde_json::from_value(command["authority"]["observed_at"].clone()).unwrap(),
    };
    let result = run(
        &configured,
        &host,
        journal.clone(),
        parsed(&command),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(result.status, wire::CommandResultStatus::Committed);
    assert_eq!(result.code, "PREPARE_ENROLL");
    assert!(result.effects.is_empty());
    let key: wire::ProofFullKey = serde_json::from_value(serde_json::json!("g0")).unwrap();
    let proof = store
        .adjudication_source(
            &journal,
            Count::new(1).unwrap(),
            &wire::FactKind::EnrollPreparation,
            &key,
        )
        .await
        .unwrap();
    assert_eq!(proof.prefix().root(), &result.root);
    let mut retry = command.clone();
    retry["authority"]["permission"] = serde_json::json!("read");
    for _ in 0..2 {
        let saved = run(
            &configured,
            &host,
            journal.clone(),
            parsed(&retry),
            deadline(),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, result.root);
        assert!(saved.effects.is_empty());
    }
    drop(configured);
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    let configured = reopened
        .provision_adjudication(journal.clone(), logical, 65536, backing)
        .await
        .unwrap();
    let saved = run(
        &configured,
        &host,
        journal.clone(),
        parsed(&retry),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
    assert_eq!(saved.root, result.root);
    let after = reopened
        .adjudication_source(
            &journal,
            Count::new(1).unwrap(),
            &wire::FactKind::EnrollPreparation,
            &key,
        )
        .await
        .unwrap();
    assert_eq!(after.proof(), proof.proof());
    drop(configured);
    reopened.close().await;
}
