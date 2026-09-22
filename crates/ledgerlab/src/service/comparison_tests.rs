use super::*;
use crate::{
    service::accept::outcome::fixture,
    store::outcomes::{ObservedOutcomeHead, ScopedRecordRef},
};
use std::sync::{Mutex, OnceLock};

fn input() -> (RawRetainedSnapshot, RetainedSelection) {
    static FIXTURE: OnceLock<(RawRetainedSnapshot, RetainedSelection)> = OnceLock::new();
    FIXTURE
        .get_or_init(|| {
            let f = fixture::lifecycle();
            let command = &f.commands[0];
            let selection = RetainedSelection {
                scope: ["synthetic".into(), "sandbox".into()],
                target: command.target.clone(),
                invocation_id: command.invocation_id.clone(),
                expected_snapshot: None,
            };
            let mut heads = f.provisioned_heads.clone();
            let mut rows = std::collections::BTreeMap::new();
            for raw in &f.provisioned_records {
                let v = canonical::outcome::decode(raw).unwrap();
                rows.insert(b::bytes(&v["id"]).unwrap(), raw.clone());
            }
            let mut deliveries = vec![];
            let mut anchors = vec![];
            for plan in &f.plans {
                for raw in plan
                    .economic_records()
                    .iter()
                    .chain(plan.settlement_records())
                {
                    let v = canonical::outcome::decode(raw).unwrap();
                    rows.insert(b::bytes(&v["id"]).unwrap(), raw.clone());
                }
                for w in plan.head_writes() {
                    let h = ObservedOutcomeHead {
                        lock: w.lock.clone(),
                        revision: Some(w.revision.clone()),
                        value: Some(w.value.clone()),
                    };
                    if let Some(old) = heads
                        .iter_mut()
                        .find(|h| h.lock.class == w.lock.class && h.lock.key == w.lock.key)
                    {
                        *old = h;
                    } else {
                        heads.push(h);
                    }
                }
                anchors = plan.anchors().to_vec();
                deliveries.push(plan.delivery().clone());
            }
            heads.retain(|h| {
                !matches!(
                    h.lock.class,
                    OutcomeLockClass::Admission | OutcomeLockClass::Authority
                )
            });
            let target = heads
                .iter()
                .find(|h| h.lock.class == OutcomeLockClass::Target)
                .unwrap();
            let value = retained::head_value(target).unwrap().unwrap();
            let members = value["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|rf| ScopedRecordRef {
                    scope: selection.scope.clone(),
                    kind: rf["kind"].as_str().unwrap().into(),
                    id: b::bytes(&rf["id"]).unwrap(),
                    content_hash: rf["content_hash"].as_str().unwrap().into(),
                })
                .collect::<Vec<_>>();
            let records = members.iter().map(|rf| rows[&rf.id].clone()).collect();
            (
                RawRetainedSnapshot {
                    store_identity: "synthetic-store".into(),
                    anchors,
                    members,
                    records,
                    heads,
                    original_deliveries: deliveries,
                },
                selection,
            )
        })
        .clone()
}
fn verify(
    raw: RawRetainedSnapshot,
    selection: RetainedSelection,
) -> Result<ComparisonWorkspace, ComparisonError> {
    verify_workspace(
        raw,
        selection,
        &Cancellation::default(),
        Instant::now() + WORK_TIMEOUT,
    )
}
#[test]
fn comparison_verified_history_members_heads_receipts_and_fingerprint() {
    let (raw, mut selection) = input();
    let workspace = verify(raw.clone(), selection.clone()).unwrap();
    assert!(!workspace.decisions().is_empty());
    assert!(
        workspace
            .settlement_rows()
            .iter()
            .any(|r| r["kind"] == "reservation-observation"
                && r["body"]["command"]["kind"] == "close")
    );
    assert_eq!(
        workspace.historical_receipts().len(),
        raw.original_deliveries.len()
    );
    let candidates = vec!["a".repeat(64), "b".repeat(64)];
    let p = ReportProvenance::new(&workspace, candidates.clone(), "build-a".into()).unwrap();
    assert!(!p.committed());
    assert_eq!(
        p.descriptor(),
        ReportProvenance::new(&workspace, candidates.clone(), "build-a".into())
            .unwrap()
            .descriptor()
    );
    assert_ne!(
        p.descriptor()["report_provenance"],
        ReportProvenance::new(&workspace, candidates, "build-b".into())
            .unwrap()
            .descriptor()["report_provenance"]
    );
    selection.expected_snapshot = Some(workspace.fingerprint().clone());
    let mut reversed = raw.clone();
    reversed.records.reverse();
    reversed.members.reverse();
    reversed.heads.reverse();
    reversed.anchors.reverse();
    reversed.original_deliveries.reverse();
    assert_eq!(
        verify(reversed, selection.clone()).unwrap().fingerprint(),
        workspace.fingerprint()
    );
    selection.expected_snapshot = Some(SnapshotFingerprint("wrong".into()));
    assert!(matches!(
        verify(raw.clone(), selection),
        Err(ComparisonError::SnapshotChanged)
    ));
    let (_, selection) = input();
    for attack in 0..11 {
        let mut bad = raw.clone();
        match attack {
            0 => {
                bad.members.pop();
            }
            1 => {
                bad.records.pop();
            }
            2 => {
                bad.anchors[0].content_hash = format!("sha256:{}", "0".repeat(64));
            }
            3 => {
                bad.original_deliveries.pop();
            }
            4 => {
                bad.original_deliveries[0]
                    .canonical_key
                    .external_id
                    .push('x');
            }
            5 => {
                bad.heads
                    .iter_mut()
                    .find(|h| h.lock.class == OutcomeLockClass::Target)
                    .unwrap()
                    .revision = Some("999".into());
            }
            6 => {
                bad.members[0].scope[0] = "foreign".into();
            }
            7 => {
                bad.heads.push(bad.heads[0].clone());
            }
            8 => {
                bad.original_deliveries[0].settlement_receipt.push(b' ');
            }
            9 => {
                bad.heads
                    .iter_mut()
                    .find(|h| h.lock.class == OutcomeLockClass::Claim && h.revision.is_some())
                    .unwrap()
                    .revision = Some("999".into());
            }
            10 => {
                bad.heads
                    .iter_mut()
                    .find(|h| h.lock.class == OutcomeLockClass::BindingAggregate)
                    .unwrap()
                    .revision = Some("999".into());
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                verify(bad, selection.clone()),
                Err(ComparisonError::Integrity)
            ),
            "attack {attack}"
        );
    }
}
#[test]
fn comparison_resource_and_cancellation_boundaries() {
    assert_eq!(
        ReadBudget::preflight_records(4096, MAX_RETAINED_BYTES as u64, MAX_ENVELOPE_BYTES as u64),
        Ok(())
    );
    for (n, b, m) in [
        (4097, 0, 0),
        (1, MAX_RETAINED_BYTES as u64 + 1, 0),
        (1, 1, MAX_ENVELOPE_BYTES as u64 + 1),
        (u64::MAX, u64::MAX, u64::MAX),
    ] {
        assert_eq!(
            ReadBudget::preflight_records(n, b, m),
            Err(ReadError::Limit)
        );
    }
    assert!(admit_candidates(&[MAX_CANDIDATE_BYTES; 4]).is_ok());
    for sizes in [
        vec![1],
        vec![1; 9],
        vec![MAX_CANDIDATE_BYTES + 1; 2],
        vec![MAX_CANDIDATE_BYTES; 5],
        vec![usize::MAX; 2],
    ] {
        assert_eq!(admit_candidates(&sizes), Err(ComparisonError::Limit));
    }
    let (raw, selection) = input();
    let authority = ReadAuthorityObservation {
        scope: selection.scope,
        heads: vec![],
        records: vec![vec![0; MAX_ENVELOPE_BYTES]; MAX_RETAINED_BYTES / MAX_ENVELOPE_BYTES],
    };
    let mut shared = validate_authority_bounds(&authority).unwrap();
    assert_eq!(
        validate_raw_bounds_into(&raw, &mut shared),
        Err(ComparisonError::Limit)
    );
    use std::io::Write;
    let mut report = BoundedReport::new();
    report.write_all(&vec![0; MAX_REPORT_BYTES]).unwrap();
    assert!(report.write_all(&[0]).is_err());
    assert_eq!(report.finish(), Err(ComparisonError::Limit));
    let (raw, s) = input();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert!(matches!(
        verify_workspace(
            raw.clone(),
            s.clone(),
            &cancel,
            Instant::now() + WORK_TIMEOUT
        ),
        Err(ComparisonError::Cancelled)
    ));
    assert!(matches!(
        verify_workspace(raw, s, &Cancellation::default(), Instant::now()),
        Err(ComparisonError::Deadline)
    ));
}
#[derive(Default)]
struct Counts {
    begin: usize,
    load: usize,
    finish: usize,
    dropped: usize,
}
struct ReadOnlyStore {
    pending: bool,
    cleanup_failure: bool,
    counts: Arc<Mutex<Counts>>,
}
struct ReadOnlyTx {
    pending: bool,
    cleanup_failure: bool,
    counts: Arc<Mutex<Counts>>,
    finished: bool,
}
impl Drop for ReadOnlyTx {
    fn drop(&mut self) {
        if !self.finished {
            self.counts.lock().unwrap().dropped += 1;
        }
    }
}
// This type implements ONLY the read port; no AcceptanceTx/OutcomeTx exists.
impl ComparisonReadStore for ReadOnlyStore {
    type Read = ReadOnlyTx;
    async fn begin_read(&self, _: Instant) -> Result<ReadOnlyTx, ReadError> {
        self.counts.lock().unwrap().begin += 1;
        Ok(ReadOnlyTx {
            pending: self.pending,
            cleanup_failure: self.cleanup_failure,
            counts: self.counts.clone(),
            finished: false,
        })
    }
}
impl ComparisonReadTx for ReadOnlyTx {
    async fn load_authority(
        &mut self,
        who: &AuthenticatedReadContext,
    ) -> Result<ReadAuthorityObservation, ReadError> {
        Ok(ReadAuthorityObservation {
            scope: who.scope.clone(),
            heads: vec![],
            records: vec![],
        })
    }
    async fn load_retained(
        &mut self,
        _: &RetainedSelection,
    ) -> Result<RawRetainedSnapshot, ReadError> {
        self.counts.lock().unwrap().load += 1;
        if self.pending {
            std::future::pending::<()>().await;
        }
        Ok(input().0)
    }
    async fn finish(mut self) -> Result<(), ReadError> {
        self.finished = true;
        self.counts.lock().unwrap().finish += 1;
        if self.cleanup_failure {
            return Err(ReadError::Unavailable);
        }
        Ok(())
    }
}
struct SyntheticReadAuthority {
    scope: bool,
    disclosure: bool,
}
impl ComparisonReadAuthority for SyntheticReadAuthority {
    fn verify_scope(
        &self,
        _: &AuthenticatedReadContext,
        _: &RetainedSelection,
        _: &ReadAuthorityObservation,
    ) -> Result<(), ComparisonError> {
        if self.scope {
            Ok(())
        } else {
            Err(ComparisonError::Denied)
        }
    }
    fn verify_disclosure(
        &self,
        _: &AuthenticatedReadContext,
        _: &RetainedSelection,
        _: &ReadAuthorityObservation,
        _: &RawRetainedSnapshot,
    ) -> Result<(), ComparisonError> {
        if self.disclosure {
            Ok(())
        } else {
            Err(ComparisonError::Denied)
        }
    }
}
#[tokio::test]
async fn comparison_read_capability_authority_cleanup_and_shared_admission() {
    let (_, s) = input();
    let who = AuthenticatedReadContext {
        scope: s.scope.clone(),
        principal_id: "local-synthetic".into(),
        authority_head: "operator".into(),
    };
    let reader = ReadOnlyStore {
        pending: false,
        cleanup_failure: false,
        counts: Arc::new(Mutex::new(Counts::default())),
    };
    let cancel = Cancellation::default();
    let operation = loop {
        match ComparisonOperation::begin(cancel.clone()) {
            Ok(op) => break op,
            Err(ComparisonError::Busy) => tokio::task::yield_now().await,
            Err(e) => panic!("{e:?}"),
        }
    };
    for (scope, disclosure) in [(false, false), (true, false), (true, true)] {
        let auth = SyntheticReadAuthority { scope, disclosure };
        let result = load_workspace(&reader, &auth, &who, s.clone(), &operation).await;
        if scope && disclosure {
            assert!(result.is_ok());
        } else {
            assert!(matches!(result, Err(ComparisonError::Denied)));
        }
    }
    {
        let c = reader.counts.lock().unwrap();
        assert_eq!((c.begin, c.load, c.finish, c.dropped), (3, 2, 3, 0));
    }
    assert!(matches!(
        ComparisonOperation::begin(Cancellation::default()),
        Err(ComparisonError::Busy)
    ));
    let pending = ReadOnlyStore {
        pending: true,
        cleanup_failure: false,
        counts: reader.counts.clone(),
    };
    let auth = SyntheticReadAuthority {
        scope: true,
        disclosure: true,
    };
    assert!(tokio::time::timeout(
        Duration::from_millis(2),
        load_workspace(&pending, &auth, &who, s.clone(), &operation)
    )
    .await
    .is_err());
    assert_eq!(reader.counts.lock().unwrap().dropped, 1);
    let cleanup_failure = ReadOnlyStore {
        pending: false,
        cleanup_failure: true,
        counts: reader.counts.clone(),
    };
    assert!(matches!(
        load_workspace(&cleanup_failure, &auth, &who, s, &operation).await,
        Err(ComparisonError::Unavailable)
    ));
    drop(operation);
    loop {
        match ComparisonOperation::begin(Cancellation::default()) {
            Ok(op) => {
                drop(op);
                break;
            }
            Err(ComparisonError::Busy) => tokio::task::yield_now().await,
            Err(e) => panic!("{e:?}"),
        }
    }
}

#[test]
fn comparison_foundation_malformed_secret_canary_is_redacted() {
    let (mut raw, selection) = input();
    let canary = "synthetic-only-secret-canary-compare-672941";
    raw.records[0] = format!("{{\"private_evidence\":\"{canary}\"}}").into_bytes();
    let error = verify(raw, selection).err().unwrap();
    assert_eq!(error, ComparisonError::Integrity);
    assert!(!format!("{error:?}").contains(canary));
}

#[test]
fn comparison_foundation_projection_requires_exact_observation_order() {
    let (raw, selection) = input();
    let mut workspace = verify(raw, selection).unwrap();
    assert!(foundation::activity(&workspace).is_ok());
    workspace.history.decisions.swap(0, 1);
    assert!(matches!(
        foundation::activity(&workspace),
        Err(ComparisonError::Integrity)
    ));
}
