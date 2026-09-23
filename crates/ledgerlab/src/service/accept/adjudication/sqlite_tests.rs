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
    let anchor = tempfile::tempdir().unwrap();
    let store = SqliteStore::create_fenced(dir.path(), installation, anchor.path())
        .await
        .unwrap();
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
    let reopened = SqliteStore::open_fenced(dir.path(), anchor.path())
        .await
        .unwrap();
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

use super::base_fixture::BaseFixture;
use std::collections::BTreeMap;
struct FlowHost {
    stores: BTreeMap<String, SqliteStore>,
    heads: BTreeMap<String, ObservedHead>,
    sources: Vec<wire::AuthoritySource>,
    base: BaseFixture,
    now: r3::types::Time,
}
impl AdjudicationAuthority for FlowHost {
    fn current(
        &self,
        command: &ParsedCommand,
        inputs: &LockedInputs,
        access: AuthorityAccess,
    ) -> Result<AuthorityObservation, ServiceError> {
        let discovery = &self.heads[inputs.journal.host.as_str()];
        let current = inputs
            .heads
            .iter()
            .find(|h| h.key == discovery.key)
            .unwrap_or(discovery);
        let state: State = serde_json::from_slice(
            current
                .value
                .as_deref()
                .ok_or(ServiceError::IntegrityFailure)?,
        )
        .map_err(|_| ServiceError::IntegrityFailure)?;
        let State::AuthorityCurrent(source) = state else {
            return Err(ServiceError::IntegrityFailure);
        };
        let raw = r3::proofs::decode_base64(&source.body, 16384).unwrap();
        let body: Value = serde_json::from_slice(&raw).unwrap();
        let value = rt::command_value(command.command()).unwrap();
        let permission = if matches!(access, AuthorityAccess::ReadSavedResult) {
            "read"
        } else {
            match value["kind"].as_str().unwrap() {
                "ENROLL" => "enroll",
                "RECEIVE" => "submit",
                "BEGIN" | "CLOSE" => "close",
                "DECIDE" if value["payload"]["path"] == "ADJUSTMENT" => "adjust",
                "DECIDE" => "decide",
                "CORRECT" => "correct",
                _ => "capacity",
            }
        };
        let mut sources = if value["kind"] == "ENROLL" {
            self.sources
                .iter()
                .filter(|s| s.body_hash != source.body_hash)
                .cloned()
                .collect()
        } else {
            vec![]
        };
        if matches!(value["kind"].as_str(), Some("DECIDE" | "CORRECT")) {
            for reference in [
                &value["payload"]["assent"],
                &value["payload"]["roles"]["payer_delegation"],
            ] {
                if let Some(hash) = reference.as_str() {
                    sources.push(
                        self.sources
                            .iter()
                            .find(|s| s.body_hash.as_str() == hash)
                            .expect("configured exact authority source")
                            .clone(),
                    );
                }
            }
        }
        sources.push(source.clone());
        AuthorityObservation::from_backend(
            serde_json::from_value(body["principal"].clone()).unwrap(),
            rt::command_digest(command.command()).unwrap(),
            source.body_hash,
            serde_json::from_value(body["revision"].clone()).unwrap(),
            self.now.clone(),
            serde_json::from_value(serde_json::json!(permission)).unwrap(),
            sources,
            vec![current.clone()],
        )
        .map_err(|e| ServiceError::Rejection(e.code.into()))
    }
}
impl AdjudicationHost<SqliteTx> for FlowHost {
    fn guards(
        &self,
        command: &ParsedCommand,
        _: &JournalIdentity,
    ) -> Result<Vec<Guard>, ServiceError> {
        if matches!(command.command(), wire::Command::Enroll { .. }) {
            Ok(self
                .base
                .resolve
                .locks
                .iter()
                .cloned()
                .map(Guard::Legacy)
                .collect())
        } else {
            Ok(vec![])
        }
    }
    async fn source(&self, request: &SourceRequest) -> Result<VerifiedSource, ServiceError> {
        match request {
            SourceRequest::Exact(p) => {
                self.stores[p.host.as_str()]
                    .adjudication_exact_source(p)
                    .await
            }
            SourceRequest::Enrollment { journal, .. } => {
                self.stores[journal.host.as_str()]
                    .adjudication_enrollment_source(journal)
                    .await
            }
        }
        .map_err(crate::service::store_error)
    }
    async fn fresh_base(
        &self,
        tx: &mut SqliteTx,
        terms: &wire::Enroll,
        inputs: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        self.base.prepare(tx, terms, inputs).await
    }
}
fn flow_budget(host: &str) -> wire::Resource {
    let ws = Worksheet::frozen().unwrap();
    let mut total = wire::Resource::zero();
    for _ in 0..32 {
        for kind in ws
            .bundle(if host == "center" {
                "finish_central"
            } else {
                "finish_gateway"
            })
            .unwrap()
        {
            total = total
                .checked_add(&ws.template(kind).unwrap().resources().unwrap())
                .unwrap();
        }
    }
    // Ample finite first-path allowance, separately quoted from the exact runtime worksheet.
    for _ in 0..8 {
        for t in ws.transitions.values() {
            total = total.checked_add(&t.resources().unwrap()).unwrap();
        }
    }
    total
}
fn journal(host: &str) -> JournalIdentity {
    JournalIdentity {
        store: Id::parse("center").unwrap(),
        scope: wire::Scope(
            Id::parse("synthetic").unwrap(),
            Id::parse("sandbox").unwrap(),
        ),
        registration: Id::parse("registration").unwrap(),
        host: Id::parse(host).unwrap(),
    }
}
fn proof_key(value: &Value) -> String {
    serde_json::to_string(&serde_json::json!([
        value["host"],
        value["fact_kind"],
        value["full_key"]
    ]))
    .unwrap()
}
fn hydrate(command: &mut Value, proofs: &BTreeMap<String, Value>, root: &Digest) {
    for field in ["proof", "begin"] {
        if command["payload"].get(field).is_some() {
            let key = proof_key(&command["payload"][field]);
            command["payload"][field] = proofs[&key].clone();
        }
    }
    if let Some(list) = command["payload"]
        .get_mut("preparations")
        .and_then(Value::as_array_mut)
    {
        for proof in list {
            *proof = proofs[&proof_key(proof)].clone();
        }
    }
    if command["kind"] == "LOCAL_GRANT" {
        command["payload"]["grant"]["journal_head"] = serde_json::json!(root);
        let mut unsigned = command["payload"]["grant"].clone();
        unsigned.as_object_mut().unwrap().remove("authentication");
        command["payload"]["grant"]["authentication"] =
            serde_json::json!(rt::hash("grant", &unsigned).unwrap());
    }
    command["authority"]["head"] = serde_json::json!(root);
    command["authority"]["command"] =
        serde_json::json!(rt::command_digest(parsed(command).command()).unwrap());
}
#[tokio::test]
async fn actual_atomic_base_receipt_import_and_finish() {
    actual_flow(false).await;
}
#[tokio::test]
async fn actual_customer_ninety_five_commands() {
    actual_flow(true).await;
}
async fn actual_flow(customer: bool) {
    let input = fixture();
    let base = BaseFixture::new(&input);
    let mut stores = BTreeMap::new();
    let mut heads = BTreeMap::new();
    let mut dirs = Vec::new();
    let mut anchors = Vec::new();
    let sources: Vec<wire::AuthoritySource> =
        serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
    for host in ["center", "g0", "g1", "g2", "g3"] {
        let dir = tempfile::tempdir().unwrap();
        let mut install = crate::store::sqlite::tests::installation();
        install.logical_store_id = "center".into();
        install.scope.tenant = "synthetic".into();
        install.scope.environment = "sandbox".into();
        let anchor = tempfile::tempdir().unwrap();
        let store = SqliteStore::create_fenced(dir.path(), install, anchor.path())
            .await
            .unwrap();
        drop(
            store
                .provision_adjudication(
                    journal(host),
                    flow_budget(host),
                    65536,
                    Count::new(1u128 << 40).unwrap(),
                )
                .await
                .unwrap(),
        );
        heads.insert(
            host.into(),
            store
                .provision_adjudication_authority(&journal(host), &sources[0], None)
                .await
                .unwrap(),
        );
        if host == "center" {
            store
                .test_provision_outcome_evidence(&base.evidence, &base.heads)
                .await
                .unwrap();
        }
        stores.insert(host.into(), store);
        dirs.push(dir);
        anchors.push(anchor);
    }
    let host = FlowHost {
        stores,
        heads,
        sources,
        base,
        now: serde_json::from_value(input["commands"][0]["authority"]["observed_at"].clone())
            .unwrap(),
    };
    let mut roots: BTreeMap<String, Digest> = ["center", "g0", "g1", "g2", "g3"]
        .into_iter()
        .map(|h| (h.into(), Digest::parse(&"0".repeat(64)).unwrap()))
        .collect();
    let mut ordinals: BTreeMap<String, u128> = roots.keys().map(|h| (h.clone(), 0)).collect();
    let mut proofs = BTreeMap::new();
    let mut schedule: Vec<Value> = (0..=10)
        .chain(12..=15)
        .chain([44, 45, 60, 62])
        .chain(68..=78)
        .map(|index| input["commands"][index].clone())
        .collect();
    let mut returned = input["commands"][15].clone();
    returned["kind"] = serde_json::json!("RETURN_UNUSED");
    returned["key"][2] = serde_json::json!("return-token2");
    returned["payload"]["claim"] = input["commands"][14]["payload"]["token"]["claim"].clone();
    schedule.push(returned);
    let mut reconcile = input["commands"][46].clone();
    reconcile["payload"]["proof"]["fact_kind"] = serde_json::json!("RETURNED_UNUSED");
    schedule.push(reconcile);
    schedule.extend([47, 64].map(|index| input["commands"][index].clone()));
    schedule.extend((79..=90).map(|index| input["commands"][index].clone()));
    if customer {
        schedule = input["commands"].as_array().unwrap().clone();
    }
    let mut last_command = None;
    let mut witness = Vec::new();
    let mut customer_oracle = CustomerOracle::default();
    let case: wire::Case =
        serde_json::from_value(input["commands"][9]["payload"]["submission"]["case"].clone())
            .unwrap();
    let case_key = HeadKey {
        journal: journal("center"),
        kind: HeadKind::Case,
        full_key: rt::points::Point::case(&case).unwrap().key,
    };
    let mut pending = None;
    for (index, mut command) in schedule.into_iter().enumerate() {
        let kind = command["kind"].as_str().unwrap().to_owned();
        let owner = match kind.as_str() {
            "PREPARE_ENROLL" | "ACTIVATE" | "RECEIVE" | "RETURN_UNUSED" | "LOCAL_TERMINAL"
            | "SEAL_BEGIN" | "SEALED" | "INSTALL" => {
                command["payload"]["gateway"].as_str().unwrap()
            }
            "LOCAL_GRANT" => command["payload"]["grant"]["gateway"].as_str().unwrap(),
            _ => "center",
        }
        .to_owned();
        hydrate(&mut command, &proofs, &roots[&owner]);
        // REGISTER carries the actual local grant body, including its actual head.
        if kind == "REGISTER_GRANT" {
            let proof = &command["payload"]["proof"];
            let source = host.stores[proof["host"].as_str().unwrap()]
                .adjudication_exact_source(&serde_json::from_value(proof.clone()).unwrap())
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(
                &r3::proofs::decode_base64(&source.exact_object.body, r3::COMMAND_BYTES).unwrap(),
            )
            .unwrap();
            command["payload"]["grant"] = body["payload"]["grant"].clone();
            hydrate(&mut command, &proofs, &roots[&owner]);
        }
        let configured = host.stores[&owner]
            .provision_adjudication(
                journal(&owner),
                flow_budget(&owner),
                65536,
                Count::new(1u128 << 40).unwrap(),
            )
            .await
            .unwrap();
        if kind == "ENROLL" {
            host.base
                .assert_atomic_state(&host.stores["center"], false)
                .await;
            host.stores["center"].test_fail_after_original_base();
            let failed = run(
                &configured,
                &host,
                journal(&owner),
                parsed(&command),
                deadline(),
            )
            .await;
            assert!(
                matches!(failed, Err(ServiceError::Retryable)),
                "injected post-original cut: {failed:?}"
            );
            host.base
                .assert_atomic_state(&host.stores["center"], false)
                .await;
            assert!(host.stores["center"]
                .adjudication_enrollment_source(&journal("center"))
                .await
                .is_err());
        }
        if customer && matches!(index, 91 | 94) {
            // Current valid source documents do not authorize excess pool spend
            // or a stale economic revision. Refusal leaves the actual
            // case head byte-identical before the valid command commits.
            let key: wire::Case =
                serde_json::from_value(command["payload"]["case"].clone()).unwrap();
            let case_head = HeadKey {
                journal: journal("center"),
                kind: HeadKind::Case,
                full_key: rt::points::Point::case(&key).unwrap().key,
            };
            let before = actual_point(&configured, case_head.clone(), GuardClass::Case).await;
            let mut invalid = command.clone();
            invalid["key"][2] = serde_json::json!(format!("negative-economic-{index}"));
            let expected = if index == 91 {
                invalid["payload"]["signed_atoms"] = serde_json::json!("101");
                "ADJUSTMENT_CAPACITY"
            } else {
                invalid["payload"]["expected_revision"] = serde_json::json!("2");
                "REVISION"
            };
            hydrate(&mut invalid, &proofs, &roots[&owner]);
            let rejected = run(
                &configured,
                &host,
                journal(&owner),
                parsed(&invalid),
                deadline(),
            )
            .await;
            assert!(
                matches!(&rejected,Err(ServiceError::Rejection(code)) if code==expected),
                "expected {expected}: {rejected:?}"
            );
            let after = actual_point(&configured, case_head, GuardClass::Case).await;
            assert_eq!(before.revision, after.revision);
            assert_eq!(before.value, after.value);
        }
        let result = run(
            &configured,
            &host,
            journal(&owner),
            parsed(&command),
            deadline(),
        )
        .await
        .unwrap_or_else(|e| panic!("step {index} {kind}: {e}"));
        assert_eq!(
            result.status,
            wire::CommandResultStatus::Committed,
            "step {index}"
        );
        if kind == "ENROLL" {
            host.base
                .assert_atomic_state(&host.stores["center"], true)
                .await;
            let mut second = command.clone();
            second["key"][2] = serde_json::json!("re-enroll-same-original");
            hydrate(&mut second, &proofs, &result.root);
            assert!(
                matches!(
                    run(
                        &configured,
                        &host,
                        journal(&owner),
                        parsed(&second),
                        deadline()
                    )
                    .await,
                    Err(ServiceError::Unavailable)
                ),
                "occupied original key cannot be enrolled through a fresh control key"
            );
        }
        if kind == "IMPORT" {
            pending = Some(actual_point(&configured, case_key.clone(), GuardClass::Case).await);
        }
        if kind == "CLOSE" && !customer {
            let after = actual_point(&configured, case_key.clone(), GuardClass::Case).await;
            let before = pending.as_ref().unwrap();
            assert_eq!(
                after.revision, before.revision,
                "closure must not rewrite pending case"
            );
            assert_eq!(
                after.value, before.value,
                "closure must not rewrite pending case"
            );
            let family_key = HeadKey {
                journal: journal("center"),
                kind: HeadKind::Family,
                full_key: rt::points::Point::family(&case.0).unwrap().key,
            };
            let family =
                actual_point(&configured, family_key, GuardClass::FamilyPrerequisite).await;
            let State::Case(c) = serde_json::from_slice(after.value.as_deref().unwrap()).unwrap()
            else {
                panic!("case")
            };
            let State::Family(f) =
                serde_json::from_slice(family.value.as_deref().unwrap()).unwrap()
            else {
                panic!("family")
            };
            assert_eq!(c.status, rt::points::CaseStatus::OrdinaryPending);
            assert_eq!(
                c.effective_status(&f),
                rt::points::CaseStatus::AdjustmentPending
            );
        }
        if customer {
            customer_oracle
                .observe(&configured, index + 1, &input, &result)
                .await;
        }
        roots.insert(owner.clone(), result.root.clone());
        *ordinals.get_mut(&owner).unwrap() += 1;
        let fact = match kind.as_str() {
            "PREPARE_ENROLL" => Some(("ENROLL_PREPARATION", command["payload"]["gateway"].clone())),
            "ENROLL" => Some(("ENROLLMENT", serde_json::json!("registration"))),
            "LOCAL_GRANT" => Some(("GRANT", command["payload"]["grant"]["id"].clone())),
            "ISSUE" => Some(("CLAIM", command["payload"]["token"]["id"].clone())),
            "RECEIVE" => Some(("RECEIPT", command["payload"]["token"].clone())),
            "RETURN_UNUSED" => Some(("RETURNED_UNUSED", command["payload"]["token"].clone())),
            "RECONCILE" => Some(("RECONCILIATION", command["payload"]["token"].clone())),
            "BEGIN" => Some(("BEGIN", command["payload"]["round"].clone())),
            "SEALED" => Some(("SEAL", command["payload"]["round"].clone())),
            "CLOSE" => Some(("TERMINAL", command["payload"]["round"].clone())),
            "INSTALL" => Some(("INSTALLATION", command["payload"]["round"].clone())),
            _ => None,
        };
        if let Some((kind, key)) = fact {
            let source = host.stores[&owner]
                .adjudication_source(
                    &journal(&owner),
                    Count::new(ordinals[&owner]).unwrap(),
                    &serde_json::from_value(serde_json::json!(kind)).unwrap(),
                    &serde_json::from_value(key).unwrap(),
                )
                .await
                .unwrap();
            let value = serde_json::to_value(source.proof()).unwrap();
            proofs.insert(proof_key(&value), value);
        }
        eprintln!(
            "actual step {index} {kind} host={owner} ordinal={} root={}",
            ordinals[&owner],
            result.root.as_str()
        );
        let mut retry = command.clone();
        retry["authority"]["permission"] = serde_json::json!("read");
        let saved = run(
            &configured,
            &host,
            journal(&owner),
            parsed(&retry),
            deadline(),
        )
        .await
        .unwrap();
        assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
        assert_eq!(saved.root, result.root);
        assert_eq!(saved.effects, result.effects);
        if kind == "CLOSE" {
            let [wire::Effect::Closure { body }] = result.effects.as_slice() else {
                panic!("missing closure")
            };
            assert_eq!(body.round.value(), 1);
        }
        witness.push(serde_json::json!({"host":owner,"ordinal":ordinals[&owner].to_string(),"command":command,"result":result}));
        last_command = Some((owner, retry, result.root));
        drop(configured);
    }
    let (last_host, last_retry, last_root) = last_command.unwrap();
    assert_eq!(last_host, "center");
    assert!(ordinals["center"] > 4);
    assert!(ordinals["g1"] > 4);
    let mut host = host;
    for store in std::mem::take(&mut host.stores).into_values() {
        store.close().await;
    }
    for ((name, dir), anchor) in ["center", "g0", "g1", "g2", "g3"]
        .into_iter()
        .zip(&dirs)
        .zip(&anchors)
    {
        host.stores.insert(
            name.into(),
            SqliteStore::open_fenced(dir.path(), anchor.path())
                .await
                .unwrap(),
        );
    }
    let configured = host.stores["center"]
        .provision_adjudication(
            journal("center"),
            flow_budget("center"),
            65536,
            Count::new(1u128 << 40).unwrap(),
        )
        .await
        .unwrap();
    let saved = run(
        &configured,
        &host,
        journal("center"),
        parsed(&last_retry),
        deadline(),
    )
    .await
    .unwrap();
    assert_eq!(saved.status, wire::CommandResultStatus::Duplicate);
    assert_eq!(saved.root, last_root);
    drop(configured);
    reader_checks::check(&host.stores["center"], &witness).await;
    for store in host.stores.into_values() {
        store.close().await;
    }
    if let Ok(path) = std::env::var("LEDGERLAB_R3_ACTUAL_TRACE") {
        std::fs::write(path,serde_json::to_vec_pretty(&serde_json::json!({"format":"ledgerlab-actual-sqlite-first-path/1","steps":witness,"heads":roots,"ordinals":ordinals.iter().map(|(k,v)|(k.clone(),v.to_string())).collect::<BTreeMap<_,_>>(),"physical_g2":"PENDING","durable_publication":"external per-store STABLE anchor outside database directory","customer_story":if customer {"EXACT_95"} else {"FIRST_PATH_ONLY"},"oracle_checkpoints":customer_oracle.checkpoints})).unwrap()).unwrap();
    }
}

async fn actual_point<S: AdjudicationStore>(
    store: &S,
    key: HeadKey,
    class: GuardClass,
) -> ObservedHead {
    use crate::store::{
        outcomes::{OutcomeLock, OutcomeLockClass, OutcomeLockMode},
        ports::AcceptanceTx,
    };
    let work = WorkRequest {
        journal: key.journal.clone(),
        owner: Id::parse("test-point-read").unwrap(),
        transition: r3::raw_sha256(b"test-point-read"),
        mandatory: false,
        maximum: Worksheet::frozen()
            .unwrap()
            .template("ENROLL")
            .unwrap()
            .resources()
            .unwrap(),
    };
    let mut tx = store.begin_adjudication(&work, deadline()).await.unwrap();
    let mut guards = vec![
        Guard::Legacy(OutcomeLock {
            class: OutcomeLockClass::Admission,
            key: r3::canonical_bytes(&serde_json::json!([key.journal.scope]), 4096).unwrap(),
            mode: OutcomeLockMode::Write,
        }),
        Guard::R3 {
            class,
            host: key.journal.host.clone(),
            key: key.full_key.clone(),
        },
    ];
    guards.sort_by_key(Guard::order_key);
    tx.lock_adjudication(&guards).await.unwrap();
    let result = tx
        .resolve_adjudication(&ResolveRequest {
            journal: key.journal.clone(),
            key: wire::Delivery(
                key.journal.scope.clone(),
                r3::types::Source::parse("test-point-read").unwrap(),
                Id::parse("read").unwrap(),
            ),
            guards,
            objects: vec![],
            heads: vec![key],
        })
        .await
        .unwrap();
    let Resolution::Complete(inputs) = result else {
        panic!("point resolution")
    };
    let head = inputs.heads[0].clone();
    tx.rollback().await.unwrap();
    head
}

// Independently authored oracle EXPECTATIONS.json SHA256
// 8d43d92d2f31be079527e93874bbcb5f8ce05945ba673fe0c47588a89ec4fd67.
// These literals are not read from candidate checkpoint outputs or runtime replay.
struct CustomerOracle {
    retail: i128,
    supplier: i128,
    actions: Vec<wire::Action>,
    late_before_close: Vec<ObservedHead>,
    checkpoints: Vec<Value>,
}
impl Default for CustomerOracle {
    fn default() -> Self {
        Self {
            retail: 10000,
            supplier: 3000,
            actions: vec![],
            late_before_close: vec![],
            checkpoints: vec![],
        }
    }
}
impl CustomerOracle {
    async fn observe<S: AdjudicationStore>(
        &mut self,
        store: &S,
        through: usize,
        input: &Value,
        result: &wire::CommandResult,
    ) {
        for effect in &result.effects {
            if let wire::Effect::Action { body } = effect {
                assert_eq!(
                    body.magnitude.value(),
                    body.signed_atoms.value().unsigned_abs()
                );
                match body.book {
                    wire::ActionBook::Retail => self.retail += body.signed_atoms.value(),
                    wire::ActionBook::Supplier => self.supplier += body.signed_atoms.value(),
                }
                self.actions.push(body.clone());
            }
        }
        const THROUGH: [usize; 15] = [5, 11, 12, 18, 19, 25, 26, 32, 38, 44, 91, 92, 93, 94, 95];
        const RETAIL: [i128; 15] = [
            10000, 10000, 11200, 11200, 11200, 11200, 11700, 11700, 11700, 11700, 11700, 11800,
            11650, 11650, 11450,
        ];
        const ENTITLEMENTS: [usize; 15] = [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 4, 5, 5];
        let Some(step) = THROUGH.iter().position(|n| *n == through) else {
            return;
        };
        assert_eq!(self.retail, RETAIL[step], "oracle S{step:02} retail");
        assert_eq!(self.supplier, 3000, "oracle S{step:02} supplier");
        let mut families = Vec::new();
        for terms in input["commands"][4]["payload"]["families"]
            .as_array()
            .unwrap()
        {
            let key: wire::Family = serde_json::from_value(terms["key"].clone()).unwrap();
            let point = rt::points::Point::family(&key).unwrap();
            let head = actual_point(
                store,
                HeadKey {
                    journal: journal("center"),
                    kind: HeadKind::Family,
                    full_key: point.key,
                },
                GuardClass::FamilyPrerequisite,
            )
            .await;
            let State::Family(family) =
                serde_json::from_slice(head.value.as_deref().unwrap()).unwrap()
            else {
                panic!("family")
            };
            families.push(*family);
        }
        let consumed = families
            .iter()
            .filter(|f| matches!(f.entitlement, wire::EntitlementHead::Consumed { .. }))
            .count();
        assert_eq!(
            consumed, ENTITLEMENTS[step],
            "oracle S{step:02} entitlements"
        );
        assert_eq!(
            families.iter().filter(|f| f.closed).count(),
            if step >= 10 { 5 } else { 0 }
        );
        assert_eq!(
            families
                .iter()
                .map(|f| f.ordinary_positive.value())
                .sum::<u128>(),
            if step >= 6 {
                1700
            } else if step >= 2 {
                1200
            } else {
                0
            },
            "correction cannot restore premium usage"
        );
        let mut pools = Vec::new();
        for name in ["positive", "negative", "zero"] {
            let head = actual_point(
                store,
                HeadKey {
                    journal: journal("center"),
                    kind: HeadKind::Adjustment,
                    full_key: rt::index_key(*b"ADJPOOL_", &[name.as_bytes()]).unwrap(),
                },
                GuardClass::AdjustmentPool,
            )
            .await;
            let State::Adjustment(pool) =
                serde_json::from_slice(head.value.as_deref().unwrap()).unwrap()
            else {
                panic!("pool")
            };
            pools.push(*pool);
        }
        let positive = if step >= 11 { 100 } else { 0 };
        let negative = if step >= 12 { 150 } else { 0 };
        assert_eq!(
            pools.iter().map(|p| p.gross_used.value()).sum::<u128>(),
            positive + negative
        );
        assert_eq!(
            pools.iter().map(|p| p.positive_used.value()).sum::<u128>(),
            positive
        );
        assert_eq!(
            pools.iter().map(|p| p.negative_used.value()).sum::<u128>(),
            negative
        );
        assert_eq!(
            pools
                .iter()
                .map(|p| p.funding_used.value())
                .collect::<Vec<_>>(),
            vec![positive, negative, 0]
        );
        if step >= 4 {
            let denied = oracle_case(store, input, 16).await;
            assert_eq!(denied.status, rt::points::CaseStatus::FinalDeny);
            assert_eq!(denied.revision, Count::ZERO);
            assert!(!self.actions.iter().any(|a| a.case == denied.input.case));
        }
        if step >= 6 {
            let qualified = oracle_case(store, input, 23).await;
            let wire::EntitlementHead::Consumed { case, revision, .. } = &families[1].entitlement
            else {
                panic!("upsell consumer")
            };
            assert_eq!(**case, qualified.input.case);
            assert_eq!(revision.value(), if step == 14 { 2 } else { 1 });
            assert_eq!(qualified.signed.value(), if step == 14 { 300 } else { 500 });
        }
        if step == 9 || step == 10 {
            for (i, command) in [30, 36, 42].into_iter().enumerate() {
                let key: wire::Case = serde_json::from_value(
                    input["commands"][command]["payload"]["submission"]["case"].clone(),
                )
                .unwrap();
                let head = actual_point(
                    store,
                    HeadKey {
                        journal: journal("center"),
                        kind: HeadKind::Case,
                        full_key: rt::points::Point::case(&key).unwrap().key,
                    },
                    GuardClass::Case,
                )
                .await;
                if step == 9 {
                    self.late_before_close.push(head);
                } else {
                    assert_eq!(
                        head.revision, self.late_before_close[i].revision,
                        "virtual close writes no case"
                    );
                    assert_eq!(
                        head.value, self.late_before_close[i].value,
                        "virtual close writes no case"
                    );
                    let State::Case(c) =
                        serde_json::from_slice(head.value.as_deref().unwrap()).unwrap()
                    else {
                        panic!("late case")
                    };
                    assert_eq!(c.status, rt::points::CaseStatus::OrdinaryPending);
                    assert_eq!(
                        c.effective_status(&families[i + 2]),
                        rt::points::CaseStatus::AdjustmentPending
                    );
                    assert!(families[i + 2].first_closure.is_some());
                }
            }
        }
        if step == 4 || step == 13 {
            assert!(
                result.effects.is_empty(),
                "DENY and zero ALLOW post no action"
            );
        }
        if step == 11 || step == 12 {
            let wire::Effect::Action { body } = &result.effects[0] else {
                panic!("adjustment action")
            };
            assert_eq!(result.effects.len(), 1);
            assert_eq!(
                body.roles.payer.as_str(),
                if step == 11 { "customer" } else { "vendor" }
            );
            assert_eq!(
                body.roles.recipient.as_str(),
                if step == 11 { "vendor" } else { "customer" }
            );
            assert_eq!(body.kind, wire::ActionKind::Adjustment);
        }
        if step == 14 {
            assert_eq!(result.effects.len(), 2);
            let wire::Effect::Action { body: inverse } = &result.effects[0] else {
                panic!("inverse")
            };
            let wire::Effect::Action { body: replacement } = &result.effects[1] else {
                panic!("replacement")
            };
            assert_eq!(
                (inverse.kind.clone(), inverse.signed_atoms.value()),
                (wire::ActionKind::Inverse, -500)
            );
            assert_eq!(
                (replacement.kind.clone(), replacement.signed_atoms.value()),
                (wire::ActionKind::Replacement, 300)
            );
            assert_eq!(self.actions.len(), 6);
        }
        self.checkpoints.push(serde_json::json!({"id":format!("S{step:02}"),"through":through,"retail":self.retail.to_string(),"supplier":self.supplier.to_string(),"entitlements":consumed,"gross":(positive+negative).to_string(),"closed":step>=10}));
    }
}
async fn oracle_case<S: AdjudicationStore>(
    store: &S,
    input: &Value,
    receive: usize,
) -> rt::points::CaseState {
    let case: wire::Case =
        serde_json::from_value(input["commands"][receive]["payload"]["submission"]["case"].clone())
            .unwrap();
    let head = actual_point(
        store,
        HeadKey {
            journal: journal("center"),
            kind: HeadKind::Case,
            full_key: rt::points::Point::case(&case).unwrap().key,
        },
        GuardClass::Case,
    )
    .await;
    let State::Case(c) = serde_json::from_slice(head.value.as_deref().unwrap()).unwrap() else {
        panic!("case")
    };
    *c
}

#[path = "reader_checks.rs"]
mod reader_checks;
