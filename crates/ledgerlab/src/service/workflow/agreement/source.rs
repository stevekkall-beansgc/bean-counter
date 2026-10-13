//! Local-owner source administration. Pure records assert data; this resolver
//! authenticates their retained origins, permanent scope and current projection.
use super::super::request::OperationKey;
use super::*;
use crate::{service::billing::Setup, store::sqlite::workflow::agreement::SourceHead};
use ledgerlab_core::policy::workflow::{SourceCeiling, SourceEnrollment, SourceGrant, SourceState};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enroll {
    schema: String,
    action: String,
    expected_revision: String,
    enrollment_id: String,
    customer_refs: Vec<RecordRef>,
    ceiling: Value,
    evidence_ref: RecordRef,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Restrict {
    schema: String,
    action: String,
    enrollment_ref: RecordRef,
    expected_revision: String,
    replacement_ceiling: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revoke {
    schema: String,
    action: String,
    enrollment_ref: RecordRef,
    expected_revision: String,
    reason: String,
}
fn id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 128 && !s.chars().any(char::is_control)
}
fn reference(r: &RecordRef, kind: &str) -> Result<()> {
    let parsed = ledgerlab_core::policy::workflow::AgreementRef::parse(&records::bytes(r)?)
        .map_err(|_| reject("INVALID_REFERENCE"))?;
    require(parsed.kind() == kind, "INVALID_REFERENCE")
}
fn revision(s: &str) -> Result<i64> {
    let n = s.parse::<i64>().map_err(|_| reject("INVALID_SHAPE"))?;
    require(n > 0 && n.to_string() == s, "INVALID_SHAPE")?;
    Ok(n)
}
pub(super) fn normalize(payload: &Value) -> Result<Value> {
    match payload["schema"].as_str() {
        Some("ledger-workflow-source-enroll/1") => {
            let mut p: Enroll =
                serde_json::from_value(payload.clone()).map_err(|_| reject("INVALID_SHAPE"))?;
            require(
                p.action == "source.enroll"
                    && p.expected_revision == "0"
                    && id(&p.enrollment_id)
                    && (1..=64).contains(&p.customer_refs.len()),
                "INVALID_SHAPE",
            )?;
            for r in &p.customer_refs {
                reference(r, "workflow-customer")?;
            }
            canonical::sort_set(&mut p.customer_refs).map_err(|_| reject("INVALID_SHAPE"))?;
            reference(&p.evidence_ref, "workflow-evidence")?;
            p.ceiling = SourceCeiling::parse(&records::bytes(&p.ceiling)?)
                .map_err(typed_error)?
                .canonical_value()
                .map_err(typed_error)?;
            serde_json::to_value(p).map_err(|_| ServiceError::IntegrityFailure)
        }
        Some("ledger-workflow-source-restrict/1") => {
            let mut p: Restrict =
                serde_json::from_value(payload.clone()).map_err(|_| reject("INVALID_SHAPE"))?;
            require(p.action == "source.restrict", "INVALID_SHAPE")?;
            revision(&p.expected_revision)?;
            reference(&p.enrollment_ref, "workflow-source-enrollment")?;
            p.replacement_ceiling = SourceCeiling::parse(&records::bytes(&p.replacement_ceiling)?)
                .map_err(typed_error)?
                .canonical_value()
                .map_err(typed_error)?;
            serde_json::to_value(p).map_err(|_| ServiceError::IntegrityFailure)
        }
        Some("ledger-workflow-source-revoke/1") => {
            let p: Revoke =
                serde_json::from_value(payload.clone()).map_err(|_| reject("INVALID_SHAPE"))?;
            require(
                p.action == "source.revoke" && !p.reason.is_empty() && p.reason.len() <= 1024,
                "INVALID_SHAPE",
            )?;
            revision(&p.expected_revision)?;
            reference(&p.enrollment_ref, "workflow-source-enrollment")?;
            serde_json::to_value(p).map_err(|_| ServiceError::IntegrityFailure)
        }
        _ => Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
    }
}
pub(super) fn semantic(payload: &Value, enrollment_id: &str) -> Result<Vec<u8>> {
    records::bytes(&if payload["action"] == "source.enroll" {
        json!(["source.enroll", enrollment_id])
    } else {
        let next = revision(text(payload, "expected_revision")?)?
            .checked_add(1)
            .ok_or_else(|| reject("INVALID_SHAPE"))?;
        json!(["source.state", enrollment_id, next.to_string()])
    })
}
pub(super) async fn enrollment(
    tx: &mut SqliteTx,
    key: &OperationKey,
    r: &RecordRef,
) -> Result<SourceEnrollment> {
    verify::document_origin(tx, key, r).await?;
    let d = loaded(tx, key, r).await?;
    let e = SourceEnrollment::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
    if json!(e.reference()) != json!(r) || e.scope() != &key.scope {
        return Err(ServiceError::IntegrityFailure);
    }
    let index = tx
        .workflow_agreement_enrollment(&key.logical_store_id, &key.scope, e.enrollment_id())
        .await
        .map_err(crate::service::store_error)?;
    if index.as_deref() != Some(r.id.as_str()) {
        return Err(ServiceError::IntegrityFailure);
    }
    let namespace = tx
        .workflow_agreement_namespace(&key.logical_store_id, &key.scope, e.ceiling().source())
        .await
        .map_err(crate::service::store_error)?;
    if namespace.as_deref() != Some(e.ceiling().canonical_economic_namespace()) {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(e)
}
async fn by_id(tx: &mut SqliteTx, key: &OperationKey, id: &str) -> Result<Document> {
    tx.workflow_document_by_id(id, &key.logical_store_id, &key.scope[0], &key.scope[1])
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)
}
fn data_document(kind: &str, body: Value) -> Result<Document> {
    records::source_document(kind, body)
}
fn grant(
    ceiling: &Value,
    e: &SourceEnrollment,
    revision: i64,
    prior: Option<&SourceGrant>,
) -> Result<(SourceGrant, Document)> {
    let mut body = ceiling.clone();
    body["schema"] = json!("ledger-workflow-source-grant/1");
    body["grant_id"] = json!(e.enrollment_id());
    body["revision"] = json!(revision.to_string());
    body["original_ceiling_ref"] = json!(e.reference());
    let parsed =
        SourceGrant::parse(&records::bytes(&body)?).map_err(|_| ServiceError::IntegrityFailure)?;
    parsed
        .validate_lineage(e.enrollment_id(), e.reference(), e.ceiling(), prior)
        .map_err(|_| reject("SOURCE_CEILING_EXCEEDED"))?;
    let doc = data_document(
        "workflow-source-grant",
        parsed
            .canonical_value()
            .map_err(|_| ServiceError::IntegrityFailure)?,
    )?;
    Ok((parsed, doc))
}
fn state(
    e: &SourceEnrollment,
    key: &OperationKey,
    actor: &str,
    at: &Timestamp,
    revision: i64,
    current: Option<&SourceGrant>,
    prior: Option<(&SourceState, &SourceGrant)>,
) -> Result<(SourceState, Document)> {
    let mut body = json!({"schema":"ledger-workflow-source-state/1","enrollment_ref":e.reference(),"revision":revision.to_string(),"status":if current.is_some(){"active"}else{"revoked"},"operation_key":key,"actor":actor,"accepted_at":at});
    if let Some(g) = current {
        body["current_grant_ref"] = json!({"kind":"workflow-source-grant","id":g.document_id(),"digest":g.content_digest()});
    }
    if let Some((s, _)) = prior {
        body["prior_state_ref"] = json!(s.reference());
    }
    let parsed = SourceState::parse(&records::bytes(&body)?, e)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    parsed
        .validate_lineage(e, current, prior)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let doc = Document {
        reference: serde_json::from_value(json!(parsed.reference()))
            .map_err(|_| ServiceError::IntegrityFailure)?,
        schema: "ledger-workflow-source-state/1".into(),
        bytes: parsed.canonical_bytes().to_vec(),
    };
    Ok((parsed, doc))
}
pub(super) struct PreparedSource {
    pub documents: Vec<Document>,
    pub results: Vec<RecordRef>,
    pub projection: Projection,
}
/// Caller has checked actual current owner/mapping and holds the writer lock.
pub(super) async fn prepare(
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    setup: &Setup,
    at: &Timestamp,
    mode: &str,
    root: &RecordRef,
) -> Result<PreparedSource> {
    if payload["action"] == "source.enroll" {
        let p: Enroll =
            serde_json::from_value(payload.clone()).map_err(|_| ServiceError::IntegrityFailure)?;
        require(
            tx.workflow_agreement_enrollment(&key.logical_store_id, &key.scope, &p.enrollment_id)
                .await
                .map_err(crate::service::store_error)?
                .is_none(),
            "SEMANTIC_CONFLICT",
        )?;
        let ceiling = SourceCeiling::parse(&records::bytes(&p.ceiling)?).map_err(typed_error)?;
        require(
            ceiling.source() == setup.source
                && ceiling.canonical_economic_namespace() == setup.source
                && ceiling.principal_id() == setup.operator,
            "AUTHORITY_REQUIRED",
        )?;
        if tx
            .workflow_agreement_namespace(&key.logical_store_id, &key.scope, &setup.source)
            .await
            .map_err(crate::service::store_error)?
            .is_some_and(|n| n != setup.source)
        {
            return Err(ServiceError::IntegrityFailure);
        }
        let dependencies = verify::authenticated_closure(
            tx,
            key,
            &p.customer_refs
                .iter()
                .cloned()
                .chain(std::iter::once(p.evidence_ref.clone()))
                .collect::<Vec<_>>(),
        )
        .await?;
        let mut customers = vec![];
        for r in &p.customer_refs {
            let d = dependencies
                .iter()
                .find(|d| &d.reference == r)
                .ok_or(ServiceError::IntegrityFailure)?;
            let b = canonical_body(d)?;
            require(b["installation_mode"] == mode, "INSTALLATION_MODE_MISMATCH")?;
            let customer = text(&b, "customer")?;
            require(
                tx.workflow_agreement_customer(&key.logical_store_id, &key.scope, customer)
                    .await
                    .map_err(crate::service::store_error)?
                    .as_deref()
                    == Some(r.id.as_str()),
                "AUTHORITY_REQUIRED",
            )?;
            customers.push(customer.to_string());
        }
        canonical::sort_set(&mut customers).map_err(|_| reject("INVALID_SHAPE"))?;
        require(
            p.ceiling["customer_scope"] == json!(customers),
            "AUTHORITY_REQUIRED",
        )?;
        let proof = dependencies
            .iter()
            .find(|d| d.reference == p.evidence_ref)
            .ok_or(ServiceError::IntegrityFailure)?;
        let proof_body = canonical_body(proof)?;
        let input = evidence_input(&proof_body)?;
        require(
            input.purpose() == "source_enrollment"
                && customers.contains(&input.customer().to_string())
                && input.metadata()["source"] == setup.source
                && input.metadata()["principal_id"] == setup.operator
                && proof_body["retained_by"] == setup.operator,
            "AUTHORITY_REQUIRED",
        )?;
        require(
            proof_body["installation_mode"] == mode,
            "INSTALLATION_MODE_MISMATCH",
        )?;
        let body = json!({"schema":"ledger-workflow-source-enrollment/1","scope":key.scope,"enrollment_id":p.enrollment_id,"customer_refs":p.customer_refs,"ceiling":p.ceiling,"evidence_ref":p.evidence_ref,"enrolled_by":setup.operator,"enrolled_at":at,"authority_ref":root,"installation_mode":mode});
        let e = SourceEnrollment::parse(&records::bytes(&body)?)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let enrollment_doc = data_document(
            "workflow-source-enrollment",
            e.canonical_value()
                .map_err(|_| ServiceError::IntegrityFailure)?,
        )?;
        let (g, grant_doc) = grant(&p.ceiling, &e, 1, None)?;
        let (_, state_doc) = state(&e, key, &setup.operator, at, 1, Some(&g), None)?;
        let projection = Projection::Enrollment {
            enrollment: p.enrollment_id,
            document: enrollment_doc.reference.id.clone(),
            source: setup.source.clone(),
            namespace: setup.source.clone(),
            head: SourceHead {
                revision: 1,
                state: state_doc.reference.id.clone(),
                grant: Some(grant_doc.reference.id.clone()),
                status: "active".into(),
            },
        };
        let results = vec![
            enrollment_doc.reference.clone(),
            grant_doc.reference.clone(),
            state_doc.reference.clone(),
        ];
        let mut documents = vec![enrollment_doc, grant_doc, state_doc];
        documents.extend(dependencies);
        return Ok(PreparedSource {
            documents,
            results,
            projection,
        });
    }
    let r: RecordRef = serde_json::from_value(payload["enrollment_ref"].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let e = enrollment(tx, key, &r).await?;
    require(
        e.ceiling().source() == setup.source && e.ceiling().principal_id() == setup.operator,
        "AUTHORITY_REQUIRED",
    )?;
    let b = e
        .canonical_value()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    require(b["installation_mode"] == mode, "INSTALLATION_MODE_MISMATCH")?;
    let head = tx
        .workflow_agreement_source_head(&key.logical_store_id, &key.scope, e.enrollment_id())
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    let expected = revision(text(payload, "expected_revision")?)?;
    require(head.revision == expected, "STALE_REVISION")?;
    require(head.status == "active", "AUTHORITY_REQUIRED")?;
    let state_doc = by_id(tx, key, &head.state).await?;
    verify::document_origin(tx, key, &state_doc.reference).await?;
    let previous =
        SourceState::parse(&state_doc.bytes, &e).map_err(|_| ServiceError::IntegrityFailure)?;
    let grant_doc = by_id(
        tx,
        key,
        head.grant
            .as_deref()
            .ok_or(ServiceError::IntegrityFailure)?,
    )
    .await?;
    verify::document_origin(tx, key, &grant_doc.reference).await?;
    let previous_grant =
        SourceGrant::parse(&grant_doc.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
    if previous.revision() != expected as u64
        || previous.logical_store_id() != key.logical_store_id
        || previous.status() != head.status
        || json!(previous.reference()) != json!(state_doc.reference)
        || previous.current_grant_ref().map(|g| g.id()) != Some(grant_doc.reference.id.as_str())
    {
        return Err(ServiceError::IntegrityFailure);
    }
    let next = expected
        .checked_add(1)
        .ok_or_else(|| reject("INVALID_SHAPE"))?;
    let (current, new_grant) = if payload["action"] == "source.restrict" {
        let (g, d) = grant(
            &payload["replacement_ceiling"],
            &e,
            next,
            Some(&previous_grant),
        )?;
        (Some(g), Some(d))
    } else {
        (None, None)
    };
    let (_, new_state) = state(
        &e,
        key,
        &setup.operator,
        at,
        next,
        current.as_ref(),
        Some((&previous, &previous_grant)),
    )?;
    let projection = Projection::State {
        enrollment: e.enrollment_id().into(),
        expected_revision: expected,
        head: SourceHead {
            revision: next,
            state: new_state.reference.id.clone(),
            grant: new_grant.as_ref().map(|d| d.reference.id.clone()),
            status: if current.is_some() {
                "active"
            } else {
                "revoked"
            }
            .into(),
        },
    };
    let seeds = vec![state_doc.reference, grant_doc.reference, r];
    let mut documents = verify::authenticated_closure(tx, key, &seeds).await?;
    let mut results = vec![new_state.reference.clone()];
    documents.push(new_state);
    if let Some(d) = new_grant {
        results.push(d.reference.clone());
        documents.push(d);
    }
    Ok(PreparedSource {
        documents,
        results,
        projection,
    })
}
fn doc<'a>(
    documents: &'a std::collections::BTreeMap<String, Document>,
    v: &Value,
) -> Result<&'a Document> {
    let r: RecordRef =
        serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)?;
    let d = documents.get(&r.id).ok_or(ServiceError::IntegrityFailure)?;
    if d.reference != r {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(d)
}
pub(super) fn verify_closure(
    key: &OperationKey,
    documents: &std::collections::BTreeMap<String, Document>,
) -> Result<()> {
    for d in documents.values() {
        let body = canonical_body(d)?;
        match d.reference.kind.as_str() {
            "workflow-source-enrollment" => {
                let e = SourceEnrollment::parse(&d.bytes)
                    .map_err(|_| ServiceError::IntegrityFailure)?;
                if e.canonical_bytes() != d.bytes
                    || json!(e.reference()) != json!(d.reference)
                    || e.scope() != &key.scope
                {
                    return Err(ServiceError::IntegrityFailure);
                }
                let root = canonical_body(doc(documents, &body["authority_ref"])?)?;
                if root["basis"]["principal_id"] != body["enrolled_by"]
                    || root["basis"]["source"] != e.ceiling().source()
                    || e.ceiling().principal_id() != text(&body, "enrolled_by")?
                {
                    return Err(ServiceError::IntegrityFailure);
                }
                let mut customers = vec![];
                for r in body["customer_refs"]
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    let c = doc(documents, r)?;
                    if c.reference.kind != "workflow-customer" {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    let customer = canonical_body(c)?;
                    if customer["installation_mode"] != body["installation_mode"] {
                        return Err(ServiceError::IntegrityFailure);
                    }
                    customers.push(text(&customer, "customer")?.to_string());
                }
                canonical::sort_set(&mut customers).map_err(|_| ServiceError::IntegrityFailure)?;
                let evidence = doc(documents, &body["evidence_ref"])?;
                if evidence.reference.kind != "workflow-evidence" {
                    return Err(ServiceError::IntegrityFailure);
                }
                let proof = canonical_body(evidence)?;
                let input = evidence_input(&proof)?;
                if body["ceiling"]["customer_scope"] != json!(customers)
                    || input.purpose() != "source_enrollment"
                    || !customers.contains(&input.customer().to_string())
                    || input.metadata()["principal_id"] != body["enrolled_by"]
                    || input.metadata()["source"] != e.ceiling().source()
                    || proof["retained_by"] != body["enrolled_by"]
                    || proof["installation_mode"] != body["installation_mode"]
                {
                    return Err(ServiceError::IntegrityFailure);
                }
            }
            "workflow-source-grant" => {
                let g = SourceGrant::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
                let original = doc(documents, &body["original_ceiling_ref"])?;
                let e = SourceEnrollment::parse(&original.bytes)
                    .map_err(|_| ServiceError::IntegrityFailure)?;
                if g.canonical_bytes() != d.bytes
                    || g.document_id() != d.reference.id
                    || g.content_digest() != d.reference.digest
                    || g.original_ceiling_ref() != e.reference()
                    || g.grant_id() != e.enrollment_id()
                    || !g.ceiling().is_subset_of(e.ceiling())
                {
                    return Err(ServiceError::IntegrityFailure);
                }
            }
            "workflow-source-state" => {
                let original = doc(documents, &body["enrollment_ref"])?;
                let e = SourceEnrollment::parse(&original.bytes)
                    .map_err(|_| ServiceError::IntegrityFailure)?;
                let s =
                    SourceState::parse(&d.bytes, &e).map_err(|_| ServiceError::IntegrityFailure)?;
                if s.canonical_bytes() != d.bytes
                    || json!(s.reference()) != json!(d.reference)
                    || s.logical_store_id() != key.logical_store_id
                    || s.operation_key()
                        .map_err(|_| ServiceError::IntegrityFailure)?["source"]
                        != e.ceiling().source()
                    || body["actor"] != e.ceiling().principal_id()
                {
                    return Err(ServiceError::IntegrityFailure);
                }
                let current = body
                    .get("current_grant_ref")
                    .map(|r| {
                        doc(documents, r).and_then(|d| {
                            SourceGrant::parse(&d.bytes).map_err(|_| ServiceError::IntegrityFailure)
                        })
                    })
                    .transpose()?;
                let prior = if let Some(r) = body.get("prior_state_ref") {
                    let p = doc(documents, r)?;
                    let state = SourceState::parse(&p.bytes, &e)
                        .map_err(|_| ServiceError::IntegrityFailure)?;
                    let pbody = canonical_body(p)?;
                    let grant =
                        SourceGrant::parse(&doc(documents, &pbody["current_grant_ref"])?.bytes)
                            .map_err(|_| ServiceError::IntegrityFailure)?;
                    Some((state, grant))
                } else {
                    None
                };
                s.validate_lineage(&e, current.as_ref(), prior.as_ref().map(|(s, g)| (s, g)))
                    .map_err(|_| ServiceError::IntegrityFailure)?;
            }
            _ => {}
        }
    }
    Ok(())
}
pub(super) fn verify_result_kinds(
    payload: &Value,
    kind: &str,
    results: &[RecordRef],
) -> Result<()> {
    let mut expected = if kind == "evidence.retain" {
        vec!["workflow-evidence"]
    } else {
        match payload["action"].as_str() {
            Some("customer.register") => vec!["workflow-customer"],
            Some("source.enroll") => vec![
                "workflow-source-enrollment",
                "workflow-source-grant",
                "workflow-source-state",
            ],
            Some("source.restrict") => vec!["workflow-source-grant", "workflow-source-state"],
            Some("source.revoke") => vec!["workflow-source-state"],
            _ => return Err(ServiceError::IntegrityFailure),
        }
    };
    expected.sort();
    let mut actual: Vec<_> = results.iter().map(|r| r.kind.as_str()).collect();
    actual.sort();
    if actual != expected {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut sorted = results.to_vec();
    records::sort_refs(&mut sorted)?;
    if sorted != results {
        return Err(ServiceError::IntegrityFailure);
    }
    Ok(())
}
fn document_map(documents: &[Document]) -> std::collections::BTreeMap<String, Document> {
    documents
        .iter()
        .map(|d| {
            (
                d.reference.id.clone(),
                Document {
                    reference: d.reference.clone(),
                    schema: d.schema.clone(),
                    bytes: d.bytes.clone(),
                },
            )
        })
        .collect()
}
pub(super) fn verify_intent(
    key: &OperationKey,
    payload: &Value,
    results: &[RecordRef],
    documents: &[Document],
) -> Result<Vec<u8>> {
    let map = document_map(documents);
    let sr = results
        .iter()
        .find(|r| r.kind == "workflow-source-state")
        .ok_or(ServiceError::IntegrityFailure)?;
    let state_doc = map.get(&sr.id).ok_or(ServiceError::IntegrityFailure)?;
    let state_body = canonical_body(state_doc)?;
    let original = doc(&map, &state_body["enrollment_ref"])?;
    let e = SourceEnrollment::parse(&original.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
    let state =
        SourceState::parse(&state_doc.bytes, &e).map_err(|_| ServiceError::IntegrityFailure)?;
    if let Some(current) = state.current_grant_ref() {
        if !results.iter().any(|r| json!(r) == json!(current)) {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    if e.scope() != &key.scope || state.logical_store_id() != key.logical_store_id {
        return Err(ServiceError::IntegrityFailure);
    }
    if payload["action"] == "source.enroll" {
        let body = canonical_body(original)?;
        if e.enrollment_id() != text(payload, "enrollment_id")?
            || body["customer_refs"] != payload["customer_refs"]
            || body["ceiling"] != payload["ceiling"]
            || body["evidence_ref"] != payload["evidence_ref"]
            || !results.iter().any(|r| r == &original.reference)
            || state.revision() != 1
            || state.status() != "active"
        {
            return Err(ServiceError::IntegrityFailure);
        }
    } else {
        let expected = revision(text(payload, "expected_revision")?)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        if payload["enrollment_ref"] != json!(e.reference())
            || state.revision() != expected as u64 + 1
            || (payload["action"] == "source.restrict") != (state.status() == "active")
        {
            return Err(ServiceError::IntegrityFailure);
        }
        if state.status() == "active" {
            let grant_doc = doc(&map, &state_body["current_grant_ref"])?;
            let g =
                SourceGrant::parse(&grant_doc.bytes).map_err(|_| ServiceError::IntegrityFailure)?;
            if g.ceiling()
                .canonical_value()
                .map_err(|_| ServiceError::IntegrityFailure)?
                != payload["replacement_ceiling"]
                || !results.iter().any(|r| r == &grant_doc.reference)
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
    }
    semantic(payload, e.enrollment_id()).map_err(|_| ServiceError::IntegrityFailure)
}
pub(super) fn verify_original(
    key: &OperationKey,
    payload: &Value,
    results: &[RecordRef],
    documents: &[Document],
    operation: &Value,
    authority: &Value,
    root: &RecordRef,
) -> Result<()> {
    let map = document_map(documents);
    let sr = results
        .iter()
        .find(|r| r.kind == "workflow-source-state")
        .ok_or(ServiceError::IntegrityFailure)?;
    let body = canonical_body(map.get(&sr.id).ok_or(ServiceError::IntegrityFailure)?)?;
    if body["operation_key"] != json!(key)
        || body["actor"] != authority["principal_id"]
        || body["accepted_at"] != operation["accepted_at"]
    {
        return Err(ServiceError::IntegrityFailure);
    }
    if payload["action"] == "source.enroll" {
        let e = canonical_body(doc(&map, &body["enrollment_ref"])?)?;
        if e["enrolled_by"] != authority["principal_id"]
            || e["enrolled_at"] != operation["accepted_at"]
            || e["authority_ref"] != json!(root)
        {
            return Err(ServiceError::IntegrityFailure);
        }
    }
    Ok(())
}
