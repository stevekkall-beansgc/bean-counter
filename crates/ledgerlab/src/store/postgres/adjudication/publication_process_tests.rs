//! Actual SIGKILL around the real publication supervisor. Native Primitive
//! verifies storage atomicity only; this does not assert economic acceptance,
//! physical backing, or an enforced PostgreSQL CommitCapability.
use super::*;
use crate::store::{
    postgres::adjudication::publication::read_witness,
    records::{JournalRow, WriteOp},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};
const CHILD: &str =
    "store::postgres::adjudication::tests::publication_process_tests::publication_process_child";
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn parsed(s: &wire::Segment) -> r3::ParsedCommand {
    r3::ParsedCommand::parse(&r3::canonical_bytes(&s.command, r3::COMMAND_BYTES).unwrap()).unwrap()
}
fn record(anchor: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(anchor.join("state.json")).unwrap()).unwrap()
}
fn document() -> WriteOp {
    crate::store::sqlite::tests::seed().remove(0)
}
async fn read_document(store: &PostgresStore) {
    let WriteOp::Journal(row) = document() else {
        panic!("document fixture")
    };
    let JournalRow::Document { id, kind } = &row.row else {
        panic!("document fixture")
    };
    let mut tx = store.begin(deadline()).await.unwrap();
    assert_eq!(
        tx.load_document(&row.scope, id).await.unwrap(),
        Some((kind.clone(), row.canonical.clone()))
    );
    tx.commit().await.unwrap();
}
async fn native_read(store: &PostgresStore, present: bool) {
    let (j, s) = sample();
    let command = parsed(&s);
    let mut tx = store
        .begin_native_storage(j.clone(), &command, deadline())
        .await
        .unwrap();
    tx.native_adjudication(Operation::Locks(j.clone(), guards(&j)))
        .await
        .unwrap();
    let Value::Head(head) = tx
        .native_adjudication(Operation::Head(j.clone()))
        .await
        .unwrap()
    else {
        panic!("head")
    };
    let key: wire::Delivery =
        serde_json::from_value(runtime::command_value(&s.command).unwrap()["key"].clone()).unwrap();
    let Value::Saved(saved) = tx
        .native_adjudication(Operation::Lookup(j.clone(), key))
        .await
        .unwrap()
    else {
        panic!("saved")
    };
    if present {
        assert_eq!(head.ordinal(), s.ordinal);
        assert_eq!(head.segment(), &runtime::hash("segment", &s).unwrap());
        assert_eq!(head.root(), &s.result.root);
        let saved = saved.unwrap();
        assert_eq!(saved.command, command.bytes());
        assert_eq!(saved.result, s.result);
        let object = s
            .objects
            .iter()
            .find(|o| o.kind == wire::FactKind::Enrollment)
            .unwrap();
        let full_key: wire::ProofFullKey = serde_json::from_value(json!(object.full_key)).unwrap();
        let Value::Source(source) = tx
            .native_adjudication(Operation::Source(
                j,
                s.ordinal,
                object.kind.clone(),
                full_key,
            ))
            .await
            .unwrap()
        else {
            panic!("source")
        };
        assert_eq!(source.object(), object);
    } else {
        assert_eq!(head.ordinal(), Count::ZERO);
        assert!(saved.is_none());
    }
    // Existing saved commands and guards are read-only; no publication nonce.
    tx.rollback().await.unwrap();
}
async fn write_one(store: &PostgresStore, native: bool) {
    if native {
        let (j, s) = sample();
        let command = parsed(&s);
        let mut tx = store
            .begin_native_storage(j.clone(), &command, deadline())
            .await
            .unwrap();
        tx.native_adjudication(Operation::Locks(j.clone(), guards(&j)))
            .await
            .unwrap();
        tx.native_adjudication(Operation::Primitive(Box::new(Primitive {
            journal: j,
            segment: s,
            writes: vec![],
            fail_after_segment: false,
        })))
        .await
        .unwrap();
        tx.commit().await.unwrap();
    } else {
        let mut tx = store.begin(deadline()).await.unwrap();
        tx.write(&document()).await.unwrap();
        tx.commit().await.unwrap();
    }
}
// All exact authoritative/native projection rows, plus the fixed operational
// slot. Inspection may count tables; no production transaction scans history.
async fn inventory<C: GenericClient + Sync>(c: &C) -> serde_json::Value {
    let mut result = serde_json::Map::new();
    for table in [
        "documents",
        "outcome_scope_locks",
        "r3_commit_witness",
        "r3_unresolved_work",
        "r3_scope_locks",
        "r3_journals",
        "r3_segments",
        "r3_segment_pages",
        "r3_objects",
        "r3_object_pages",
        "r3_commands",
        "r3_heads",
        "r3_head_versions",
        "r3_namespaces",
        "r3_deliveries",
        "r3_index_pages",
        "r3_index_roots",
        "r3_held_intentions",
    ] {
        let rows = c.query(&format!("SELECT encode(sha256(convert_to(row_to_json(t)::text,'UTF8')),'hex') FROM ledgerlab.{table} t ORDER BY 1"), &[]).await.unwrap();
        result.insert(
            table.into(),
            json!(rows
                .iter()
                .map(|r| r.get::<_, String>(0))
                .collect::<Vec<_>>()),
        );
    }
    serde_json::Value::Object(result)
}
async fn primary<C: GenericClient + Sync>(c: &C) -> serde_json::Value {
    let row = c.query_one("SELECT anchor,witness,(SELECT count(*) FROM ledgerlab.documents),(SELECT count(*) FROM ledgerlab.r3_segments),(SELECT count(*) FROM ledgerlab.r3_commands),(SELECT state FROM ledgerlab.r3_unresolved_work),(SELECT encode(generation,'hex') FROM ledgerlab.r3_unresolved_work) FROM ledgerlab.r3_commit_witness WHERE singleton=1", &[]).await.unwrap();
    json!({"anchor":row.get::<_,String>(0),"witness":row.get::<_,String>(1),"documents":row.get::<_,i64>(2),"segments":row.get::<_,i64>(3),"commands":row.get::<_,i64>(4),"slot":row.get::<_,String>(5),"generation":row.get::<_,String>(6)})
}
struct Killer(Child);
impl Drop for Killer {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
async fn ready(child: &mut Killer, path: &Path, phase: u8) {
    let until = Instant::now() + Duration::from_secs(45);
    loop {
        if let Ok(value) = fs::read_to_string(path) {
            if value == format!("{}:{phase}", child.0.id()) {
                return;
            }
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before exact phase {phase}; retained child log"
        );
        assert!(
            Instant::now() < until,
            "child failed to rendezvous at phase {phase}; retained fixture/log"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}
#[tokio::test]
#[ignore = "subprocess helper; invoked only by actual publication cut parent"]
async fn publication_process_child() {
    let name = std::env::var("LEDGERLAB_PG_CUT_DATABASE").unwrap();
    let anchor = PathBuf::from(std::env::var_os("LEDGERLAB_PG_CUT_ANCHOR").unwrap());
    let native = std::env::var("LEDGERLAB_PG_CUT_MODE").unwrap() == "native";
    let store = PostgresStore::open_fenced(config(&name, false), &anchor)
        .await
        .unwrap();
    write_one(&store, native).await;
    // This phase proves the caller received the actual successful commit reply.
    super::super::super::trace::publication_cut(4).await;
    panic!("selected process cut did not hold child");
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL17/18 TLS; eight real process-kill histories"]
async fn postgres_publication_eight_process_cuts() {
    let root = PathBuf::from(
        std::env::var_os("LEDGERLAB_PG_CUT_EVIDENCE").expect("retained evidence directory"),
    );
    fs::create_dir_all(&root).unwrap();
    for native in [false, true] {
        for phase in 1..=4_u8 {
            let mode = if native { "native" } else { "legacy" };
            let dir = root.join(format!("{mode}-{phase}-{}", std::process::id()));
            fs::create_dir(&dir).unwrap();
            let anchor = dir.join("anchor");
            fs::create_dir(&anchor).unwrap();
            let Fixture { name, owner, store } = Fixture::new().await;
            // Provision only before binding; this never inserts accepted rows.
            store.close().await;
            owner
                .client
                .execute(
                    "UPDATE ledgerlab.installation SET admission='frozen' WHERE singleton=1",
                    &[],
                )
                .await
                .unwrap();
            fs::write(dir.join("fixture.json"),serde_json::to_vec_pretty(&json!({"database":name,"mode":mode,"phase":phase,"anchor":anchor,"major":std::env::var("LEDGERLAB_PG_TEST_MAJOR").unwrap(),"scope":"storage publication, no economic or physical claim"})).unwrap()).unwrap();
            eprintln!(
                "PG_PUBLICATION_FIXTURE database={name} mode={mode} phase={phase} anchor={}",
                anchor.display()
            );
            let publication =
                super::super::super::bootstrap::bind(config(&name, true), &anchor, "center")
                    .await
                    .unwrap();
            let original = read_witness(&owner.client).await.unwrap();
            let before = record(&anchor);
            drop(publication);
            let log = fs::File::create(dir.join("child.log")).unwrap();
            let ready_path = dir.join("ready");
            let mut child = Killer(
                Command::new(std::env::current_exe().unwrap())
                    .args(["--ignored", "--exact", CHILD, "--nocapture"])
                    .env("LEDGERLAB_PG_PUBLICATION_CUT", phase.to_string())
                    .env("LEDGERLAB_PG_PUBLICATION_READY", &ready_path)
                    .env("LEDGERLAB_PG_CUT_DATABASE", &name)
                    .env("LEDGERLAB_PG_CUT_ANCHOR", &anchor)
                    .env("LEDGERLAB_PG_CUT_MODE", mode)
                    .stdout(Stdio::from(log.try_clone().unwrap()))
                    .stderr(Stdio::from(log))
                    .spawn()
                    .unwrap(),
            );
            ready(&mut child, &ready_path, phase).await;
            // Kill immediately at the synced exact boundary. No configuration
            // changes or advisory sleep are used to fabricate a successful cut.
            assert!(Command::new("/bin/kill")
                .args(["-KILL", &child.0.id().to_string()])
                .status()
                .unwrap()
                .success());
            let status = child.0.wait().unwrap();
            use std::os::unix::process::ExitStatusExt;
            assert_eq!(status.signal(), Some(9));
            let interrupted = record(&anchor);
            assert_eq!(
                interrupted["publication"]["state"],
                if phase <= 2 { "Pending" } else { "Stable" }
            );
            let reopened = PostgresStore::open_fenced(config(&name, false), &anchor)
                .await
                .unwrap();
            let after = primary(&owner.client).await;
            assert_eq!(after["anchor"], original.anchor().as_str());
            assert_eq!(after["slot"], "IDLE");
            assert_eq!(after["generation"], "00".repeat(16));
            let present = phase >= 2;
            assert_eq!(after["documents"], if !native && present { 1 } else { 0 });
            assert_eq!(after["segments"], if native && present { 1 } else { 0 });
            assert_eq!(after["commands"], if native && present { 1 } else { 0 });
            let recovered = record(&anchor);
            assert_eq!(recovered["publication"]["state"], "Stable");
            assert_eq!(recovered["publication"]["witness"], after["witness"]);
            if phase == 1 {
                assert_eq!(recovered, before);
                assert_eq!(after["witness"], original.witness().as_str());
                // The original operation remains executable after the absent
                // cut; its retry is one real commit, never a fabricated Saved.
                if native {
                    native_read(&reopened, false).await;
                }
                write_one(&reopened, native).await;
            } else {
                assert_ne!(after["witness"], original.witness().as_str());
                if phase == 2 {
                    assert_eq!(interrupted["publication"]["new"], after["witness"]);
                } else {
                    assert_eq!(interrupted, recovered);
                }
            }
            let stable = record(&anchor);
            let exact = inventory(&owner.client).await;
            for _ in 0..2 {
                if native {
                    native_read(&reopened, true).await;
                } else {
                    read_document(&reopened).await;
                }
            }
            reopened.close().await;
            assert_eq!(
                record(&anchor),
                stable,
                "pure read/saved retry must not mint nonce"
            );
            assert_eq!(
                inventory(&owner.client).await,
                exact,
                "exact retry/read changes no row"
            );
            let again = PostgresStore::open_fenced(config(&name, false), &anchor)
                .await
                .unwrap();
            if native {
                native_read(&again, true).await;
            } else {
                read_document(&again).await;
            }
            again.close().await;
            assert_eq!(record(&anchor), stable);
            assert_eq!(inventory(&owner.client).await, exact);
            fs::write(dir.join("result.json"),serde_json::to_vec_pretty(&json!({"phase":phase,"mode":mode,"signal":9,"before":before,"interrupted":interrupted,"recovered_primary":after,"recovered_anchor":recovered,"completed_anchor":stable,"exact_retained_inventory":exact,"status":"PASS"})).unwrap()).unwrap();
            owner.discard().await;
            // Retain the isolated database/anchor instead of timed DROP cleanup.
            eprintln!(
                "PG_PUBLICATION_CUT PASS mode={mode} phase={phase} retained={}",
                dir.display()
            );
        }
    }
}
