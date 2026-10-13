//! Mechanical selected-fixture tests, not public compiler/authority conformance.
use super::*;
use crate::store::{
    ports::{AcceptanceStore, AcceptanceTx},
    sqlite::{SqliteStore, tests::installation},
};
use ledgerlab_core::canonical::CanonicalBytes;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::Instant;

#[tokio::test]
async fn per_kind_sql_byte_guard_accepts_four_mib_only_for_compiled_plan() {
    let (_dir, store) = create().await;
    let mut tx = store.begin(deadline()).await.unwrap();
    // Isolate the table's mechanical byte constraint, without claiming these
    // diagnostic blobs are authenticated semantic documents or journal rows.
    for (index, (kind, size, accepted)) in [
        ("workflow-compiled-plan", 4194304, true),
        ("workflow-compiled-plan", 4194305, false),
        ("workflow-policy", 1048576, true),
        ("workflow-policy", 1048577, false),
        ("workflow-build-manifest", 1048577, false),
    ].into_iter().enumerate() {
        let result = sqlx::query("INSERT INTO workflow_documents(kind,id,digest,schema_name,canonical_bytes) VALUES(?,?,?,?,?)")
            .bind(kind).bind(format!("mechanical-byte-bound-{index}")).bind("diagnostic-only")
            .bind("diagnostic-only").bind(vec![b'a'; size]).execute(tx.conn()).await;
        assert_eq!(result.is_ok(), accepted, "{kind} size {size}: {result:?}");
    }
    tx.rollback().await.unwrap();
    store.close().await;
}

pub(crate) fn fixture() -> PublicationAppend {
    let fixture:Value=serde_json::from_str(include_str!("../../../../../../contracts/candidates/full-suite-v1/core/publication-independent-expected.json")).unwrap();
    let vector = |name: &str| {
        fixture["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["name"] == name)
            .unwrap()
    };
    let reference =
        |name: &str| serde_json::from_value::<RecordRef>(vector(name)["ref"].clone()).unwrap();
    let bytes = |name: &str| {
        vector(name)["canonical_body_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec()
    };
    let operation = &vector("operation")["body"];
    let key = &operation["key"];
    let identity = CanonicalBytes::from_value(key).unwrap().into_vec();
    let documents = fixture["vectors"].as_array().unwrap()[..9]
        .iter()
        .map(|v| {
            let b = CanonicalBytes::from_value(&v["body"]).unwrap().into_vec();
            assert_eq!(
                b,
                v["canonical_body_utf8"].as_str().unwrap().as_bytes(),
                "independent canonical bytes"
            );
            Document {
                reference: serde_json::from_value(v["ref"].clone()).unwrap(),
                schema: v["body"]["schema"].as_str().unwrap().into(),
                bytes: b,
            }
        })
        .collect();
    let registry = [
        ("facts", "schema_id"),
        ("context", "schema_id"),
        ("template", "template_id"),
        ("product", "product_id"),
        ("policy", "policy_id"),
    ]
    .into_iter()
    .map(|(name, id)| RegistryEntry {
        kind: reference(name).kind,
        semantic_id: vector(name)["body"][id].as_str().unwrap().into(),
        version: "1".into(),
        document_id: reference(name).id,
    })
    .collect();
    PublicationAppend {
        store_id: key["logical_store_id"].as_str().unwrap().into(),
        tenant: key["scope"][0].as_str().unwrap().into(),
        environment: key["scope"][1].as_str().unwrap().into(),
        source: key["source"].as_str().unwrap().into(),
        operation_id: key["operation_id"].as_str().unwrap().into(),
        documents,
        registry,
        publication_key: reference("publication").id.into_bytes(),
        root_policy_id: reference("policy").id,
        operation: StoredOperation {
            identity,
            operation_ref: reference("operation"),
            operation_bytes: bytes("operation"),
            request_hash: operation["request_evidence"]["request_hash"]
                .as_str()
                .unwrap()
                .into(),
            manifest_ref: reference("manifest"),
            manifest_bytes: bytes("manifest"),
            receipt_ref: reference("receipt"),
            receipt_bytes: bytes("receipt"),
            publication_id: reference("publication").id,
            plan_id: reference("unit-plan").id,
            sequence: 1,
            accepted_at_us: ledgerlab_core::domain::Timestamp::parse(
                operation["accepted_at"].as_str().unwrap(),
            )
            .unwrap()
            .micros(),
            members: serde_json::from_value(vector("manifest")["body"]["members"].clone()).unwrap(),
        },
    }
}
pub(crate) async fn create() -> (tempfile::TempDir, SqliteStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::create(dir.path(), installation())
        .await
        .unwrap();
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(20))
        .await
        .unwrap();
    sqlx::raw_sql(include_str!("schema.sql"))
        .execute(tx.conn())
        .await
        .unwrap();
    sqlx::query("PRAGMA user_version=12")
        .execute(tx.conn())
        .await
        .unwrap();
    tx.commit().await.unwrap();
    (dir, store)
}
pub(crate) fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(20)
}

#[tokio::test]
async fn exact_fixture_commit_identity_and_cache_free_database_reopen() {
    let (dir, store) = create().await;
    let p = fixture();
    let mut tx = store.begin(deadline()).await.unwrap();
    assert!(
        tx.workflow_lookup(&p.operation.identity)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(tx.workflow_sequence().await.unwrap(), 1);
    tx.workflow_append(&p).await.unwrap();
    assert_eq!(
        capacity::check(tx.conn(), 0, 0).await.unwrap(),
        capacity::WorkflowMeter {
            operations: 1,
            payload_bytes: 30258
        }
    );
    tx.commit().await.unwrap();
    // Simulate a response lost after real durable commit. Lookup returns the
    // original retained bytes without any compiler or current policy selection.
    let mut tx = store.begin(deadline()).await.unwrap();
    let saved = tx
        .workflow_lookup(&p.operation.identity)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(saved.operation_bytes, p.operation.operation_bytes);
    assert_eq!(saved.receipt_bytes, p.operation.receipt_bytes);
    assert_eq!(saved.manifest_bytes, p.operation.manifest_bytes);
    assert_eq!(saved.members.len(), 10);
    assert_eq!(tx.workflow_sequence().await.unwrap(), 2);
    for d in &p.documents {
        assert_eq!(
            tx.workflow_document(
                &d.reference,
                &p.store_id,
                &p.tenant,
                &p.environment,
                Some(&p.source)
            )
            .await
            .unwrap()
            .unwrap()
            .bytes,
            d.bytes
        );
    }
    assert_eq!(
        tx.workflow_publication(&p.publication_key)
            .await
            .unwrap()
            .unwrap()
            .0,
        p.operation.publication_id
    );
    let d = &p.documents[0];
    assert!(
        tx.workflow_document(&d.reference, "wrong-store", &p.tenant, &p.environment, None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.workflow_document(
            &d.reference,
            &p.store_id,
            "wrong-tenant",
            &p.environment,
            None
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        tx.workflow_document(&d.reference, &p.store_id, &p.tenant, "wrong-env", None)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        tx.workflow_document(
            &d.reference,
            &p.store_id,
            &p.tenant,
            &p.environment,
            Some("wrong-source")
        )
        .await
        .unwrap()
        .is_none()
    );
    assert!(
        tx.workflow_document(&d.reference, &p.store_id, &p.tenant, &p.environment, None)
            .await
            .unwrap()
            .is_some()
    );
    tx.rollback().await.unwrap();
    store.close().await;
    // Migration12 registration belongs to integration. This narrowly proves
    // on-disk bytes survive connection/process-owner loss, not facade open().
    use sqlx::Connection;
    let mut reopened = sqlx::SqliteConnection::connect(&format!(
        "sqlite:{}",
        dir.path().join("local.db").display()
    ))
    .await
    .unwrap();
    let original: Vec<u8> =
        sqlx::query_scalar("SELECT receipt_bytes FROM workflow_operations WHERE identity_key=?")
            .bind(&p.operation.identity)
            .fetch_one(&mut reopened)
            .await
            .unwrap();
    assert_eq!(original, p.operation.receipt_bytes);
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn every_insert_failure_rolls_back_all_records_indexes_and_feed() {
    let (_dir, store) = create().await;
    let p = fixture();
    let boundaries = p.documents.len() + p.registry.len() + 1 + 1 + p.operation.members.len() + 1;
    for cut in 1..=boundaries {
        let mut tx = store.begin(deadline()).await.unwrap();
        sqlx::query("CREATE TEMP TABLE fail_counter(n INTEGER)")
            .execute(tx.conn())
            .await
            .unwrap();
        sqlx::query("INSERT INTO fail_counter VALUES(0)")
            .execute(tx.conn())
            .await
            .unwrap();
        for table in [
            "workflow_documents",
            "workflow_definition_registry",
            "workflow_publications",
            "workflow_operations",
            "workflow_manifest_members",
            "workflow_feed",
        ] {
            let trigger = format!(
                "CREATE TEMP TRIGGER fail_{table} BEFORE INSERT ON {table} BEGIN UPDATE fail_counter SET n=n+1; SELECT CASE WHEN (SELECT n FROM fail_counter)={cut} THEN RAISE(ABORT,'injected insert failure') END; END"
            );
            sqlx::raw_sql(sqlx::AssertSqlSafe(trigger))
                .execute(tx.conn())
                .await
                .unwrap();
        }
        assert!(tx.workflow_append(&p).await.is_err(), "cut {cut}");
        assert!(matches!(
            tx.commit().await,
            Err(crate::store::errors::CommitError::RolledBack(_))
        ));
        let mut tx = store.begin(deadline()).await.unwrap();
        let counts:Value=json!(sqlx::query_as::<_,(i64,i64,i64,i64,i64,i64)>("SELECT (SELECT count(*) FROM workflow_documents),(SELECT count(*) FROM workflow_definition_registry),(SELECT count(*) FROM workflow_publications),(SELECT count(*) FROM workflow_operations),(SELECT count(*) FROM workflow_manifest_members),(SELECT count(*) FROM workflow_feed)").fetch_one(tx.conn()).await.unwrap());
        assert_eq!(counts, json!([0, 0, 0, 0, 0, 0]), "cut {cut}");
        assert_eq!(
            capacity::check(tx.conn(), 0, 0).await.unwrap(),
            capacity::WorkflowMeter {
                operations: 0,
                payload_bytes: 0
            }
        );
        tx.rollback().await.unwrap();
    }
    store.close().await;
}

#[tokio::test]
async fn independent_alias_meter_charges_one_unit_only_new_document_bytes() {
    let (_dir, store) = create().await;
    let p = fixture();
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.workflow_append(&p).await.unwrap();
    tx.commit().await.unwrap();
    let expected:Value=serde_json::from_str(include_str!("../../../../../../contracts/candidates/full-suite-v1/core/admission-independent-expected.json")).unwrap();
    let v = |name: &str| {
        expected["vectors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["name"] == name)
            .unwrap()
    };
    let reference =
        |name: &str| serde_json::from_value::<RecordRef>(v(name)["ref"].clone()).unwrap();
    let bytes = |name: &str| {
        v(name)["canonical_utf8"]
            .as_str()
            .unwrap()
            .as_bytes()
            .to_vec()
    };
    let mut alias = fixture();
    alias.documents[8] = Document {
        reference: reference("authority-snapshot"),
        schema: v("authority-snapshot")["body"]["schema"]
            .as_str()
            .unwrap()
            .into(),
        bytes: bytes("authority-snapshot"),
    };
    let body = &v("operation")["body"];
    alias.operation_id = body["key"]["operation_id"].as_str().unwrap().into();
    alias.operation = StoredOperation {
        identity: CanonicalBytes::from_value(&body["key"]).unwrap().into_vec(),
        operation_ref: reference("operation"),
        operation_bytes: bytes("operation"),
        request_hash: body["request_evidence"]["request_hash"]
            .as_str()
            .unwrap()
            .into(),
        manifest_ref: reference("manifest"),
        manifest_bytes: bytes("manifest"),
        receipt_ref: reference("receipt"),
        receipt_bytes: bytes("receipt"),
        publication_id: p.operation.publication_id.clone(),
        plan_id: p.operation.plan_id.clone(),
        sequence: 2,
        accepted_at_us: ledgerlab_core::domain::Timestamp::parse(
            body["accepted_at"].as_str().unwrap(),
        )
        .unwrap()
        .micros(),
        members: serde_json::from_value(v("manifest")["body"]["members"].clone()).unwrap(),
    };
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.workflow_append(&alias).await.unwrap();
    assert_eq!(
        capacity::check(tx.conn(), 0, 0).await.unwrap(),
        capacity::WorkflowMeter {
            operations: 2,
            payload_bytes: 30258 + 19996
        }
    );
    let document_count: i64 = sqlx::query_scalar("SELECT count(*) FROM workflow_documents")
        .fetch_one(tx.conn())
        .await
        .unwrap();
    assert_eq!(document_count, 10);
    tx.commit().await.unwrap();
    store.close().await;
}

#[tokio::test]
async fn immutable_document_and_registry_conflicts_poison_append() {
    let (_dir, store) = create().await;
    let p = fixture();
    let mut tx = store.begin(deadline()).await.unwrap();
    tx.workflow_append(&p).await.unwrap();
    tx.commit().await.unwrap();
    let mut changed = fixture();
    changed.operation.identity = b"new-request".to_vec();
    changed.operation_id = "publish-002".into();
    changed.operation.sequence = 2;
    changed.source = "urn:fixture:other-publisher".into();
    changed.registry[0].document_id = "wdoc_different".into();
    let mut tx = store.begin(deadline()).await.unwrap();
    assert!(matches!(
        tx.workflow_append(&changed).await,
        Err(StoreError::DeliveryConflict)
    ));
    assert!(matches!(
        tx.commit().await,
        Err(crate::store::errors::CommitError::RolledBack(_))
    ));
    let mut tx = store.begin(deadline()).await.unwrap();
    assert!(
        sqlx::query("UPDATE workflow_documents SET canonical_bytes=x'00'")
            .execute(tx.conn())
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    store.close().await;
}
