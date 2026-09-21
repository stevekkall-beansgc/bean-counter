//! Independent fixture-to-storage projection. No core/evaluator is used as an oracle.
use super::*;
use crate::store::{ports::AcceptanceTx, records::*};
use serde_json::Value;
use sqlx::AssertSqlSafe;
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

const ACCEPTED: &str =
    include_str!("../../../../../fixtures/journals/first-slice/accepted-records.jsonl");
const SEED_DOCS: &str =
    include_str!("../../../../../fixtures/journals/first-slice/seed-documents.jsonl");
const SEED_ROWS: &str =
    include_str!("../../../../../fixtures/journals/first-slice/seed-records.jsonl");
const STATE: &str = include_str!("../../../../../fixtures/journals/first-slice/preseed-state.json");
const RECEIPT: &[u8] = include_bytes!("../../../../../fixtures/journals/first-slice/receipt.json");
const T: i64 = 1789912800000000;
fn text(v: &Value, k: &str) -> String {
    v[k].as_str()
        .unwrap_or_else(|| panic!("missing text {k}: {v}"))
        .into()
}
fn number(v: &Value, k: &str) -> i64 {
    v[k].as_i64()
        .unwrap_or_else(|| v[k].as_str().unwrap().parse().unwrap())
}
fn scope() -> Scope {
    Scope {
        tenant: "demo".into(),
        environment: "sandbox".into(),
    }
}
fn values(s: &str) -> Vec<Value> {
    s.lines()
        .map(|line| {
            let v: Value = serde_json::from_str(line).unwrap();
            assert_eq!(
                serde_json::to_string(&v).unwrap(),
                line,
                "fixture byte preservation"
            );
            v
        })
        .collect()
}
fn accepted() -> Vec<Value> {
    values(ACCEPTED)
}
fn one(kind: &str) -> Value {
    accepted().into_iter().find(|v| v["kind"] == kind).unwrap()
}
pub(crate) fn row(v: &Value) -> JournalRecord {
    let b = &v["body"];
    let row = match v["kind"].as_str().unwrap() {
        "document" => JournalRow::Document {
            id: text(v, "id"),
            kind: text(v, "document_type"),
        },
        "party" => JournalRow::Party {
            id: text(v, "id"),
            role_metadata_doc: text(b, "role_metadata_doc"),
        },
        "source-grant-record" => JournalRow::SourceGrant {
            id: text(v, "id"),
            principal_id: text(b, "principal_id"),
            source: text(b, "source"),
            grant_doc: text(b, "grant_doc"),
        },
        "binding-record" => JournalRow::Binding {
            id: text(v, "id"),
            agreement_id: text(b, "agreement_id"),
            version: number(b, "version"),
            policy_doc: text(b, "policy_doc"),
            assent_doc: text(b, "assent_doc"),
            roles_doc: text(b, "roles_doc"),
            context_doc: text(b, "context_doc"),
            currency: text(b, "currency"),
            scale: number(b, "scale"),
        },
        "event" => JournalRow::Event {
            id: text(v, "id"),
            source: text(b, "source"),
            external_id: text(b, "id"),
            operation_id: text(b, "operation_id"),
            kind: text(b, "type"),
            chain_id: text(b, "chain"),
            decision_id: text(&one("decision-manifest"), "id"),
            ingress_hash: text(&one("delivery-key")["body"], "ingress_hash"),
            claim_facts_hash: text(&one("claim")["body"], "facts_hash"),
            ingress_bytes: serde_json::to_vec(&one("delivery-key")["body"]["ingress"]).unwrap(),
            occurred_us: None,
            received_us: T,
        },
        "snapshot-ref" => JournalRow::Snapshot {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            document_id: text(b, "document_id"),
            purpose: text(b, "purpose"),
        },
        "delivery-key" => JournalRow::DeliveryKey {
            source: text(b, "source"),
            external_id: text(b, "external_id"),
            ingress_hash: text(b, "ingress_hash"),
            canonical_event_id: text(b, "canonical_event_id"),
            kind: text(b, "kind"),
            observed_us: T,
        },
        "claim" => JournalRow::Claim {
            id: text(v, "id"),
            source: text(b, "source"),
            operation_id: text(b, "operation_id"),
            kind: text(b, "kind"),
            token: text(b, "token"),
            facts_hash: text(b, "facts_hash"),
            event_id: text(b, "event_id"),
        },
        "effect" => JournalRow::Effect {
            id: text(v, "id"),
            agreement_id: text(b, "agreement_id"),
            component: text(b, "component"),
            claim_id: text(b, "claim_id"),
            namespace: text(b, "namespace"),
            facts_hash: text(b, "facts_hash"),
            action_id: text(b, "action_id"),
            match_key_bytes: serde_json::to_vec(&b["match_key"]).unwrap(),
        },
        "action" => JournalRow::Action {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            decision_id: text(b, "decision_id"),
            effect_id: text(b, "effect_id"),
            obligation_id: text(b, "obligation_id"),
            kind: text(b, "kind"),
            book: text(b, "book"),
            component: text(b, "component"),
            binding_id: text(b, "binding_id"),
            snapshot_doc: text(b, "snapshot_doc"),
            roles_doc: text(b, "roles_doc"),
            currency: text(&b["amount"], "currency"),
            scale: number(&b["amount"], "scale"),
            atoms: text(&b["amount"], "atoms"),
            reverses: b.get("reverses").map(|x| x.as_str().unwrap().to_owned()),
            allocation_parent: b
                .get("allocation_parent")
                .map(|x| x.as_str().unwrap().to_owned()),
        },
        "action-source" => JournalRow::ActionSource {
            action_id: text(b, "action_id"),
            event_id: text(b, "event_id"),
        },
        "action-dependency" => JournalRow::ActionDependency {
            action_id: text(b, "action_id"),
            input_action_id: text(b, "input_action_id"),
        },
        "explanation" => JournalRow::Explanation {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            ordinal: number(b, "ordinal"),
            code: text(b, "code"),
            rule_id: b.get("rule_id").map(|x| x.as_str().unwrap().to_owned()),
        },
        "intention" => JournalRow::Intention {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            obligation_id: text(b, "obligation_id"),
            destination_id: text(b, "destination_id"),
            idempotency_key: text(b, "idempotency_key"),
        },
        "control-transition" => JournalRow::ControlTransition {
            id: text(v, "id"),
            control_kind: text(b, "control_kind"),
            control_id: text(b, "control_id"),
            from_revision: number(b, "from_revision"),
            to_revision: number(b, "to_revision"),
            from_event_count: number(b, "from_event_count"),
            to_event_count: number(b, "to_event_count"),
            event_id: text(b, "event_id"),
            document_id: text(b, "document_id"),
        },
        "chain-revision" => JournalRow::ChainRevision {
            chain_id: text(b, "chain_id"),
            revision: number(b, "revision"),
            event_id: text(b, "event_id"),
            decision_id: text(b, "decision_id"),
        },
        "decision-manifest" => JournalRow::Manifest {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            chain_id: text(b, "chain_id"),
            revision: number(b, "revision"),
            decision_hash: text(v, "content_hash"),
        },
        "receipt" => JournalRow::Receipt {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            decision_id: text(b, "decision_id"),
        },
        k => panic!("unmapped fixture {k}"),
    };
    JournalRecord {
        scope: scope(),
        canonical: CanonicalRecord {
            canonical_bytes: serde_json::to_vec(b).unwrap(),
            content_hash: text(v, "content_hash"),
        },
        row,
    }
}
pub(crate) fn installation() -> Installation {
    Installation {
        scope: scope(),
        logical_store_id: "store-demo-slice".into(),
        mode: "sandbox".into(),
        admission: "open".into(),
        dispatch_hold: true,
        dispatch_enabled: false,
        generation: 0,
    }
}
fn state() -> Value {
    serde_json::from_str(STATE).unwrap()
}
pub(crate) fn seed() -> Vec<WriteOp> {
    let mut ops: Vec<_> = values(SEED_DOCS)
        .iter()
        .chain(values(SEED_ROWS).iter())
        .map(|r| WriteOp::Journal(Box::new(row(r))))
        .collect();
    let s = state();
    let c = &s["chain"];
    let a = &s["authority_head"];
    let b = &s["binding_head"];
    ops.push(WriteOp::SeedChain(Chain {
        scope: scope(),
        id: text(c, "id"),
        customer: text(c, "customer"),
        currency: text(c, "currency"),
        scale: number(c, "scale"),
        binding_set_doc: text(c, "binding_set_doc"),
        context_doc: text(c, "context_doc"),
        revision: 0,
        event_count: 0,
    }));
    ops.push(WriteOp::SeedAuthority(AuthorityHead {
        scope: scope(),
        id: text(a, "id"),
        grant_id: text(a, "grant_id"),
        revision: 1,
        active: true,
    }));
    ops.push(WriteOp::SeedBinding(BindingHead {
        scope: scope(),
        id: text(b, "id"),
        selector_doc: text(b, "selector_doc"),
        binding_id: text(b, "binding_id"),
        revision: 1,
        active: true,
    }));
    ops
}
pub(crate) fn schedule() -> Vec<WriteOp> {
    let all = accepted();
    let mut ops = Vec::new();
    for kind in [
        "document",
        "snapshot-ref",
        "event",
        "delivery-key",
        "claim",
        "effect",
        "action",
        "action-source",
        "action-dependency",
        "explanation",
        "intention",
        "held",
        "control-transition",
        "head",
        "chain-revision",
        "decision-manifest",
        "receipt",
    ] {
        match kind {
            "held" => ops.push(WriteOp::HoldDelivery(HeldDelivery {
                scope: scope(),
                intention_id: text(&one("intention"), "id"),
                next_attempt_us: T,
            })),
            "head" => ops.push(WriteOp::AdvanceChain(ChainAdvance {
                scope: scope(),
                id: "demo-slice".into(),
                from_revision: 0,
                to_revision: 1,
                from_event_count: 0,
                to_event_count: 1,
            })),
            _ => ops.extend(
                all.iter()
                    .filter(|r| r["kind"] == kind)
                    .map(|r| WriteOp::Journal(Box::new(row(r)))),
            ),
        }
    }
    assert_eq!(ops.len(), 27);
    ops
}
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}
async fn fresh() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), installation())
        .await
        .unwrap();
    let mut tx = store.begin(deadline()).await.unwrap();
    for op in seed() {
        tx.write(&op).await.unwrap();
    }
    tx.commit().await.unwrap();
    (dir, store)
}
async fn append(store: &SqliteStore) {
    let mut tx = store.begin(deadline()).await.unwrap();
    for op in schedule() {
        tx.write(&op).await.unwrap();
    }
    tx.commit().await.unwrap();
}
async fn dump(store: &SqliteStore) -> Vec<(String, Vec<String>)> {
    let mut conn = store.inner.readers.acquire().await.unwrap();
    let tables: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_schema WHERE type='table' ORDER BY name")
            .fetch_all(&mut *conn)
            .await
            .unwrap();
    let mut result = vec![];
    for table in tables {
        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(&table)
                .fetch_all(&mut *conn)
                .await
                .unwrap();
        let expression = cols
            .iter()
            .map(|c| format!("quote({c})"))
            .collect::<Vec<_>>()
            .join("||'|'||");
        let sql = format!("SELECT {expression} FROM {table} ORDER BY 1");
        let rows = sqlx::query_scalar(AssertSqlSafe(sql))
            .fetch_all(&mut *conn)
            .await
            .unwrap();
        result.push((table, rows));
    }
    result
}
async fn assert_complete(store: &SqliteStore) {
    let found = store
        .lookup_identity(&scope(), "urn:demo:app", "generation-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(found.receipt.canonical_bytes, RECEIPT);
    let mut tx = store.begin(deadline()).await.unwrap();
    let chain = tx
        .load_chain(&scope(), "demo-slice")
        .await
        .unwrap()
        .unwrap();
    assert_eq!((chain.revision, chain.event_count), (1, 1));
    let delivery = tx
        .load_delivery(&scope(), &text(&one("intention"), "id"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            delivery.state.as_str(),
            delivery.attempts,
            delivery.generation,
            delivery.next_attempt_us
        ),
        ("held", 0, 0, T)
    );
    assert_eq!(
        (
            delivery.lease_owner,
            delivery.lease_until_us,
            delivery.last_observation
        ),
        (None, None, None)
    );
    assert_eq!(
        tx.load_claim(
            &scope(),
            "urn:demo:app",
            "generation-1",
            "completion",
            "completion"
        )
        .await
        .unwrap()
        .unwrap()
        .receipt
        .canonical_bytes,
        RECEIPT
    );
    tx.rollback().await.unwrap();
    let mut expected = Vec::new();
    let mut actual = Vec::new();
    for v in values(SEED_DOCS)
        .iter()
        .chain(values(SEED_ROWS).iter())
        .chain(accepted().iter())
    {
        expected.push((row(v).canonical.canonical_bytes, text(v, "content_hash")));
    }
    let mut c = store.inner.readers.acquire().await.unwrap();
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM documents",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>("SELECT canonical_bytes,content_hash FROM parties")
            .fetch_all(&mut *c)
            .await
            .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM source_grants",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>("SELECT canonical_bytes,content_hash FROM bindings")
            .fetch_all(&mut *c)
            .await
            .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>("SELECT canonical_bytes,content_hash FROM events")
            .fetch_all(&mut *c)
            .await
            .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM snapshots",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM delivery_keys",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>("SELECT canonical_bytes,content_hash FROM claims")
            .fetch_all(&mut *c)
            .await
            .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>("SELECT canonical_bytes,content_hash FROM effects")
            .fetch_all(&mut *c)
            .await
            .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>("SELECT canonical_bytes,content_hash FROM actions")
            .fetch_all(&mut *c)
            .await
            .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM action_sources",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM action_dependencies",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM explanations",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM intentions",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM control_transitions",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM chain_revisions",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM decision_manifests",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    actual.extend(
        sqlx::query_as::<_, (Vec<u8>, String)>(
            "SELECT canonical_bytes,content_hash FROM accepted_receipts",
        )
        .fetch_all(&mut *c)
        .await
        .unwrap(),
    );
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
    let atoms: Vec<String> = sqlx::query_scalar("SELECT atoms FROM actions ORDER BY id")
        .fetch_all(&mut *c)
        .await
        .unwrap();
    assert_eq!(
        atoms
            .iter()
            .map(|a| a.parse::<i128>().unwrap())
            .sum::<i128>(),
        80
    );
}

#[tokio::test]
async fn linked_sqlite_reopen_and_exact_journal() {
    let (dir, store) = fresh().await;
    eprintln!(
        "Linked SQLite {} source {} options {:?}",
        store.diagnostics.version, store.diagnostics.source_id, store.diagnostics.compile_options
    );
    assert_eq!(store.diagnostics.version, "3.51.3");
    append(&store).await;
    assert_complete(&store).await;
    store.close().await;
    let store = SqliteStore::open(dir.path()).await.unwrap();
    assert_complete(&store).await;
    store.close().await;
}

#[tokio::test]
async fn rollback_at_every_write_boundary_and_reopen() {
    for cut in 0..=schedule().len() {
        let (dir, store) = fresh().await;
        let before = dump(&store).await;
        let mut tx = store.begin(deadline()).await.unwrap();
        for op in schedule().iter().take(cut) {
            tx.write(op).await.unwrap();
        }
        tx.rollback().await.unwrap();
        store.close().await;
        let reopened = SqliteStore::open(dir.path()).await.unwrap();
        assert_eq!(dump(&reopened).await, before, "write boundary {cut}");
        reopened.close().await;
    }
}

#[tokio::test]
async fn dropped_transaction_at_every_boundary_is_clean_for_next_borrower() {
    for cut in 0..=schedule().len() {
        let (_dir, store) = fresh().await;
        let before = dump(&store).await;
        let mut tx = store.begin(deadline()).await.unwrap();
        for op in schedule().iter().take(cut) {
            tx.write(op).await.unwrap();
        }
        drop(tx);
        let next = store.begin(deadline()).await.unwrap();
        next.rollback().await.unwrap();
        assert_eq!(dump(&store).await, before, "drop boundary {cut}");
        store.close().await;
    }
}

fn poll_once<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}
#[tokio::test]
async fn cancel_each_write_future_poisoned_and_rolled_back() {
    for cut in 0..schedule().len() {
        let (_dir, store) = fresh().await;
        let before = dump(&store).await;
        let mut tx = store.begin(deadline()).await.unwrap();
        let ops = schedule();
        for op in ops.iter().take(cut) {
            tx.write(op).await.unwrap();
        }
        let mut writing = Box::pin(tx.write(&ops[cut]));
        let poll = poll_once(writing.as_mut());
        drop(writing);
        // A ready write is the corresponding after-boundary; a pending write is
        // actual future cancellation, even if the worker later executes it.
        if poll.is_pending() {
            assert!(tx.failed);
        }
        tx.rollback().await.unwrap();
        let next = store.begin(deadline()).await.unwrap();
        next.rollback().await.unwrap();
        assert_eq!(dump(&store).await, before, "cancel write {cut}");
        store.close().await;
    }
}

#[tokio::test]
async fn owner_guard_and_reader_settings() {
    let (dir, store) = fresh().await;
    assert!(matches!(
        SqliteStore::open(dir.path()).await,
        Err(StoreError::Owned)
    ));
    let mut reader = store.inner.readers.acquire().await.unwrap();
    connect::verify(&mut reader, true).await.unwrap();
    assert!(sqlx::query("DELETE FROM installation")
        .execute(&mut *reader)
        .await
        .is_err());
    drop(reader);
    let mut writer = store.inner.writer.acquire().await.unwrap();
    connect::verify(&mut writer, false).await.unwrap();
    drop(writer);
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    reopened.close().await;
}

#[tokio::test]
async fn seed_rollback_boundaries_preserve_empty_installation() {
    for cut in 0..=seed().len() {
        let dir = tempfile::tempdir().unwrap();
        let store = SqliteStore::create(dir.path(), installation())
            .await
            .unwrap();
        let before = dump(&store).await;
        let mut tx = store.begin(deadline()).await.unwrap();
        for op in seed().iter().take(cut) {
            tx.write(op).await.unwrap();
        }
        drop(tx);
        let next = store.begin(deadline()).await.unwrap();
        next.rollback().await.unwrap();
        assert_eq!(dump(&store).await, before, "seed boundary {cut}");
        store.close().await;
    }
}

#[tokio::test]
async fn immutable_guards_cover_every_journal_table() {
    let (_dir, store) = fresh().await;
    append(&store).await;
    let ledger = crate::Ledger {
        store: crate::Backend::Sqlite(store.clone()),
    };
    crate::outbox::tests::exercise_evidence(&ledger).await;
    drop(ledger);
    let before = dump(&store).await;
    let tables:Vec<String>=sqlx::query_scalar("SELECT DISTINCT tbl_name FROM sqlite_schema WHERE type='trigger' AND name LIKE '%_no_update' ORDER BY tbl_name").fetch_all(&store.inner.readers).await.unwrap();
    assert_eq!(tables.len(), 21);
    for table in tables {
        for sql in [
            format!("UPDATE {table} SET canonical_bytes=canonical_bytes"),
            format!("DELETE FROM {table}"),
        ] {
            let mut tx = store.begin(deadline()).await.unwrap();
            let e = sqlx::query(AssertSqlSafe(sql))
                .execute(&mut **tx.transaction.as_mut().unwrap())
                .await
                .unwrap_err();
            assert!(e.to_string().contains("IMMUTABLE_RECORD"));
            tx.rollback().await.unwrap();
        }
    }
    assert_eq!(dump(&store).await, before);
    store.close().await;
}

#[tokio::test]
async fn unique_delivery_claim_effect_intention_primitives_and_original_receipt() {
    let (_dir, store) = fresh().await;
    append(&store).await;
    let before = dump(&store).await;
    for kind in [
        "delivery-key",
        "claim",
        "effect",
        "action",
        "intention",
        "receipt",
        "chain-revision",
    ] {
        let mut r = row(&one(kind));
        // Change the generated PK where present, so the semantic unique key is exercised.
        match &mut r.row {
            JournalRow::Claim { id, .. } => *id = format!("cl_{}", "0".repeat(64)),
            JournalRow::Effect { id, .. } => *id = format!("ef_{}", "0".repeat(64)),
            JournalRow::Action { id, .. } => *id = format!("ac_{}", "0".repeat(64)),
            JournalRow::Intention { id, .. } => *id = format!("in_{}", "0".repeat(64)),
            JournalRow::Receipt { id, .. } => *id = format!("rc_{}", "0".repeat(64)),
            _ => {}
        }
        let mut tx = store.begin(deadline()).await.unwrap();
        let e = tx.write(&WriteOp::Journal(Box::new(r))).await.unwrap_err();
        assert!(
            matches!(&e,StoreError::Database(sqlx::Error::Database(d)) if d.is_unique_violation()),
            "{kind}: {e:?}"
        );
        assert!(matches!(
            tx.commit().await,
            Err(crate::store::errors::CommitError::RolledBack(_))
        ));
        assert_eq!(dump(&store).await, before, "unique {kind}");
    }
    assert_complete(&store).await;
    store.close().await;
}

#[tokio::test]
async fn deferred_missing_reference_commit_is_unknown_and_reopen_has_no_residue() {
    let (dir, store) = fresh().await;
    let before = dump(&store).await;
    let mut tx = store.begin(deadline()).await.unwrap();
    // Snapshot associations can be written before their event; missing event at
    // commit must fail. The boundary conservatively reports unknown after send.
    tx.write(&WriteOp::Journal(Box::new(row(&one("snapshot-ref")))))
        .await
        .unwrap();
    assert!(matches!(
        tx.commit().await,
        Err(crate::store::errors::CommitError::OutcomeUnknown)
    ));
    // Production ambiguity must close/drain the writer before returning unknown,
    // independently of the service test adapter's later close/reopen protocol.
    assert!(store.test_writer_closed());
    assert!(matches!(
        store.test_pool_probe().await,
        Err(StoreError::Database(sqlx::Error::PoolClosed))
    ));
    assert!(matches!(
        store.begin(deadline()).await,
        Err(StoreError::WritesDisabled)
    ));
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    assert_eq!(dump(&reopened).await, before);
    reopened.close().await;
}

#[tokio::test]
async fn cancelled_commit_reply_drains_to_one_complete_journal() {
    let (dir, store) = fresh().await;
    let mut tx = store.begin(deadline()).await.unwrap();
    for op in schedule() {
        tx.write(&op).await.unwrap();
    }
    let mut commit = Box::pin(tx.commit());
    assert!(poll_once(commit.as_mut()).is_pending());
    drop(commit);
    // The request future is gone, but close awaits the registered bounded commit.
    store.close().await;
    let reopened = SqliteStore::open(dir.path()).await.unwrap();
    assert_complete(&reopened).await;
    reopened.close().await;
}

#[tokio::test]
async fn cancelled_begin_and_rollback_do_not_leak_transaction() {
    let (_dir, store) = fresh().await;
    let before = dump(&store).await;
    for _ in 0..20 {
        let mut begin = Box::pin(store.begin(deadline()));
        let result = poll_once(begin.as_mut());
        drop(begin);
        if let Poll::Ready(Ok(tx)) = result {
            drop(tx);
        }
        let mut tx = store.begin(deadline()).await.unwrap();
        tx.write(&schedule()[0]).await.unwrap();
        let mut rollback = Box::pin(tx.rollback());
        let _ = poll_once(rollback.as_mut());
        drop(rollback);
        let next = store.begin(deadline()).await.unwrap();
        next.rollback().await.unwrap();
        assert_eq!(dump(&store).await, before);
    }
    store.close().await;
}

#[tokio::test]
async fn immediate_lock_contention_and_queue_deadline() {
    let (_dir, store) = fresh().await;
    let mut outsider = connect::initial(&store.inner._owner).await.unwrap();
    let outside_tx = outsider.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let result = store
        .begin(Instant::now() + Duration::from_millis(20))
        .await;
    assert!(matches!(result, Err(StoreError::Deadline)));
    outside_tx.rollback().await.unwrap();
    let tx = store.begin(deadline()).await.unwrap();
    assert!(matches!(
        store
            .begin(Instant::now() + Duration::from_millis(10))
            .await,
        Err(StoreError::Deadline)
    ));
    tx.rollback().await.unwrap();
    outsider.close().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn cross_scope_references_strict_types_atoms_and_schema_checks() {
    let (dir, store) = fresh().await;
    let before = dump(&store).await;
    let mut tx = store.begin(deadline()).await.unwrap();
    let mut r = row(&one("snapshot-ref"));
    r.scope.tenant = "other".into();
    tx.write(&WriteOp::Journal(Box::new(r))).await.unwrap();
    assert!(tx.commit().await.is_err());
    store.close().await;
    let store = SqliteStore::open(dir.path()).await.unwrap();
    assert_eq!(dump(&store).await, before);
    append(&store).await;
    for atoms in [
        "0",
        "-0",
        "01",
        "-01",
        "1.2",
        "1e2",
        "1000000000000000000000000000000",
    ] {
        let mut r = row(&one("action"));
        if let JournalRow::Action {
            id,
            atoms: a,
            effect_id,
            ..
        } = &mut r.row
        {
            *id = format!("ac_{}", "0".repeat(64));
            *effect_id = format!("ef_{}", "1".repeat(64));
            *a = atoms.into();
        }
        let mut tx = store.begin(deadline()).await.unwrap();
        let e = tx.write(&WriteOp::Journal(Box::new(r))).await.unwrap_err();
        assert!(
            e.to_string().contains("CHECK constraint failed"),
            "{atoms}: {e:?}"
        );
        tx.rollback().await.unwrap();
    }
    store.close().await;
    let owner = owner::Owner::acquire(dir.path()).unwrap();
    let mut conn = connect::initial(&owner).await.unwrap();
    sqlx::query("PRAGMA user_version=3")
        .execute(&mut conn)
        .await
        .unwrap();
    conn.close().await.unwrap();
    drop(owner);
    assert!(matches!(
        SqliteStore::open(dir.path()).await,
        Err(StoreError::InvalidStore("unsupported SQLite write schema"))
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn symlink_storage_is_rejected() {
    use std::os::unix::fs::symlink;
    let (dir, store) = fresh().await;
    store.close().await;
    let parent = tempfile::tempdir().unwrap();
    let link = parent.path().join("linked");
    symlink(dir.path(), &link).unwrap();
    assert!(matches!(
        SqliteStore::open(&link).await,
        Err(StoreError::InvalidStore(_))
    ));
    let second = tempfile::tempdir().unwrap();
    symlink(dir.path().join("local.db"), second.path().join("local.db")).unwrap();
    assert!(matches!(
        SqliteStore::open(second.path()).await,
        Err(StoreError::InvalidStore(_))
    ));
}

#[tokio::test]
async fn cancel_each_typed_read_and_deadline_prevents_partial_commit() {
    let (_dir, store) = fresh().await;
    let before = dump(&store).await;
    for which in 0..8 {
        let mut tx = store.begin(deadline()).await.unwrap();
        tx.write(&schedule()[0]).await.unwrap();
        let s = scope();
        let id = text(&one("document"), "id");
        let intention = text(&one("intention"), "id");
        let mut future: Pin<Box<dyn Future<Output = ()> + '_>> = Box::pin(async {
            match which {
                0 => {
                    let _ = tx.load_installation().await;
                }
                1 => {
                    let _ = tx.load_chain(&s, "demo-slice").await;
                }
                2 => {
                    let _ = tx.load_document(&s, &id).await;
                }
                3 => {
                    let _ = tx.load_authority(&s, "demo-source-grant-v1").await;
                }
                4 => {
                    let _ = tx.load_binding(&s, "demo-retail-selector").await;
                }
                5 => {
                    let _ = tx.load_identity(&s, "urn:demo:app", "generation-1").await;
                }
                6 => {
                    let _ = tx
                        .load_claim(
                            &s,
                            "urn:demo:app",
                            "generation-1",
                            "completion",
                            "completion",
                        )
                        .await;
                }
                _ => {
                    let _ = tx.load_delivery(&s, &intention).await;
                }
            }
        });
        let pending = future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending();
        drop(future);
        if pending {
            assert!(matches!(
                tx.commit().await,
                Err(crate::store::errors::CommitError::RolledBack(_))
            ));
        } else {
            tx.rollback().await.unwrap();
        }
        assert_eq!(dump(&store).await, before);
    }
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.write(&schedule()[0]).await.unwrap();
    tx.deadline = Instant::now();
    assert!(tx.commit().await.is_err());
    assert_eq!(dump(&store).await, before);
    store.close().await;
}

#[tokio::test]
async fn concurrent_identity_check_and_complete_writes_have_one_winner() {
    let (_dir, store) = fresh().await;
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..12 {
        let store = store.clone();
        tasks.spawn(async move {
            let mut tx = store.begin(deadline()).await.unwrap();
            if let Some(found) = tx
                .load_identity(&scope(), "urn:demo:app", "generation-1")
                .await
                .unwrap()
            {
                assert_eq!(found.receipt.canonical_bytes, RECEIPT);
                tx.rollback().await.unwrap();
                false
            } else {
                for op in schedule() {
                    tx.write(&op).await.unwrap();
                }
                tx.commit().await.unwrap();
                true
            }
        });
    }
    let mut winners = 0;
    while let Some(r) = tasks.join_next().await {
        winners += usize::from(r.unwrap());
    }
    assert_eq!(winners, 1);
    assert_complete(&store).await;
    store.close().await;
}

#[tokio::test]
async fn owner_lives_until_outstanding_transaction_finishes() {
    let (dir, store) = fresh().await;
    let tx = store.begin(deadline()).await.unwrap();
    drop(store);
    assert!(matches!(
        SqliteStore::open(dir.path()).await,
        Err(StoreError::Owned)
    ));
    tx.rollback().await.unwrap();
    // Closing idle pools is asynchronous in SQLx; retry only ownership startup.
    let reopened = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match SqliteStore::open(dir.path()).await {
                Ok(store) => break store,
                Err(StoreError::Owned) => tokio::task::yield_now().await,
                Err(e) => panic!("reopen: {e:?}"),
            }
        }
    })
    .await
    .unwrap();
    reopened.close().await;
}

#[test]
fn owner_probe_child() {
    let Some(path) = std::env::var_os("LEDGERLAB_TEST_OWNER_PATH") else {
        return;
    };
    assert!(matches!(
        owner::Owner::acquire(Path::new(&path)),
        Err(StoreError::Owned)
    ));
}

#[tokio::test]
async fn owner_os_lock_excludes_another_process() {
    let (dir, store) = fresh().await;
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "store::sqlite::tests::owner_probe_child",
            "--nocapture",
        ])
        .env("LEDGERLAB_TEST_OWNER_PATH", dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    store.close().await;
}

#[tokio::test]
async fn bounded_writer_queue_and_strict_operational_types() {
    let (_dir, store) = fresh().await;
    let permits = Arc::clone(&store.inner.queue)
        .acquire_many_owned(65)
        .await
        .unwrap();
    assert!(matches!(
        store.begin(deadline()).await,
        Err(StoreError::Overloaded)
    ));
    drop(permits);
    let mut tx = store.begin(deadline()).await.unwrap();
    let e = sqlx::query("UPDATE chains SET event_count='not-an-integer'")
        .execute(&mut **tx.transaction.as_mut().unwrap())
        .await
        .unwrap_err();
    assert!(e
        .to_string()
        .contains("cannot store TEXT value in INTEGER column"));
    tx.rollback().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn driver_commit_future_cancel_and_hard_close_recovers_none_or_complete() {
    for _ in 0..12 {
        let (dir, store) = fresh().await;
        let before = dump(&store).await;
        store.close().await;
        let owner = owner::Owner::acquire(dir.path()).unwrap();
        let mut connection = connect::initial(&owner).await.unwrap();
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await.unwrap();
        for operation in schedule() {
            write::operation(&mut transaction, &operation)
                .await
                .unwrap();
        }
        // Exercise the driver itself: abandon COMMIT acknowledgement, then drop
        // the connection without a graceful SQLx shutdown handshake.
        let mut commit = Box::pin(transaction.commit());
        let _ = poll_once(commit.as_mut());
        drop(commit);
        connection.close_hard().await.unwrap();
        // Absence is not rollback proof until this replacement writer has passed
        // the original writer's SQLite lock and verified the recovered database.
        let mut replacement = connect::initial(&owner).await.unwrap();
        replacement
            .begin_with("BEGIN IMMEDIATE")
            .await
            .unwrap()
            .rollback()
            .await
            .unwrap();
        connect::integrity(&mut replacement).await.unwrap();
        replacement.close().await.unwrap();
        drop(owner);
        let reopened = SqliteStore::open(dir.path()).await.unwrap();
        match reopened
            .lookup_identity(&scope(), "urn:demo:app", "generation-1")
            .await
            .unwrap()
        {
            Some(_) => assert_complete(&reopened).await,
            None => assert_eq!(dump(&reopened).await, before),
        }
        reopened.close().await;
    }
}
