//! Coordinator protocol tests. These intentionally model transaction outcomes;
//! actual durability, cancellation and SQL races belong to the two adapter lanes.
use super::*;
use crate::store::records as old;
use std::sync::{Arc, Mutex, OnceLock};
fn fixture() -> fixture::Fixture {
    static F: OnceLock<fixture::Fixture> = OnceLock::new();
    F.get_or_init(fixture::lifecycle).clone()
}
#[derive(Default)]
struct Faults {
    retry_append: bool,
    unknown: Option<bool>,
    legacy: bool,
}
struct Memory {
    snapshot: OutcomeSnapshot,
    deliveries: BTreeMap<ScopedDelivery, StoredCompositeDelivery>,
    commits: usize,
    rollbacks: usize,
    begins: usize,
    appends: usize,
    faults: Faults,
}
#[derive(Clone)]
struct Store(Arc<Mutex<Memory>>);
struct Tx {
    store: Store,
    snapshot: OutcomeSnapshot,
    delivery: Option<StoredCompositeDelivery>,
    locks: Vec<OutcomeLock>,
}
impl Store {
    fn new(f: &fixture::Fixture) -> Self {
        Self(Arc::new(Mutex::new(Memory {
            snapshot: OutcomeSnapshot {
                anchors: vec![],
                records: f.provisioned_records.clone(),
                heads: f.provisioned_heads.clone(),
            },
            deliveries: BTreeMap::new(),
            commits: 0,
            rollbacks: 0,
            begins: 0,
            appends: 0,
            faults: Faults::default(),
        })))
    }
}
impl OutcomeStore for Store {
    type Tx = Tx;
    async fn begin_outcome(&self, _: Instant) -> std::result::Result<Tx, StoreError> {
        let mut m = self.0.lock().unwrap();
        m.begins += 1;
        Ok(Tx {
            store: self.clone(),
            snapshot: m.snapshot.clone(),
            delivery: None,
            locks: vec![],
        })
    }
}
impl OutcomeTx for Tx {
    async fn lock_scopes(&mut self, scopes: &[OutcomeLock]) -> std::result::Result<(), StoreError> {
        assert_eq!(normalize_locks(scopes.to_vec()).unwrap(), scopes);
        self.locks = scopes.to_vec();
        Ok(())
    }
    async fn lookup_outcome_delivery(
        &mut self,
        key: &ScopedDelivery,
    ) -> std::result::Result<Option<StoredCompositeDelivery>, StoreError> {
        let m = self.store.0.lock().unwrap();
        if m.faults.legacy {
            return Err(StoreError::DeliveryConflict);
        }
        Ok(m.deliveries.get(key).cloned())
    }
    async fn resolve_outcome(
        &mut self,
        r: &OutcomeResolve,
    ) -> std::result::Result<OutcomeResolution, StoreError> {
        assert_eq!(r.locks, self.locks);
        let mut s = self.snapshot.clone();
        s.heads = r
            .locks
            .iter()
            .map(|l| {
                let h = self
                    .snapshot
                    .heads
                    .iter()
                    .find(|h| h.lock.class == l.class && h.lock.key == l.key)
                    .unwrap();
                ObservedOutcomeHead {
                    lock: l.clone(),
                    revision: h.revision.clone(),
                    value: h.value.clone(),
                }
            })
            .collect();
        Ok(OutcomeResolution::Complete(s))
    }
    async fn append_outcome(
        &mut self,
        p: &ValidatedOutcomePlan,
    ) -> std::result::Result<(), StoreError> {
        let mut m = self.store.0.lock().unwrap();
        m.appends += 1;
        if std::mem::take(&mut m.faults.retry_append) {
            return Err(StoreError::ExpectedCurrent);
        }
        for observed in p.observed_heads() {
            let actual = m
                .snapshot
                .heads
                .iter()
                .find(|h| h.lock.class == observed.lock.class && h.lock.key == observed.lock.key)
                .unwrap();
            assert_eq!(actual.revision, observed.revision);
            assert_eq!(actual.value, observed.value);
        }
        fixture::apply(&mut self.snapshot, p);
        self.delivery = Some(p.delivery().clone());
        Ok(())
    }
}
impl AcceptanceTx for Tx {
    async fn load_installation(&mut self) -> std::result::Result<old::Installation, StoreError> {
        Ok(old::Installation {
            scope: old::Scope {
                tenant: "synthetic".into(),
                environment: "sandbox".into(),
            },
            logical_store_id: "test".into(),
            mode: "sandbox".into(),
            admission: "open".into(),
            dispatch_hold: true,
            dispatch_enabled: false,
            generation: 1,
        })
    }
    async fn load_outbox(
        &mut self,
        _: crate::outbox::Query,
    ) -> std::result::Result<crate::outbox::Snapshot, StoreError> {
        panic!("unused")
    }
    async fn load_chain(
        &mut self,
        _: &old::Scope,
        _: &str,
    ) -> std::result::Result<Option<old::Chain>, StoreError> {
        panic!("unused")
    }
    async fn load_authority(
        &mut self,
        _: &old::Scope,
        _: &str,
    ) -> std::result::Result<Option<old::AuthorityHead>, StoreError> {
        panic!("unused")
    }
    async fn load_binding(
        &mut self,
        _: &old::Scope,
        _: &str,
    ) -> std::result::Result<Option<old::BindingHead>, StoreError> {
        panic!("unused")
    }
    async fn load_document(
        &mut self,
        _: &old::Scope,
        _: &str,
    ) -> std::result::Result<Option<(String, old::CanonicalRecord)>, StoreError> {
        panic!("unused")
    }
    async fn load_grant_document(
        &mut self,
        _: &old::Scope,
        _: &str,
    ) -> std::result::Result<Option<String>, StoreError> {
        panic!("unused")
    }
    async fn write(&mut self, _: &old::WriteOp) -> std::result::Result<(), StoreError> {
        panic!("unused")
    }
    async fn load_identity(
        &mut self,
        _: &old::Scope,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<old::StoredIdentity>, StoreError> {
        panic!("unused")
    }
    async fn load_claim(
        &mut self,
        _: &old::Scope,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> std::result::Result<Option<old::StoredClaim>, StoreError> {
        panic!("unused")
    }
    async fn commit(self) -> std::result::Result<(), CommitError> {
        let mut m = self.store.0.lock().unwrap();
        let unknown = m.faults.unknown.take();
        if unknown != Some(false) {
            m.snapshot = self.snapshot;
            let d = self.delivery.unwrap();
            m.deliveries.insert(d.key.clone(), d);
            m.commits += 1;
        }
        if unknown.is_some() {
            Err(CommitError::OutcomeUnknown)
        } else {
            Ok(())
        }
    }
    async fn rollback(self) -> std::result::Result<(), StoreError> {
        self.store.0.lock().unwrap().rollbacks += 1;
        Ok(())
    }
}
struct Authority {
    f: fixture::Fixture,
    read_only: bool,
    deny_read: bool,
    deny_early: bool,
}
impl Authority {
    fn new(f: &fixture::Fixture) -> Self {
        Self {
            f: f.clone(),
            read_only: false,
            deny_read: false,
            deny_early: false,
        }
    }
}
impl OutcomeAuthority for Authority {
    fn verify(&self, c: &OutcomeCommand, _: &OutcomeSnapshot, _: bool) -> Result<AuthorityProof> {
        if self.deny_read {
            return Err(ServiceError::Rejection("READ_UNAUTHORIZED".into()));
        }
        let i = match c.operation {
            OutcomeOperation::FinalBase { .. } => 0,
            OutcomeOperation::Economic { .. } => {
                if c.principal.source == "urn:synthetic:correction" {
                    2
                } else {
                    1
                }
            }
            OutcomeOperation::Close { .. } => 3,
        };
        let mut p = self.f.proofs[i].clone();
        p.source = c.principal.source.clone();
        if self.read_only {
            p.authority["permissions"] = json!(["read"])
        }
        if self.deny_early {
            p.authorized_early_close = false;
        }
        Ok(p)
    }
}
async fn accepted(s: &Store, c: &OutcomeCommand, a: &Authority) -> StoredCompositeDelivery {
    match run(s, c, a).await.unwrap() {
        OutcomeResult::Accepted(d) => d,
        v => panic!("expected acceptance: {v:?}"),
    }
}
#[tokio::test]
async fn transaction_path_matches_validated_plans_and_stable_identity() {
    let f = fixture();
    let s = Store::new(&f);
    let mut a = Authority::new(&f);
    for (i, c) in f.commands.iter().enumerate() {
        let d = accepted(&s, c, &a).await;
        assert_eq!(d, f.plans[i].delivery);
    }
    let mut retry = f.commands[1].clone();
    retry.received_at = Timestamp::parse("2026-10-21T12:00:00.000000Z").unwrap();
    retry.accepted_at = retry.received_at.clone();
    a.read_only = true;
    let before = s.0.lock().unwrap().commits;
    assert_eq!(
        run(&s, &retry, &a).await.unwrap(),
        OutcomeResult::Duplicate(f.plans[1].delivery.clone())
    );
    assert_eq!(s.0.lock().unwrap().commits, before);
    let OutcomeOperation::Economic { ingress, .. } = &mut retry.operation else {
        panic!()
    };
    let mut e: Value = serde_json::from_slice(ingress).unwrap();
    e["data"]["code"] = json!("none");
    *ingress = bytes(&e).unwrap();
    assert_eq!(
        run(&s, &retry, &a).await.unwrap(),
        OutcomeResult::IdentityConflict
    );
    a.deny_read = true;
    assert!(matches!(
        run(&s, &f.commands[1], &a).await,
        Err(ServiceError::Rejection(_))
    ));
}
#[tokio::test]
async fn semantic_alias_after_correction_and_closure_keeps_first_pair() {
    let f = fixture();
    let s = Store::new(&f);
    let mut a = Authority::new(&f);
    for c in &f.commands {
        accepted(&s, c, &a).await;
    }
    let mut alias = f.commands[1].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut alias.operation else {
        panic!()
    };
    let mut e: Value = serde_json::from_slice(ingress).unwrap();
    e["data"]["external_id"] = json!("late-semantic-alias");
    *ingress = bytes(&e).unwrap();
    alias.received_at = Timestamp::parse("2026-10-21T12:00:00.000000Z").unwrap();
    alias.accepted_at = alias.received_at.clone();
    a.read_only = true;
    let before = s.0.lock().unwrap().snapshot.records.clone();
    let OutcomeResult::Duplicate(d) = run(&s, &alias, &a).await.unwrap() else {
        panic!()
    };
    assert_eq!(d.economic_receipt, f.plans[1].delivery.economic_receipt);
    assert_eq!(d.settlement_receipt, f.plans[1].delivery.settlement_receipt);
    assert_eq!(s.0.lock().unwrap().snapshot.records, before);
    assert_eq!(
        run(&s, &alias, &a).await.unwrap(),
        OutcomeResult::Duplicate(d)
    );
}
#[tokio::test]
async fn rollback_required_before_expected_current_retry_and_ordered_restart() {
    let f = fixture();
    let s = Store::new(&f);
    let a = Authority::new(&f);
    s.0.lock().unwrap().faults.retry_append = true;
    accepted(&s, &f.commands[0], &a).await;
    let m = s.0.lock().unwrap();
    assert_eq!(m.commits, 1);
    assert_eq!(m.appends, 2);
    assert!(m.rollbacks >= 2);
    assert!(m.begins >= 3);
}
#[tokio::test]
async fn unknown_commit_never_retries_and_original_lookup_resolves() {
    let f = fixture();
    let a = Authority::new(&f);
    for durable in [false, true] {
        let s = Store::new(&f);
        s.0.lock().unwrap().faults.unknown = Some(durable);
        let c = &f.commands[0];
        let err = run(&s, c, &a).await.unwrap_err();
        assert_eq!(err, unknown(&f.plans[0].delivery.key));
        assert_eq!(s.0.lock().unwrap().appends, 1);
        let r = run(&s, c, &a).await.unwrap();
        assert_eq!(matches!(r, OutcomeResult::Duplicate(_)), durable);
        assert_eq!(s.0.lock().unwrap().commits, 1);
    }
}
#[tokio::test]
async fn stale_correction_early_close_and_legacy_conflicts_never_append() {
    let f = fixture();
    let s = Store::new(&f);
    let mut a = Authority::new(&f);
    accepted(&s, &f.commands[0], &a).await;
    accepted(&s, &f.commands[1], &a).await;
    let mut stale = f.commands[2].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut stale.operation else {
        panic!()
    };
    let mut e: Value = serde_json::from_slice(ingress).unwrap();
    e["data"]["expected_revision_number"] = json!("0");
    *ingress = bytes(&e).unwrap();
    assert!(run(&s, &stale, &a).await.is_err());
    a.deny_early = true;
    assert!(run(&s, &f.commands[3], &a).await.is_err());
    assert_eq!(s.0.lock().unwrap().commits, 2);
    let s = Store::new(&f);
    s.0.lock().unwrap().faults.legacy = true;
    assert_eq!(
        run(&s, &f.commands[0], &a).await.unwrap(),
        OutcomeResult::IdentityConflict
    );
    assert_eq!(s.0.lock().unwrap().appends, 0);
}
#[tokio::test]
async fn post_hoc_after_release_preserves_every_reservation_field() {
    let f = fixture();
    let s = Store::new(&f);
    let a = Authority::new(&f);
    accepted(&s, &f.commands[0], &a).await;
    accepted(&s, &f.commands[1], &a).await;
    accepted(&s, &f.commands[3], &a).await;
    let state =
        s.0.lock()
            .unwrap()
            .snapshot
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Reservation)
            .unwrap()
            .clone();
    let mut c = f.commands[2].clone();
    c.received_at = Timestamp::parse("2026-09-21T15:00:00.000000Z").unwrap();
    c.accepted_at = c.received_at.clone();
    accepted(&s, &c, &a).await;
    assert_eq!(
        *s.0.lock()
            .unwrap()
            .snapshot
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Reservation)
            .unwrap(),
        state
    );
}
#[tokio::test]
async fn accepted_zero_consumes_permanent_slot_without_money_or_release() {
    let f = fixture();
    let s = Store::new(&f);
    let a = Authority::new(&f);
    accepted(&s, &f.commands[0], &a).await;
    let mut c = f.commands[1].clone();
    let OutcomeOperation::Economic { ingress, .. } = &mut c.operation else {
        panic!()
    };
    let mut e: Value = serde_json::from_slice(ingress).unwrap();
    e["data"]["code"] = json!("none");
    *ingress = bytes(&e).unwrap();
    let d = accepted(&s, &c, &a).await;
    let receipt: Value = serde_json::from_slice(&d.settlement_receipt).unwrap();
    assert_eq!(receipt["body"]["result"]["held"], "12000");
    assert_eq!(receipt["body"]["result"]["revision"], "1");
    assert_eq!(
        receipt["body"]["result"]["families"][0]["status"],
        "claimed"
    );
    let econ: Value = serde_json::from_slice(d.economic_receipt.as_ref().unwrap()).unwrap();
    assert_eq!(econ["body"]["action_ids"], json!([]));
    assert_eq!(econ["body"]["intention_ids"], json!([]));
}
#[tokio::test]
async fn missing_companion_or_corrupted_locked_head_rejects_before_disclosure() {
    let f = fixture();
    let a = Authority::new(&f);
    for corrupt_record in [true, false] {
        let s = Store::new(&f);
        accepted(&s, &f.commands[0], &a).await;
        accepted(&s, &f.commands[1], &a).await;
        {
            let mut m = s.0.lock().unwrap();
            if corrupt_record {
                m.snapshot
                    .records
                    .retain(|r| *r != f.plans[1].delivery.settlement_receipt);
            } else {
                let h = m
                    .snapshot
                    .heads
                    .iter_mut()
                    .find(|h| h.lock.class == OutcomeLockClass::Reservation)
                    .unwrap();
                let mut v: Value = serde_json::from_slice(h.value.as_ref().unwrap()).unwrap();
                v["held"] = json!("12000");
                h.value = Some(bytes(&v).unwrap());
            }
        }
        assert_eq!(
            run(&s, &f.commands[1], &a).await.unwrap_err(),
            ServiceError::IntegrityFailure
        );
        assert_eq!(s.0.lock().unwrap().commits, 2);
    }
}
fn edit_ingress(c: &mut OutcomeCommand, f: impl FnOnce(&mut Value)) {
    let OutcomeOperation::Economic { ingress, .. } = &mut c.operation else {
        panic!()
    };
    let mut v: Value = serde_json::from_slice(ingress).unwrap();
    f(&mut v);
    *ingress = bytes(&v).unwrap();
}
#[tokio::test]
async fn changed_facts_conflict_and_duplicate_ignores_current_binding_selection() {
    let f = fixture();
    let s = Store::new(&f);
    let a = Authority::new(&f);
    accepted(&s, &f.commands[0], &a).await;
    accepted(&s, &f.commands[1], &a).await;
    let mut conflict = f.commands[1].clone();
    edit_ingress(&mut conflict, |e| {
        e["data"]["external_id"] = json!("conflicting-facts");
        e["data"]["code"] = json!("none");
    });
    assert_eq!(
        run(&s, &conflict, &a).await.unwrap(),
        OutcomeResult::SemanticConflict
    );
    {
        let mut m = s.0.lock().unwrap();
        for h in &mut m.snapshot.heads {
            if h.lock.class == OutcomeLockClass::Binding {
                h.revision = Some("2".into());
                h.value =
                    Some(bytes(&json!({"active":false,"current_selector":"changed"})).unwrap());
            }
        }
    }
    assert!(matches!(
        run(&s, &f.commands[1], &a).await.unwrap(),
        OutcomeResult::Duplicate(_)
    ));
    assert_eq!(s.0.lock().unwrap().commits, 2);
}
#[tokio::test]
async fn zero_net_correction_and_full_reversal_reinstatement_never_change_reservation() {
    let f = fixture();
    let s = Store::new(&f);
    let a = Authority::new(&f);
    accepted(&s, &f.commands[0], &a).await;
    accepted(&s, &f.commands[1], &a).await;
    let original =
        s.0.lock()
            .unwrap()
            .snapshot
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Reservation)
            .unwrap()
            .clone();
    let mut c = f.commands[2].clone();
    edit_ingress(&mut c, |e| {
        e["data"]["replacement"] = json!({"kind":"code","code":"fee"})
    });
    let d = accepted(&s, &c, &a).await;
    let e: Value = serde_json::from_slice(d.economic_receipt.as_ref().unwrap()).unwrap();
    assert_eq!(e["body"]["action_ids"].as_array().unwrap().len(), 2);
    assert_eq!(e["body"]["intention_ids"], json!([]));
    for (index, replacement) in [
        json!({"kind":"reverse"}),
        json!({"kind":"code","code":"fee"}),
    ]
    .into_iter()
    .enumerate()
    {
        let last = {
            let m = s.0.lock().unwrap();
            m.snapshot
                .records
                .iter()
                .map(|r| serde_json::from_slice::<Value>(r).unwrap())
                .filter(|r| r["kind"] == "claim-revision")
                .max_by_key(|r| text(&r["body"]["number"]).unwrap().parse::<u64>().unwrap())
                .unwrap()
        };
        edit_ingress(&mut c, |e| {
            e["data"]["external_id"] = json!(format!("post-hoc-{index}"));
            e["data"]["expected_revision"] = last["id"].clone();
            e["data"]["expected_revision_number"] = last["body"]["number"].clone();
            e["data"]["replacement"] = replacement;
        });
        accepted(&s, &c, &a).await;
    }
    assert_eq!(
        *s.0.lock()
            .unwrap()
            .snapshot
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Reservation)
            .unwrap(),
        original
    );
}
#[tokio::test]
async fn ordinary_and_close_losers_are_domain_rejections_without_writes() {
    let f = fixture();
    let a = Authority::new(&f);
    let mut close = f.commands[3].clone();
    let OutcomeOperation::Close {
        expected_revision, ..
    } = &mut close.operation
    else {
        panic!()
    };
    *expected_revision = "0".into();
    for ordinary_first in [true, false] {
        let s = Store::new(&f);
        accepted(&s, &f.commands[0], &a).await;
        let (first, second, code) = if ordinary_first {
            (&f.commands[1], &close, "EXPECTED_REVISION")
        } else {
            (&close, &f.commands[1], "ORDINARY_CLOSED")
        };
        accepted(&s, first, &a).await;
        assert_eq!(
            run(&s, second, &a).await.unwrap_err(),
            ServiceError::Rejection(code.into())
        );
        assert_eq!(s.0.lock().unwrap().commits, 2);
    }
}
#[tokio::test]
async fn deadline_is_strict_and_repeated_close_is_a_nonmonetary_observation() {
    let f = fixture();
    let s = Store::new(&f);
    let a = Authority::new(&f);
    accepted(&s, &f.commands[0], &a).await;
    let mut c = f.commands[3].clone();
    let OutcomeOperation::Close {
        expected_revision,
        reason,
        ..
    } = &mut c.operation
    else {
        panic!()
    };
    *expected_revision = "0".into();
    *reason = "deadline".into();
    c.received_at = Timestamp::parse("2026-09-23T13:00:00.000000Z").unwrap();
    c.accepted_at = c.received_at.clone();
    assert_eq!(
        run(&s, &c, &a).await.unwrap_err(),
        ServiceError::Rejection("CLOSE_DEADLINE".into())
    );
    c.received_at = Timestamp::parse("2026-09-23T13:00:00.000001Z").unwrap();
    c.accepted_at = c.received_at.clone();
    accepted(&s, &c, &a).await;
    let prior =
        s.0.lock()
            .unwrap()
            .snapshot
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Reservation)
            .unwrap()
            .clone();
    let OutcomeOperation::Close {
        expected_revision,
        external_id,
        ..
    } = &mut c.operation
    else {
        panic!()
    };
    *expected_revision = "1".into();
    *external_id = "closure-noop".into();
    let d = accepted(&s, &c, &a).await;
    assert!(d.economic_receipt.is_none());
    assert_eq!(
        *s.0.lock()
            .unwrap()
            .snapshot
            .heads
            .iter()
            .find(|h| h.lock.class == OutcomeLockClass::Reservation)
            .unwrap(),
        prior
    );
}
