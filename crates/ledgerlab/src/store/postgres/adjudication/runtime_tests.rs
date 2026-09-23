//! Actual runtime/native PostgreSQL projection with an explicit TEST-ONLY
//! physical assumption. No production PG admission implementation is enabled.
use super::*;
use crate::service::accept::adjudication::{self as coordinator, *};
use crate::store::{errors::CommitError, outcomes::*, postgres::PostgresTx, records::*};
use crate::ServiceError;
use r3::{
    runtime::points::{ResourceState, State},
    ParsedCommand,
};
use serde_json::Value as Json;
use std::{collections::BTreeMap, sync::Arc};
#[path = "runtime_base.rs"]
mod runtime_base;
use runtime_base::BaseFixture;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}
fn parse(v: &Json) -> ParsedCommand {
    ParsedCommand::parse(&r3::canonical_bytes(v, r3::COMMAND_BYTES).unwrap()).unwrap()
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
fn budget(host: &str) -> wire::Resource {
    let ws = runtime::accounting::Worksheet::frozen().unwrap();
    let mut out = wire::Resource::zero();
    for _ in 0..32 {
        for name in ws
            .bundle(if host == "center" {
                "finish_central"
            } else {
                "finish_gateway"
            })
            .unwrap()
        {
            out = out
                .checked_add(&ws.template(name).unwrap().resources().unwrap())
                .unwrap();
        }
    }
    for _ in 0..8 {
        for t in ws.transitions.values() {
            out = out.checked_add(&t.resources().unwrap()).unwrap();
        }
    }
    out
}
fn encode(bytes: &[u8]) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        out.push(A[(c[0] >> 2) as usize] as char);
        out.push(A[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        out.push(if c.len() > 1 {
            A[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            A[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
struct PgRuntimeHarness<'a> {
    store: &'a PostgresStore,
    command: ParsedCommand,
    fail_at: Option<usize>,
    assumed_ceiling: Option<wire::Resource>,
    fault_count: Arc<AtomicU64>,
}
struct PgRuntimeTx {
    inner: PostgresTx,
    journal: JournalIdentity,
    work: WorkRequest,
    transaction: Digest,
    fail_at: Option<usize>,
    assumed_ceiling: Option<wire::Resource>,
    fault_count: Arc<AtomicU64>,
}
impl AdjudicationStore for PgRuntimeHarness<'_> {
    type Tx = PgRuntimeTx;
    async fn begin_adjudication(
        &self,
        w: &WorkRequest,
        d: Instant,
    ) -> Result<PgRuntimeTx, StoreError> {
        if runtime::command_digest(self.command.command()).map_err(core)? != w.transition {
            return Err(invalid());
        }
        let inner = self
            .store
            .begin_native_storage(w.journal.clone(), &self.command, d)
            .await
            .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?;
        let transaction = r3::raw_sha256(
            format!(
                "TEST-ONLY-live-PG-transaction:{}:{}",
                inner.pid,
                inner.admission_pid.load(Ordering::Acquire)
            )
            .as_bytes(),
        );
        Ok(PgRuntimeTx {
            inner,
            journal: w.journal.clone(),
            work: w.clone(),
            transaction,
            fail_at: self.fail_at,
            assumed_ceiling: self.assumed_ceiling.clone(),
            fault_count: self.fault_count.clone(),
        })
    }
}
impl AdjudicationTx for PgRuntimeTx {
    async fn lock_adjudication(&mut self, g: &[Guard]) -> Result<(), StoreError> {
        self.inner
            .native_adjudication(Operation::Locks(self.journal.clone(), g.to_vec()))
            .await
            .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?;
        Ok(())
    }
    async fn lookup_adjudication(
        &mut self,
        j: &JournalIdentity,
        k: &wire::Delivery,
    ) -> Result<Option<SavedOutcome>, StoreError> {
        let Value::Saved(v) = self
            .inner
            .native_adjudication(Operation::Lookup(j.clone(), k.clone()))
            .await
            .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?
        else {
            return Err(invalid());
        };
        Ok(*v)
    }
    async fn resolve_adjudication(&mut self, q: &ResolveRequest) -> Result<Resolution, StoreError> {
        let Value::Resolved(v) = self
            .inner
            .native_adjudication(Operation::Resolve(q.clone()))
            .await
            .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?
        else {
            return Err(invalid());
        };
        Ok(Resolution::Complete(v))
    }
    async fn commit_capability(&mut self) -> Result<CommitCapability, StoreError> {
        let Value::Head(head) = self
            .inner
            .native_adjudication(Operation::Head(self.journal.clone()))
            .await
            .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?
        else {
            return Err(invalid());
        };
        // This helper is compiled ONLY in this test module. It assumes the
        // missing physical envelope to exercise runtime/native atomicity. These
        // quantities are NOT enforced PG limits or evidence of promised finish.
        let assumed = Count::new(1u128 << 60).map_err(core)?;
        let marker = r3::raw_sha256(b"TEST-ONLY-UNPROVED-PG-PHYSICAL-ENVELOPE");
        let physical = PhysicalEnvelope::from_backend(
            assumed,
            assumed,
            assumed,
            assumed,
            assumed,
            assumed,
            marker.clone(),
        )
        .map_err(core)?;
        CommitCapability::from_backend(
            self.journal.clone(),
            self.transaction.clone(),
            marker,
            Count::new(u128::from(self.journal.host != self.journal.store)).map_err(core)?,
            head,
            self.work.owner.clone(),
            physical,
            self.assumed_ceiling
                .clone()
                .unwrap_or_else(|| budget(self.journal.host.as_str())),
            None,
        )
        .map_err(core)
    }
    async fn append_adjudication(
        &mut self,
        p: &ValidatedAdjudicationPlan,
        cap: &CommitCapability,
    ) -> Result<(), StoreError> {
        let Value::Head(now) = self
            .inner
            .native_adjudication(Operation::Head(self.journal.clone()))
            .await
            .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?
        else {
            return Err(invalid());
        };
        if cap.journal() != &self.journal
            || cap.transaction() != &self.transaction
            || cap.allocation_owner() != &self.work.owner
            || cap.recovered_through().root() != now.root()
            || p.prior().root() != now.root()
            || cap.recovered_through().ordinal() != now.ordinal()
        {
            return Err(invalid());
        }
        if let Some(at) = self.fail_at {
            self.inner
                .fail_outcome_at(at)
                .await
                .inspect_err(|e| eprintln!("PG_RUNTIME_STORE_ERROR {e:?}"))?;
        }
        let result = self
            .inner
            .native_adjudication(Operation::Append(Box::new(p.clone())))
            .await;
        if self.fail_at.is_some() {
            assert!(
                matches!(
                    &result,
                    Err(StoreError::Integrity("injected outcome write boundary"))
                ),
                "expected exact injected write boundary"
            );
            self.fault_count.fetch_add(1, Ordering::Relaxed);
        }
        result.inspect_err(|e| eprintln!("PG_RUNTIME_APPEND_ERROR {e:?}"))?;
        Ok(())
    }
}
impl AcceptanceTx for PgRuntimeTx {
    async fn load_outbox(
        &mut self,
        q: crate::outbox::Query,
    ) -> Result<crate::outbox::Snapshot, StoreError> {
        self.inner.load_outbox(q).await
    }
    async fn load_installation(&mut self) -> Result<Installation, StoreError> {
        self.inner.load_installation().await
    }
    async fn load_chain(&mut self, s: &Scope, id: &str) -> Result<Option<Chain>, StoreError> {
        self.inner.load_chain(s, id).await
    }
    async fn load_authority(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<AuthorityHead>, StoreError> {
        self.inner.load_authority(s, id).await
    }
    async fn load_binding(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<BindingHead>, StoreError> {
        self.inner.load_binding(s, id).await
    }
    async fn load_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<(String, CanonicalRecord)>, StoreError> {
        self.inner.load_document(s, id).await
    }
    async fn load_grant_document(
        &mut self,
        s: &Scope,
        id: &str,
    ) -> Result<Option<String>, StoreError> {
        self.inner.load_grant_document(s, id).await
    }
    async fn load_identity(
        &mut self,
        s: &Scope,
        source: &str,
        id: &str,
    ) -> Result<Option<StoredIdentity>, StoreError> {
        self.inner.load_identity(s, source, id).await
    }
    async fn load_claim(
        &mut self,
        s: &Scope,
        source: &str,
        op: &str,
        kind: &str,
        token: &str,
    ) -> Result<Option<StoredClaim>, StoreError> {
        self.inner.load_claim(s, source, op, kind, token).await
    }
    async fn write(&mut self, w: &WriteOp) -> Result<(), StoreError> {
        self.inner.write(w).await
    }
    async fn commit(self) -> Result<(), CommitError> {
        self.inner.commit().await
    }
    async fn rollback(self) -> Result<(), StoreError> {
        self.inner.rollback().await
    }
}
struct Host {
    stores: BTreeMap<String, Fixture>,
    heads: BTreeMap<String, ObservedHead>,
    sources: Vec<wire::AuthoritySource>,
    base: BaseFixture,
    now: r3::types::Time,
    fault_count: Arc<AtomicU64>,
}
impl AdjudicationAuthority for Host {
    fn current(
        &self,
        c: &ParsedCommand,
        i: &LockedInputs,
        access: AuthorityAccess,
    ) -> Result<AuthorityObservation, ServiceError> {
        let discovery = &self.heads[i.journal.host.as_str()];
        let h = i
            .heads
            .iter()
            .find(|h| h.key == discovery.key)
            .unwrap_or(discovery);
        let State::AuthorityCurrent(source) =
            serde_json::from_slice(h.value.as_ref().ok_or(ServiceError::IntegrityFailure)?)
                .map_err(|_| ServiceError::IntegrityFailure)?
        else {
            return Err(ServiceError::IntegrityFailure);
        };
        let raw = r3::proofs::decode_base64(&source.body, 16384)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let b: Json = serde_json::from_slice(&raw).unwrap();
        let value = runtime::command_value(c.command()).unwrap();
        let permission = if matches!(access, AuthorityAccess::ReadSavedResult) {
            "read"
        } else {
            match value["kind"].as_str().unwrap() {
                "ENROLL" => "enroll",
                "RECEIVE" | "SUPPLEMENT" => "submit",
                "BEGIN" | "CLOSE" | "ABORT" => "close",
                "CORRECT" => "correct",
                "REPLACE_WRITER" => "replace",
                "DECIDE" if value["payload"]["path"] == "ADJUSTMENT" => "adjust",
                "DECIDE" => "decide",
                _ => "capacity",
            }
        };
        let permission = serde_json::from_value(json!(permission)).unwrap();
        let mut exact = if matches!(c.command(), wire::Command::Enroll { .. }) {
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
                    exact.push(
                        self.sources
                            .iter()
                            .find(|s| s.body_hash.as_str() == hash)
                            .expect("configured exact authority source")
                            .clone(),
                    );
                }
            }
        }
        exact.push(source.clone());
        AuthorityObservation::from_backend(
            serde_json::from_value(b["principal"].clone()).unwrap(),
            runtime::command_digest(c.command()).unwrap(),
            source.body_hash,
            serde_json::from_value(b["revision"].clone()).unwrap(),
            self.now.clone(),
            permission,
            exact,
            vec![h.clone()],
        )
        .map_err(|e| ServiceError::Rejection(e.code.into()))
    }
}
impl AdjudicationHost<PgRuntimeTx> for Host {
    fn guards(&self, c: &ParsedCommand, _: &JournalIdentity) -> Result<Vec<Guard>, ServiceError> {
        Ok(if matches!(c.command(), wire::Command::Enroll { .. }) {
            self.base
                .resolve
                .locks
                .iter()
                .cloned()
                .map(Guard::Legacy)
                .collect()
        } else {
            vec![]
        })
    }
    async fn source(&self, r: &SourceRequest) -> Result<VerifiedSource, ServiceError> {
        let (j, n, kind, key, expected) = match r {
            SourceRequest::Exact(p) => (
                JournalIdentity {
                    store: p.store.clone(),
                    scope: p.scope.clone(),
                    registration: p.registration.clone(),
                    host: p.host.clone(),
                },
                p.ordinal,
                p.fact_kind.clone(),
                p.full_key.clone(),
                Some(p.as_ref()),
            ),
            SourceRequest::Enrollment { journal: j } => {
                let f = self
                    .stores
                    .get(j.host.as_str())
                    .ok_or(ServiceError::IntegrityFailure)?;
                let key = r3::canonical_bytes(&json!(j.registration), 4096).unwrap();
                // Actual indexed primary lookup, never a supplied ordinal/root.
                let rows=f.owner.client.query("SELECT ordinal,full_key FROM ledgerlab.r3_objects WHERE journal=$1 AND kind='ENROLLMENT' AND key_hash=sha256($2) LIMIT 2",&[&journal_key(j).unwrap(),&key]).await.map_err(|_|ServiceError::IntegrityFailure)?;
                if rows.len() != 1 || rows[0].get::<_, Vec<u8>>(1) != key {
                    return Err(ServiceError::IntegrityFailure);
                }
                (
                    j.clone(),
                    ordinal(rows[0].get(0)).map_err(crate::service::store_error)?,
                    wire::FactKind::Enrollment,
                    serde_json::from_value(json!(j.registration)).unwrap(),
                    None,
                )
            }
        };
        let source = self.export(&j, n, kind, key).await?;
        if expected.is_some_and(|p| source.proof() != p) {
            return Err(ServiceError::IntegrityFailure);
        }
        Ok(source)
    }
    async fn fresh_base(
        &self,
        tx: &mut PgRuntimeTx,
        t: &wire::Enroll,
        i: &LockedInputs,
    ) -> Result<FreshBaseAcceptance, ServiceError> {
        self.base.prepare(&mut tx.inner, t, i).await
    }
}
impl Host {
    async fn export(
        &self,
        j: &JournalIdentity,
        n: Count,
        kind: wire::FactKind,
        key: wire::ProofFullKey,
    ) -> Result<VerifiedSource, ServiceError> {
        let f = self
            .stores
            .get(j.host.as_str())
            .ok_or(ServiceError::IntegrityFailure)?;
        let mut tx = begin(f, j).await;
        let value = tx
            .native_adjudication(Operation::Source(j.clone(), n, kind, key))
            .await
            .map_err(crate::service::store_error)?;
        tx.rollback().await.map_err(crate::service::store_error)?;
        let Value::Source(s) = value else {
            return Err(ServiceError::IntegrityFailure);
        };
        Ok(*s)
    }
}
async fn provision(
    f: &mut Fixture,
    j: &JournalIdentity,
    s: &wire::AuthoritySource,
) -> ObservedHead {
    let raw = r3::proofs::decode_base64(&s.body, 16384).unwrap();
    let body: Json = serde_json::from_slice(&raw).unwrap();
    let key = HeadKey {
        journal: j.clone(),
        kind: HeadKind::Authority,
        full_key: runtime::index_key(
            *b"AUTHCURR",
            &[
                body["source"].as_str().unwrap().as_bytes(),
                body["id"].as_str().unwrap().as_bytes(),
            ],
        )
        .unwrap(),
    };
    let tx = f.owner.client.transaction().await.unwrap();
    let jk = journal_key(j).unwrap();
    tx.execute(
        "INSERT INTO ledgerlab.r3_journals VALUES($1,$2,$3,$4,$4)",
        &[
            &jk,
            &identity(j).unwrap(),
            &number(Count::ZERO),
            &"0".repeat(64),
        ],
    )
    .await
    .unwrap();
    let rs = State::Resource(Box::new(ResourceState::genesis(
        budget(j.host.as_str()),
        Count::new(u128::from(j.host != j.store)).unwrap(),
    )));
    for (k, v) in [
        (
            HeadKey {
                journal: j.clone(),
                kind: HeadKind::Resource,
                full_key: runtime::index_key(*b"RESOURCE", &[j.host.as_str().as_bytes()]).unwrap(),
            },
            r3::canonical_bytes(&rs, r3::COMMAND_BYTES).unwrap(),
        ),
        (
            key.clone(),
            r3::canonical_bytes(&State::AuthorityCurrent(s.clone()), r3::COMMAND_BYTES).unwrap(),
        ),
    ] {
        tx.execute(
            "INSERT INTO ledgerlab.r3_heads VALUES($1,$2,$3,$4,$5)",
            &[&jk, &tag(k.kind), &k.full_key, &number(Count::ZERO), &v],
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
    ObservedHead {
        key,
        revision: Some(Count::ZERO),
        value: Some(
            r3::canonical_bytes(&State::AuthorityCurrent(s.clone()), r3::COMMAND_BYTES).unwrap(),
        ),
    }
}
async fn provision_original(f: &mut Fixture, b: &BaseFixture) {
    let tx = f.owner.client.transaction().await.unwrap();
    // Trusted synthetic evidence and Authority/Binding heads only. No accepted
    // receipt, target, members, posting, consumption, or economic head is seeded.
    for raw in &b.evidence {
        let v: Json = serde_json::from_slice(raw).unwrap();
        assert_eq!(v["kind"], "evidence");
        let id = r3::canonical_bytes(&v["id"], 4096).unwrap();
        tx.execute("INSERT INTO ledgerlab.outcome_records VALUES('synthetic','sandbox','evidence',$1,$2,$3)",&[&id,&v["content_hash"].as_str().unwrap(),&raw]).await.unwrap();
    }
    for h in &b.heads {
        assert!(matches!(
            h.lock.class,
            OutcomeLockClass::Authority | OutcomeLockClass::Binding
        ));
        let class = h.lock.class as i16;
        tx.execute(
            "INSERT INTO ledgerlab.outcome_scope_locks VALUES('synthetic','sandbox',$1,$2)",
            &[&class, &h.lock.key],
        )
        .await
        .unwrap();
        tx.execute(
            "INSERT INTO ledgerlab.outcome_heads VALUES('synthetic','sandbox',$1,$2,1,$3)",
            &[&class, &h.lock.key, &h.value.as_ref().unwrap()],
        )
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();
}
async fn all_inventory(f: &Fixture) -> Vec<(String, String)> {
    let tables=f.owner.client.query("SELECT tablename FROM pg_tables WHERE schemaname='ledgerlab' AND tablename<>'r3_unresolved_work' ORDER BY tablename",&[]).await.unwrap();
    let mut out = vec![];
    for row in tables {
        let name: String = row.get(0);
        assert!(name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'));
        let query=format!("SELECT md5(coalesce(string_agg(row_to_json(t)::text,E'\\n' ORDER BY row_to_json(t)::text),'')) FROM ledgerlab.{name} t");
        let digest: String = f.owner.client.query_one(&query, &[]).await.unwrap().get(0);
        out.push((name, digest));
    }
    out
}
async fn execute(
    h: &Host,
    owner: &str,
    c: &Json,
    fail: Option<usize>,
) -> Result<wire::CommandResult, ServiceError> {
    execute_with_ceiling(h, owner, c, fail, None).await
}
async fn execute_with_ceiling(
    h: &Host,
    owner: &str,
    c: &Json,
    fail: Option<usize>,
    assumed_ceiling: Option<wire::Resource>,
) -> Result<wire::CommandResult, ServiceError> {
    let parsed = parse(c);
    let store = PgRuntimeHarness {
        store: &h.stores[owner].store,
        command: parsed.clone(),
        fail_at: fail,
        assumed_ceiling,
        fault_count: h.fault_count.clone(),
    };
    coordinator::run(&store, h, journal(owner), parsed, deadline()).await
}
#[tokio::test]
#[ignore = "requires isolated PG17/18; physical envelope explicitly assumed, runtime/atomicity evidence only"]
async fn native_runtime_prepare_and_atomic_original_enroll() {
    let input:Json=serde_json::from_str(include_str!("../../../../../../contracts/candidates/central-adjudication-r3-candidate1/customer-trace.json")).unwrap();
    let base = BaseFixture::new(&input);
    let sources: Vec<wire::AuthoritySource> =
        serde_json::from_value(input["initial"]["authority_sources"].clone()).unwrap();
    let source = sources
        .iter()
        .find(|s| {
            s.body_hash.as_str()
                == input["commands"][0]["authority"]["document"]
                    .as_str()
                    .unwrap()
        })
        .unwrap();
    let mut host = Host {
        stores: BTreeMap::new(),
        heads: BTreeMap::new(),
        sources: sources.clone(),
        fault_count: Arc::new(AtomicU64::new(0)),
        base,
        now: serde_json::from_value(input["commands"][0]["authority"]["observed_at"].clone())
            .unwrap(),
    };
    for name in ["center", "g0", "g1", "g2", "g3"] {
        let mut f = Fixture::new_scope("synthetic").await;
        let h = provision(&mut f, &journal(name), source).await;
        if name == "center" {
            provision_original(&mut f, &host.base).await;
        }
        host.heads.insert(name.into(), h);
        host.stores.insert(name.into(), f);
    }
    let mut proofs = vec![];
    for n in 0..4 {
        let owner = format!("g{n}");
        let c = input["commands"][n].clone();
        let result = execute(&host, &owner, &c, None).await.unwrap();
        assert_eq!(result.code, "PREPARE_ENROLL");
        assert_eq!(result.status, wire::CommandResultStatus::Committed);
        let before = all_inventory(&host.stores[&owner]).await;
        let mut retry = c.clone();
        retry["authority"]["permission"] = json!("read");
        assert_eq!(
            execute(&host, &owner, &retry, None).await.unwrap().status,
            wire::CommandResultStatus::Duplicate
        );
        assert_eq!(before, all_inventory(&host.stores[&owner]).await);
        let mut tx = begin(&host.stores[&owner], &journal(&owner)).await;
        let Value::Source(s) = tx
            .native_adjudication(Operation::Source(
                journal(&owner),
                Count::new(1).unwrap(),
                wire::FactKind::EnrollPreparation,
                serde_json::from_value(json!(owner)).unwrap(),
            ))
            .await
            .unwrap()
        else {
            panic!("source")
        };
        assert_eq!(s.prefix().root(), &result.root);
        proofs.push(serde_json::to_value(s.proof()).unwrap());
        tx.rollback().await.unwrap();
    }
    proofs.sort_by_cached_key(|p| r3::canonical_bytes(p, r3::COMMAND_BYTES).unwrap());
    let mut command = input["commands"][4].clone();
    command["payload"]["preparations"] = Json::Array(proofs);
    command["authority"]["command"] =
        json!(runtime::command_digest(parse(&command).command()).unwrap());
    let before = all_inventory(&host.stores["center"]).await;
    let mut unauthorized = command.clone();
    unauthorized["authority"]["principal"] = json!("untrusted-principal");
    unauthorized["authority"]["command"] =
        json!(runtime::command_digest(parse(&unauthorized).command()).unwrap());
    assert!(execute(&host, "center", &unauthorized, None).await.is_err());
    assert_eq!(before, all_inventory(&host.stores["center"]).await);
    // A real failure after the first original membership write must roll back both
    // original acceptance and native enrollment, leaving all authoritative rows.
    assert!(execute(&host, "center", &command, Some(4)).await.is_err());
    assert_eq!(before, all_inventory(&host.stores["center"]).await);
    assert_eq!(host.fault_count.load(Ordering::Relaxed), 1);
    let result = execute(&host, "center", &command, None).await.unwrap();
    assert_eq!(result.code, "ENROLL");
    assert_eq!(result.status, wire::CommandResultStatus::Committed);
    let before = all_inventory(&host.stores["center"]).await;
    let mut retry = command.clone();
    retry["authority"]["permission"] = json!("read");
    assert_eq!(
        execute(&host, "center", &retry, None).await.unwrap().status,
        wire::CommandResultStatus::Duplicate
    );
    assert_eq!(before, all_inventory(&host.stores["center"]).await);
    let f = host.stores.get_mut("center").unwrap();
    f.store.clone().close().await;
    f.store = PostgresStore::open(config(&f.name, false)).await.unwrap();
    assert_eq!(
        execute(&host, "center", &retry, None).await.unwrap().root,
        result.root
    );
    let f = &host.stores["center"];
    let records = f
        .owner
        .client
        .query(
            "SELECT envelope FROM ledgerlab.outcome_records ORDER BY kind,id",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(records.len(), host.base.objects.len());
    for o in &host.base.objects {
        let raw = r3::proofs::decode_base64(&o.body, r3::COMMAND_BYTES).unwrap();
        assert!(records.iter().any(|r| r.get::<_, Vec<u8>>(0) == raw));
    }
    let n: i64 = f
        .owner
        .client
        .query_one(
            "SELECT count(*) FROM ledgerlab.outcome_anchors WHERE kind='base-acceptance'",
            &[],
        )
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 1);
    let n: i64 = f
        .owner
        .client
        .query_one("SELECT count(*) FROM ledgerlab.r3_segments", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 1);
    let members: i64 = f
        .owner
        .client
        .query_one("SELECT count(*) FROM ledgerlab.outcome_members", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(members, 29);
    let mut retail = 0i128;
    let mut supplier = 0i128;
    for row in &records {
        let v: Json = serde_json::from_slice(&row.get::<_, Vec<u8>>(0)).unwrap();
        if v["kind"] == "base-posting" {
            let n = v["body"]["amount"]["atoms"]
                .as_str()
                .unwrap()
                .parse::<i128>()
                .unwrap();
            match v["body"]["book"].as_str().unwrap() {
                "retail" => retail += n,
                "supplier" => supplier += n,
                _ => panic!("original book"),
            }
        }
    }
    assert_eq!((retail, supplier), (10000, 3000));
    let slot = f
        .owner
        .client
        .query_one(
            "SELECT state,generation FROM ledgerlab.r3_unresolved_work WHERE singleton=1",
            &[],
        )
        .await
        .unwrap();
    assert_eq!(slot.get::<_, String>(0), "IDLE");
    assert_eq!(slot.get::<_, Vec<u8>>(1), number(Count::ZERO));
    let mut tx = begin(f, &journal("center")).await;
    let Value::Source(s) = tx
        .native_adjudication(Operation::Source(
            journal("center"),
            Count::new(1).unwrap(),
            wire::FactKind::Enrollment,
            serde_json::from_value(json!("registration")).unwrap(),
        ))
        .await
        .unwrap()
    else {
        panic!("source")
    };
    assert_eq!(s.prefix().root(), &result.root);
    tx.rollback().await.unwrap();
    println!(
        "NATIVE_RUNTIME_ATOMIC_ENROLL {}",
        json!({"journals":5,"runtime_commands":5,"saved_retries":6,"original_records":records.len(),"center_root":result.root,"physical_admission":"ASSUMED TEST ONLY / UNPROVED"})
    );
    for (_, f) in host.stores {
        f.finish().await;
    }
}

#[path = "runtime_customer.rs"]
mod runtime_customer;

#[path = "runtime_remaining.rs"]
mod runtime_remaining;
