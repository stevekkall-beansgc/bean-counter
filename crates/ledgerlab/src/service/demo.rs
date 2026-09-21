//! Synthetic provisioning only. Never a general append or real-assent API.
use crate::{
    service::store_error,
    store::{
        ports::{AcceptanceStore, AcceptanceTx},
        records::*,
        sqlite::SqliteStore,
    },
    ServiceError,
};
use ledgerlab_core::canonical::CanonicalBytes;
use serde_json::Value;
use std::{path::Path, time::Duration};
use tokio::time::Instant;

pub(crate) const EVENT: &[u8] =
    include_bytes!("../../../../fixtures/canonical/valid/first-slice-input.json");
fn text(v: &Value, k: &str) -> String {
    v[k].as_str().expect("frozen demo text").into()
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

pub(crate) async fn create(path: &Path) -> Result<(), ServiceError> {
    let store = SqliteStore::create(
        path,
        Installation {
            scope: scope(),
            logical_store_id: "store-demo-slice".into(),
            mode: "sandbox".into(),
            admission: "open".into(),
            dispatch_hold: true,
            dispatch_enabled: false,
            generation: 0,
        },
    )
    .await
    .map_err(store_error)?;
    let result = seed(&store).await;
    store.close().await;
    result
}
async fn seed(store: &SqliteStore) -> Result<(), ServiceError> {
    let mut tx = store
        .begin(Instant::now() + Duration::from_secs(5))
        .await
        .map_err(store_error)?;
    for line in include_str!("../../../../fixtures/journals/first-slice/seed-documents.jsonl")
        .lines()
        .chain(include_str!("../../../../fixtures/journals/first-slice/seed-records.jsonl").lines())
    {
        let v: Value = serde_json::from_str(line).expect("frozen demo seed");
        let b = &v["body"];
        let row = match v["kind"].as_str().unwrap() {
            "document" => JournalRow::Document {
                id: text(&v, "id"),
                kind: text(&v, "document_type"),
            },
            "party" => JournalRow::Party {
                id: text(&v, "id"),
                role_metadata_doc: text(b, "role_metadata_doc"),
            },
            "source-grant-record" => JournalRow::SourceGrant {
                id: text(&v, "id"),
                principal_id: text(b, "principal_id"),
                source: text(b, "source"),
                grant_doc: text(b, "grant_doc"),
            },
            "binding-record" => JournalRow::Binding {
                id: text(&v, "id"),
                agreement_id: text(b, "agreement_id"),
                version: number(b, "version"),
                policy_doc: text(b, "policy_doc"),
                assent_doc: text(b, "assent_doc"),
                roles_doc: text(b, "roles_doc"),
                context_doc: text(b, "context_doc"),
                currency: text(b, "currency"),
                scale: number(b, "scale"),
            },
            _ => unreachable!("only frozen provisioning records"),
        };
        tx.write(&WriteOp::Journal(Box::new(JournalRecord {
            scope: scope(),
            row,
            canonical: CanonicalRecord {
                canonical_bytes: CanonicalBytes::from_value(b)
                    .expect("frozen canonical body")
                    .into_vec(),
                content_hash: text(&v, "content_hash"),
            },
        })))
        .await
        .map_err(store_error)?;
    }
    let s: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/journals/first-slice/preseed-state.json"
    ))
    .unwrap();
    let c = &s["chain"];
    let a = &s["authority_head"];
    let b = &s["binding_head"];
    for op in [
        WriteOp::SeedChain(Chain {
            scope: scope(),
            id: text(c, "id"),
            customer: text(c, "customer"),
            currency: text(c, "currency"),
            scale: number(c, "scale"),
            binding_set_doc: text(c, "binding_set_doc"),
            context_doc: text(c, "context_doc"),
            revision: 0,
            event_count: 0,
        }),
        WriteOp::SeedAuthority(AuthorityHead {
            scope: scope(),
            id: text(a, "id"),
            grant_id: text(a, "grant_id"),
            revision: 1,
            active: true,
        }),
        WriteOp::SeedBinding(BindingHead {
            scope: scope(),
            id: text(b, "id"),
            selector_doc: text(b, "selector_doc"),
            binding_id: text(b, "binding_id"),
            revision: 1,
            active: true,
        }),
    ] {
        tx.write(&op).await.map_err(store_error)?;
    }
    tx.commit().await.map_err(|_| ServiceError::Unavailable)
}
