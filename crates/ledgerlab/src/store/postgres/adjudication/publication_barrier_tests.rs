//! Real coordinator/session-loss barrier, distinct from full-driver SIGKILL
//! coverage. No physical admission or economic acceptance is claimed.
use super::super::{
    publication::{read_witness, PublicationOwner, RecoveryDisposition},
    recovery::Gate,
};
use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{atomic::AtomicI32, Arc},
};
fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}
fn record(path: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path.join("state.json")).unwrap()).unwrap()
}
async fn slot<C: GenericClient + Sync>(c: &C) -> Vec<u8> {
    c.query_one("SELECT sha256(convert_to(row_to_json(t)::text,'UTF8')) FROM ledgerlab.r3_unresolved_work t WHERE singleton=1",&[]).await.unwrap().get(0)
}
async fn identity<C: GenericClient + Sync>(c: &C, pid: i32) -> String {
    c.query_one("SELECT backend_start::text FROM pg_stat_activity WHERE pid=$1 AND datname=current_database() AND usename='ledgerlab_phase1_runtime'",&[&pid]).await.unwrap().get(0)
}
async fn blocked<C: GenericClient + Sync>(
    c: &C,
    recovery_pid: i32,
    recovery_start: &str,
    work_pid: i32,
    work_start: &str,
) -> serde_json::Value {
    let end = Instant::now() + Duration::from_millis(350);
    loop {
        let row=c.query_one("SELECT backend_start::text,state,wait_event_type,wait_event,pg_blocking_pids(pid),(SELECT backend_start::text FROM pg_stat_activity WHERE pid=$2) FROM pg_stat_activity WHERE pid=$1 AND datname=current_database()",&[&recovery_pid,&work_pid]).await.unwrap();
        assert_eq!(row.get::<_, String>(0), recovery_start);
        assert_eq!(row.get::<_, Option<String>>(5).as_deref(), Some(work_start));
        let blockers: Vec<i32> = row.get(4);
        if blockers.contains(&work_pid) {
            assert_eq!(row.get::<_, Option<String>>(2).as_deref(), Some("Lock"));
            return json!({"recovery_pid":recovery_pid,"recovery_backend_start":recovery_start,"state":row.get::<_,String>(1),"wait_event_type":row.get::<_,Option<String>>(2),"wait_event":row.get::<_,Option<String>>(3),"blocking_pids":blockers,"work_pid":work_pid,"work_backend_start":work_start});
        }
        assert!(Instant::now()<end,"exact live legacy row-lock blocker not observed within unchanged500ms SQL lock deadline");
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}
#[tokio::test]
#[ignore = "requires isolated PostgreSQL17/18 TLS; actual legacy session-loss barrier"]
async fn postgres_live_legacy_orphan_blocks_publication_until_commit_or_rollback() {
    let root = PathBuf::from(
        std::env::var_os("LEDGERLAB_PG_CUT_EVIDENCE").expect("retained evidence directory"),
    );
    fs::create_dir_all(&root).unwrap();
    for commit in [false, true] {
        let label = if commit { "commit" } else { "rollback" };
        let dir = root.join(format!("legacy-orphan-{label}-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let anchor = dir.join("anchor");
        fs::create_dir(&anchor).unwrap();
        let Fixture { name, owner, store } = Fixture::new().await;
        store.close().await;
        owner
            .client
            .execute(
                "UPDATE ledgerlab.installation SET admission='frozen' WHERE singleton=1",
                &[],
            )
            .await
            .unwrap();
        fs::write(dir.join("fixture.json"),serde_json::to_vec_pretty(&json!({"database":name,"anchor":anchor,"outcome":label,"scope":"actual private publication coordinator, no physical/economic claim"})).unwrap()).unwrap();
        eprintln!(
            "PG_LEGACY_ORPHAN fixture={name} outcome={label} retained={}",
            dir.display()
        );
        let configured =
            super::super::super::bootstrap::bind(config(&name, true), &anchor, "center")
                .await
                .unwrap();
        drop(configured);
        let store = PostgresStore::open_fenced(config(&name, false), &anchor)
            .await
            .unwrap();
        let publication: Arc<PublicationOwner> = store.inner.publication.as_ref().unwrap().clone();
        let old = read_witness(&owner.client).await.unwrap();
        let before = record(&anchor);
        let original_slot = slot(&owner.client).await;
        let cfg = config(&name, false);
        let mut work = cfg.connect().await.unwrap();
        let work_pid: i32 = work
            .client
            .query_one("SELECT pg_backend_pid()", &[])
            .await
            .unwrap()
            .get(0);
        let work_start = identity(&owner.client, work_pid).await;
        let control_pid = Arc::new(AtomicI32::new(0));
        let mut gate = Gate::acquire_raw(&cfg, deadline(), &control_pid)
            .await
            .unwrap();
        publication
            .recover_under_gate(&mut gate, deadline())
            .await
            .unwrap();
        gate.resolve_after_publication().await.unwrap();
        let first_pid = control_pid.load(Ordering::Acquire);
        let first_start = identity(&owner.client, first_pid).await;
        let tx = work
            .client
            .build_transaction()
            .isolation_level(tokio_postgres::IsolationLevel::Serializable)
            .start()
            .await
            .unwrap();
        let pin = publication.pin_snapshot(&tx, deadline()).await.unwrap();
        let mut lease = publication
            .begin_write(&mut gate, &tx, &pin, deadline())
            .await
            .unwrap();
        let document = crate::store::sqlite::tests::seed().remove(0);
        super::super::super::write::operation(&tx, &document)
            .await
            .unwrap();
        lease.mark_mutation();
        lease
            .prepare_commit(&tx, b"actual-legacy-orphan-control", deadline())
            .await
            .unwrap();
        let pending = record(&anchor);
        assert_eq!(pending["publication"]["state"], "Pending");
        // Lose only controller ownership: the independent SQL work backend and
        // its transaction remain live and can still successfully COMMIT.
        drop(lease);
        drop(pin);
        gate.abandon().await;
        assert!(!publication.available().await);
        assert_eq!(identity(&owner.client, work_pid).await, work_start);
        let replacement_pid = Arc::new(AtomicI32::new(0));
        let mut replacement = Gate::acquire_raw(&cfg, deadline(), &replacement_pid)
            .await
            .unwrap();
        let next_pid = replacement_pid.load(Ordering::Acquire);
        let next_start = identity(&owner.client, next_pid).await;
        let publication2 = publication.clone();
        let recovery = tokio::spawn(async move {
            let result = publication2
                .recover_under_gate(&mut replacement, deadline())
                .await;
            if result.is_ok() {
                replacement.resolve_after_publication().await.unwrap();
                replacement.finish().await.unwrap();
            }
            result
        });
        let reader_store = store.clone();
        let reader = tokio::spawn(async move { reader_store.begin(deadline()).await });
        let actual_blocker =
            blocked(&owner.client, next_pid, &next_start, work_pid, &work_start).await;
        assert!(
            !recovery.is_finished(),
            "recovery cannot pass a live legacy mutation"
        );
        let early_reader = if reader.is_finished() {
            let result = reader.await.unwrap();
            assert!(
                result.is_err(),
                "application read admitted before publication recovery"
            );
            None
        } else {
            Some(reader)
        };
        assert_eq!(read_witness(&owner.client).await.unwrap(), old);
        assert_eq!(
            slot(&owner.client).await,
            original_slot,
            "recovery must not clear staging ahead of external recovery"
        );
        assert_eq!(record(&anchor), pending);
        fs::write(
            dir.join("blocker.json"),
            serde_json::to_vec_pretty(&actual_blocker).unwrap(),
        )
        .unwrap();
        if commit {
            tx.commit().await.unwrap();
        } else {
            tx.rollback().await.unwrap();
        }
        work.discard().await;
        assert_eq!(
            recovery.await.unwrap().unwrap(),
            if commit {
                RecoveryDisposition::RecoveredNew
            } else {
                RecoveryDisposition::RecoveredOld
            }
        );
        let mut admitted = if let Some(reader) = early_reader {
            match reader.await.unwrap() {
                Ok(tx) => tx,
                Err(_) => store.begin(deadline()).await.unwrap(),
            }
        } else {
            store.begin(deadline()).await.unwrap()
        };
        let crate::store::records::WriteOp::Journal(row) = document else {
            panic!("document")
        };
        let crate::store::records::JournalRow::Document { id, kind } = &row.row else {
            panic!("document")
        };
        let seen = admitted.load_document(&row.scope, id).await.unwrap();
        assert_eq!(seen, commit.then(|| (kind.clone(), row.canonical.clone())));
        admitted.commit().await.unwrap();
        let after = read_witness(&owner.client).await.unwrap();
        let stable = record(&anchor);
        assert_eq!(stable["publication"]["state"], "Stable");
        assert_eq!(stable["publication"]["witness"], after.witness().as_str());
        if commit {
            assert_eq!(pending["publication"]["new"], after.witness().as_str());
            assert_ne!(after, old);
        } else {
            assert_eq!(after, old);
            assert_eq!(stable, before);
        }
        assert_eq!(slot(&owner.client).await, original_slot);
        let count: i64 = owner
            .client
            .query_one("SELECT count(*) FROM ledgerlab.documents", &[])
            .await
            .unwrap()
            .get(0);
        assert_eq!(count, if commit { 1 } else { 0 });
        drop(publication);
        store.close().await;
        let reopened = PostgresStore::open_fenced(config(&name, false), &anchor)
            .await
            .unwrap();
        let mut pure = reopened.begin(deadline()).await.unwrap();
        assert_eq!(pure.load_document(&row.scope, id).await.unwrap(), seen);
        pure.commit().await.unwrap();
        reopened.close().await;
        assert_eq!(record(&anchor), stable);
        assert_eq!(slot(&owner.client).await, original_slot);
        fs::write(dir.join("result.json"),serde_json::to_vec_pretty(&json!({"status":"PASS","outcome":label,"old_control_pid":first_pid,"old_control_backend_start":first_start,"blocker":actual_blocker,"before":before,"pending":pending,"stable":stable,"documents":count,"staging_exact_unchanged":true,"pure_read_reopen_nonce_unchanged":true})).unwrap()).unwrap();
        owner.discard().await;
        eprintln!(
            "PG_LEGACY_ORPHAN PASS outcome={label} retained={}",
            dir.display()
        );
    }
}
