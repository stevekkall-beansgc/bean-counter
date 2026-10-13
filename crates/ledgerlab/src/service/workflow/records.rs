//! Reviewed publication record encoding and immutable retained verification.
use super::*;
use crate::store::sqlite::{
    SqliteTx,
    workflow::{Document, RecordRef, StoredOperation},
};
use ledgerlab_core::canonical::{self, CanonicalBytes};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) fn bytes(v: &impl serde::Serialize) -> Result<Vec<u8>> {
    CanonicalBytes::from_value(v)
        .map(|v| v.into_vec())
        .map_err(|_| ServiceError::IntegrityFailure)
}
pub(super) fn raw_hash(raw: &[u8]) -> String {
    canonical::hex(&Sha256::digest(raw))
}
pub(super) fn hash(domain: &str, version: u8, v: &Value) -> Result<String> {
    let mut h = Sha256::new();
    h.update(format!("bean-counter/{domain}/{version}\0").as_bytes());
    h.update(bytes(v)?);
    Ok(format!("sha256:{}", canonical::hex(&h.finalize())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        ports::{AcceptanceStore, AcceptanceTx},
        sqlite::workflow::tests::{create, deadline, fixture},
    };
    #[tokio::test]
    async fn exact_original_status_verifies_independent_retained_closure() {
        let (_dir, store) = create().await;
        let p = fixture();
        let key = request::OperationKey::parse(&p.operation.identity).unwrap();
        let mut tx = store.begin(deadline()).await.unwrap();
        tx.workflow_append(&p).await.unwrap();
        tx.commit().await.unwrap();
        let mut tx = store.begin(deadline()).await.unwrap();
        let original = tx
            .workflow_lookup(&key.bytes().unwrap())
            .await
            .unwrap()
            .unwrap();
        let result = verify_retained(&mut tx, &key, &original).await.unwrap();
        assert_eq!(
            result["receipt"],
            canonical::parse_bounded(&p.operation.receipt_bytes, 1024 * 1024).unwrap()
        );
        assert_eq!(
            result["request_evidence"],
            canonical::parse_bounded(&p.operation.operation_bytes, 8 * 1024 * 1024).unwrap()["request_evidence"]
        );
        let mut tampered = original.clone();
        tampered.receipt_bytes[8] = b'x';
        assert!(verify_retained(&mut tx, &key, &tampered).await.is_err());
        tx.rollback().await.unwrap();
        store.close().await;
    }
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key].as_str().ok_or(ServiceError::IntegrityFailure)
}
fn keys(v: &Value, expected: &[&str]) -> Result<()> {
    if v.as_object()
        .is_some_and(|o| o.len() == expected.len() && expected.iter().all(|k| o.contains_key(*k)))
    {
        Ok(())
    } else {
        Err(ServiceError::IntegrityFailure)
    }
}
fn checked(raw: &[u8], max: usize) -> Result<Value> {
    let v = canonical::parse_bounded(raw, max).map_err(|_| ServiceError::IntegrityFailure)?;
    if bytes(&v)? != raw {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(v)
}
pub(super) fn sort_refs(refs: &mut Vec<RecordRef>) -> Result<()> {
    canonical::sort_set(refs).map_err(|_| ServiceError::IntegrityFailure)
}
pub(super) fn source_document(kind: &str, body: Value) -> Result<Document> {
    let d = hash("workflow-document", 1, &json!([body["schema"], body]))?;
    Ok(Document {
        reference: RecordRef {
            kind: kind.into(),
            id: format!("wdoc_{}", &d[7..]),
            digest: d,
        },
        schema: text(&body, "schema")?.into(),
        bytes: bytes(&body)?,
    })
}
pub(super) fn record_reference(kind: &str, scope: &[String; 2], body: &Value) -> Result<RecordRef> {
    let (prefix, id_digest, digest) = match kind {
        "workflow-operation" => (
            "wop_",
            hash("workflow-operation", 1, &body["key"])?,
            hash("workflow-record", 1, &json!([kind, scope, body]))?,
        ),
        "workflow-manifest" => {
            let d = hash("workflow-manifest", 1, body)?;
            ("wmanifest_", d.clone(), d)
        }
        "workflow-receipt" => {
            let mut b = body.clone();
            b.as_object_mut()
                .ok_or(ServiceError::IntegrityFailure)?
                .remove("receipt_hash");
            let d = hash("operation-receipt", 2, &b)?;
            if body["receipt_hash"] != d {
                return Err(ServiceError::IntegrityFailure);
            }
            ("wreceipt_", d.clone(), d)
        }
        _ => return Err(ServiceError::IntegrityFailure),
    };
    Ok(RecordRef {
        kind: kind.into(),
        id: format!("{prefix}{}", &id_digest[7..]),
        digest,
    })
}
pub(super) fn publication_ref(store: &str, scope: &[String; 2], body: &Value) -> Result<RecordRef> {
    let id = hash(
        "workflow-publication",
        1,
        &json!([store, scope, body["root_policy_ref"]]),
    )?;
    Ok(RecordRef {
        kind: "workflow-publication".into(),
        id: format!("wpub_{}", &id[7..]),
        digest: hash(
            "workflow-record",
            1,
            &json!(["workflow-publication", scope, body]),
        )?,
    })
}

/// Verify the exact original closure without compiling or consulting mutable
/// source heads/current support. Current disclosure authority is checked first
/// by the caller. Any missing/corrupt retained member fails closed.
pub(super) async fn verify_retained(
    tx: &mut SqliteTx,
    key: &request::OperationKey,
    o: &StoredOperation,
) -> Result<Value> {
    let op = checked(&o.operation_bytes, 8 * 1024 * 1024)?;
    let manifest = checked(&o.manifest_bytes, 4 * 1024 * 1024)?;
    let receipt = checked(&o.receipt_bytes, 1024 * 1024)?;
    keys(
        &op,
        &[
            "schema",
            "key",
            "kind",
            "contract",
            "request_evidence",
            "authority_snapshot",
            "accepted_at",
            "commit_sequence",
            "result_refs",
        ],
    )?;
    keys(&manifest, &["schema", "key", "members"])?;
    keys(
        &receipt,
        &[
            "schema",
            "key",
            "request_hash",
            "request_hash_profile",
            "normalization_profile",
            "schema_bundle_digest",
            "descriptor_closure_digest",
            "original_request_bytes_sha256",
            "result_kind",
            "result_refs",
            "accepted_at",
            "commit_sequence",
            "manifest_hash",
            "receipt_hash",
        ],
    )?;
    if op["schema"] != "ledger-workflow-operation-record/1"
        || op["kind"] != "policy.publish"
        || op["contract"] != "workflow/1"
        || manifest["schema"] != "ledger-workflow-publication-manifest/1"
        || receipt["schema"] != "ledger-operation-receipt/2"
        || op["key"] != json!(key)
        || manifest["key"] != json!(key)
        || receipt["key"] != json!(key)
        || o.identity != key.bytes()?
        || o.operation_ref != record_reference("workflow-operation", &key.scope, &op)?
        || o.manifest_ref != record_reference("workflow-manifest", &key.scope, &manifest)?
        || o.receipt_ref != record_reference("workflow-receipt", &key.scope, &receipt)?
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut members: Vec<RecordRef> = serde_json::from_value(manifest["members"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let mut indexed = o.members.clone();
    sort_refs(&mut indexed)?;
    sort_refs(&mut members)?;
    if members != indexed
        || bytes(&members)? != bytes(&manifest["members"])?
        || members.iter().filter(|r| *r == &o.operation_ref).count() != 1
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let evidence = &op["request_evidence"];
    keys(
        evidence,
        &[
            "schema",
            "normalization_profile",
            "original_request_bytes_base64",
            "original_request_bytes_sha256",
            "normalized_request",
            "request_hash",
            "schema_bundle_digest",
            "descriptor_resolution",
        ],
    )?;
    let raw = ledgerlab_core::adjudication::proofs::decode_base64(
        text(evidence, "original_request_bytes_base64")?,
        262144,
    )
    .map_err(|_| ServiceError::IntegrityFailure)?;
    let attempted =
        request::PublicationRequest::parse(&raw).map_err(|_| ServiceError::IntegrityFailure)?;
    if attempted.key() != key
        || evidence["schema"] != "ledger-operation-request-evidence/2"
        || evidence["normalization_profile"] != "ledger-operation-normalization/2"
        || raw_hash(&raw) != text(evidence, "original_request_bytes_sha256")?
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let resolution = &evidence["descriptor_resolution"];
    keys(
        resolution,
        &[
            "schema",
            "registry_digest",
            "operation_kind",
            "payload_schema",
            "locator_digest",
            "anchors",
            "descriptors",
            "normalization_profile",
            "closure_digest",
        ],
    )?;
    let mut closure = resolution.clone();
    closure
        .as_object_mut()
        .ok_or(ServiceError::IntegrityFailure)?
        .remove("closure_digest");
    if hash("descriptor-closure", 1, &closure)? != resolution["closure_digest"]
        || hash("schema-bundle", 1, &resolution["descriptors"])? != evidence["schema_bundle_digest"]
        || hash(
            "operation-request",
            2,
            &json!([resolution["closure_digest"], evidence["normalized_request"]]),
        )? != evidence["request_hash"]
        || evidence["request_hash"] != o.request_hash
    {
        return Err(ServiceError::IntegrityFailure);
    }
    if receipt["request_hash"] != evidence["request_hash"]
        || receipt["request_hash_profile"] != "bean-counter/operation-request/2"
        || receipt["normalization_profile"] != evidence["normalization_profile"]
        || receipt["schema_bundle_digest"] != evidence["schema_bundle_digest"]
        || receipt["descriptor_closure_digest"] != resolution["closure_digest"]
        || receipt["original_request_bytes_sha256"] != evidence["original_request_bytes_sha256"]
        || receipt["manifest_hash"] != o.manifest_ref.digest
        || receipt["accepted_at"] != op["accepted_at"]
        || receipt["commit_sequence"] != op["commit_sequence"]
        || op["commit_sequence"] != o.sequence.to_string()
        || ledgerlab_core::domain::Timestamp::parse(text(&op, "accepted_at")?)
            .map_err(|_| ServiceError::IntegrityFailure)?
            .micros()
            != o.accepted_at_us
        || receipt["result_refs"] != op["result_refs"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut loaded = std::collections::BTreeMap::new();
    for r in &members {
        if r == &o.operation_ref {
            continue;
        }
        let d = tx
            .workflow_document(r, &key.logical_store_id, &key.scope[0], &key.scope[1], None)
            .await
            .map_err(super::super::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let body = checked(&d.bytes, 1024 * 1024)?;
        let expected = match r.kind.as_str() {
            "workflow-publication" => publication_ref(&key.logical_store_id, &key.scope, &body)?,
            "workflow-compiled-plan" => {
                let digest = hash("workflow-plan", 1, &body)?;
                RecordRef {
                    kind: r.kind.clone(),
                    id: format!("wplan_{}", &digest[7..]),
                    digest,
                }
            }
            _ => source_document(&r.kind, body.clone())?.reference,
        };
        if *r != expected || d.schema != text(&body, "schema")? {
            return Err(ServiceError::IntegrityFailure);
        }
        loaded.insert(r.id.clone(), (r.clone(), body));
    }
    let (publication_ref, publication) = loaded
        .get(&o.publication_id)
        .ok_or(ServiceError::IntegrityFailure)?;
    let (plan_ref, plan) = loaded
        .get(&o.plan_id)
        .ok_or(ServiceError::IntegrityFailure)?;
    keys(
        publication,
        &[
            "schema",
            "root_policy_ref",
            "source_refs",
            "compiled_plan_ref",
            "compiler_ref",
            "provenance",
        ],
    )?;
    keys(
        &publication["provenance"],
        &["source_sha", "binary_sha256", "build_manifest_digest"],
    )?;
    keys(
        plan,
        &[
            "schema",
            "compiler_ref",
            "compiler_semantics",
            "evaluator",
            "grammar",
            "limits_profile",
            "source_refs",
            "activity_table",
            "fact_slots",
            "link_paths",
            "component_slots",
            "rule_order",
            "typed_instructions",
            "dependency_edges",
            "worst_case",
            "explanation_map",
        ],
    )?;
    if publication_ref.kind != "workflow-publication"
        || plan_ref.kind != "workflow-compiled-plan"
        || publication["compiled_plan_ref"] != json!(plan_ref)
        || publication["source_refs"] != plan["source_refs"]
        || plan["compiler_ref"] != publication["compiler_ref"]
        || publication["schema"] != "ledger-workflow-publication/1"
        || plan["schema"] != "ledger-workflow-compiled-plan/1"
        || !matches!(
            receipt["result_kind"].as_str(),
            Some("administrative" | "semantic_alias")
        )
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let authority = &op["authority_snapshot"];
    keys(
        authority,
        &[
            "schema",
            "principal_id",
            "source",
            "authority_refs",
            "rights",
            "validated_at",
        ],
    )?;
    let refs: Vec<RecordRef> = serde_json::from_value(authority["authority_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    if refs.len() != 1
        || authority["schema"] != "ledger-workflow-publication-authority/1"
        || authority["source"] != key.source
        || authority["rights"] != json!(["admin"])
        || authority["validated_at"] != op["accepted_at"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let (ar, snapshot) = loaded
        .get(&refs[0].id)
        .ok_or(ServiceError::IntegrityFailure)?;
    let basis = &snapshot["basis"];
    keys(
        snapshot,
        &[
            "schema",
            "basis",
            "basis_digest",
            "verification_profile",
            "verified_at",
        ],
    )?;
    keys(
        basis,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "principal_id",
            "source",
            "canonical_economic_namespace",
            "rights",
            "source_grant_revision",
        ],
    )?;
    if *ar != refs[0]
        || ar.kind != "workflow-authority"
        || snapshot["schema"] != "ledger-workflow-local-authority-snapshot/1"
        || snapshot["basis_digest"] != hash("local-authority-basis", 1, basis)?
        || snapshot["verification_profile"] != "local-private-directory-owner/1"
        || snapshot["verified_at"] != op["accepted_at"]
        || basis["schema"] != "ledger-workflow-local-authority-basis/1"
        || basis["logical_store_id"] != key.logical_store_id
        || basis["scope"] != json!(key.scope)
        || basis["source"] != key.source
        || basis["principal_id"] != authority["principal_id"]
        || basis["canonical_economic_namespace"] != key.source
        || basis["rights"] != json!(["admin", "read"])
        || text(basis, "source_grant_revision")?
            .parse::<u64>()
            .ok()
            .is_none_or(|n| n == 0)
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let source_refs: Vec<RecordRef> = serde_json::from_value(publication["source_refs"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let raw_documents = if let Some(docs) = attempted.inline_documents() {
        docs.iter().map(bytes).collect::<Result<Vec<_>>>()?
    } else {
        attempted
            .retained_refs()
            .ok_or(ServiceError::IntegrityFailure)?
            .iter()
            .map(|r| {
                let (saved, body) = loaded.get(&r.id).ok_or(ServiceError::IntegrityFailure)?;
                if saved != r {
                    return Err(ServiceError::IntegrityFailure);
                }
                bytes(body)
            })
            .collect::<Result<Vec<_>>>()?
    };
    let normalized = super::publication::normalized(&attempted, &raw_documents)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let derived = super::publication::evidence(&attempted, &normalized)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let mut normalized_refs: Vec<RecordRef> = normalized
        .source_documents()
        .iter()
        .map(|d| RecordRef {
            kind: d.kind().into(),
            id: d.id().into(),
            digest: d.digest().into(),
        })
        .collect();
    sort_refs(&mut normalized_refs)?;
    if derived != *evidence
        || normalized_refs != source_refs
        || json!(attempted.root()) != publication["root_policy_ref"]
        || attempted.expected_digest() != plan_ref.digest
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let origin = tx
        .workflow_publication_origin(
            &o.publication_id,
            &key.logical_store_id,
            &key.scope[0],
            &key.scope[1],
        )
        .await
        .map_err(super::super::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    if receipt["result_kind"]
        != if origin == o.sequence {
            "administrative"
        } else {
            "semantic_alias"
        }
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut expected_members = source_refs.clone();
    let build_docs: Vec<_> = loaded
        .values()
        .filter(|(r, _)| r.kind == "workflow-build-manifest")
        .collect();
    if build_docs.len() == 1 {
        let (r, b) = build_docs[0];
        if super::provenance::projection(b).map_err(|_| ServiceError::IntegrityFailure)?
            != publication["provenance"]
            || b["build_inputs"]["recipe"]["compiler_ref"] != publication["compiler_ref"]
        {
            return Err(ServiceError::IntegrityFailure);
        }
        expected_members.push(r.clone());
    } else if !build_docs.is_empty() || !cfg!(test) {
        return Err(ServiceError::IntegrityFailure);
    }
    expected_members.extend([
        ar.clone(),
        plan_ref.clone(),
        publication_ref.clone(),
        o.operation_ref.clone(),
    ]);
    sort_refs(&mut expected_members)?;
    let mut results = vec![plan_ref.clone(), publication_ref.clone()];
    sort_refs(&mut results)?;
    if expected_members != members
        || op["result_refs"] != json!(results)
        || !source_refs
            .iter()
            .any(|r| json!(r) == publication["root_policy_ref"])
        || !source_refs
            .iter()
            .any(|r| json!(r) == publication["compiler_ref"])
    {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(
        json!({"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":evidence}),
    )
}
