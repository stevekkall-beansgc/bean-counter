//! Exact reviewed descriptor and journal bytes; no authority is acquired here.
use super::*;
use crate::service::billing::Setup;

pub(super) fn locators(req: &request::Request, payload: &Value) -> Result<Vec<Value>> {
    let mut out = vec![];
    match (req.kind(), payload["schema"].as_str()) {
        ("evidence.retain", Some("ledger-workflow-evidence-retain/1")) => match payload["purpose"]
            .as_str()
        {
            Some("operator_attestation") => out
                .push(json!({"role":"evidence","ref":payload["metadata"]["assent_evidence_ref"]})),
            Some("admission_stop") => {
                out.push(
                    json!({"role":"accepted_binding","ref":payload["metadata"]["binding_ref"]}),
                );
                out.push(json!({"role":"activation_target","ref":payload["metadata"]["target_transition_ref"]}));
            }
            _ => {}
        },
        ("authority.change", Some("ledger-workflow-customer-register/1")) => {
            out.push(json!({"role":"evidence","ref":payload["registration_evidence_ref"]}))
        }
        ("authority.change", Some("ledger-workflow-source-enroll/1")) => {
            for r in payload["customer_refs"]
                .as_array()
                .ok_or_else(|| reject("INVALID_SHAPE"))?
            {
                out.push(json!({"role":"customer","ref":r}));
            }
            out.push(json!({"role":"evidence","ref":payload["evidence_ref"]}));
        }
        (
            "authority.change",
            Some("ledger-workflow-source-restrict/1" | "ledger-workflow-source-revoke/1"),
        ) => out.push(json!({"role":"source_enrollment","ref":payload["enrollment_ref"]})),
        _ => return Err(reject("UNSUPPORTED_SCHEMA_PROFILE")),
    }
    canonical::sort_set(&mut out).map_err(|_| reject("INVALID_SHAPE"))?;
    Ok(out)
}
pub(super) fn evidence(req: &request::Request, normalized_payload: &Value) -> Result<Value> {
    let locators = locators(req, normalized_payload)?;
    let mut anchors = vec![];
    for l in &locators {
        let role = l["role"].as_str().ok_or(ServiceError::IntegrityFailure)?;
        let relation = match role {
            "evidence" => "evidence_input",
            "customer" => "customer_registration",
            "source_enrollment" => "source_enrollment",
            "accepted_binding" => "accepted_binding",
            "activation_target" => "activation_target",
            _ => return Err(ServiceError::IntegrityFailure),
        };
        anchors.push(
            json!({"role":role,"record_ref":l["ref"],"relation":relation,"descriptor_refs":[]}),
        );
    }
    canonical::sort_set(&mut anchors).map_err(|_| ServiceError::IntegrityFailure)?;
    let registry=canonical::parse_bounded(include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/agreement-ready-registry.json"),65536).map_err(|_|ServiceError::IntegrityFailure)?;
    let mut resolution = json!({"schema":"ledger-operation-descriptors/1","registry_digest":format!("sha256:{}",records::raw_hash(&records::bytes(&registry)?)),"operation_kind":req.kind(),"payload_schema":normalized_payload["schema"],"locator_digest":records::hash("descriptor-locators",1,&json!([req.kind(),normalized_payload["schema"],locators]))?,"anchors":anchors,"descriptors":[],"normalization_profile":"ledger-operation-normalization/2"});
    resolution["closure_digest"] = json!(records::hash("descriptor-closure", 1, &resolution)?);
    let normalized = req.normalized_envelope(normalized_payload);
    Ok(
        json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":super::super::publication::base64(req.raw()),"original_request_bytes_sha256":records::raw_hash(req.raw()),"normalized_request":normalized,"request_hash":records::hash("operation-request",2,&json!([resolution["closure_digest"],normalized]))?,"schema_bundle_digest":records::hash("schema-bundle",1,&json!([]))?,"descriptor_resolution":resolution}),
    )
}
pub(super) fn basis(setup: &Setup) -> Value {
    json!({"schema":"ledger-workflow-local-authority-basis/1","logical_store_id":setup.store_id,"scope":setup.scope,"principal_id":setup.operator,"source":setup.source,"canonical_economic_namespace":setup.source,"rights":["admin","read"],"source_grant_revision":setup.grant_revision.to_string()})
}
pub(super) fn fresh_root(setup: &Setup, at: &Timestamp) -> Result<Document> {
    let basis = basis(setup);
    records::source_document(
        "workflow-authority",
        json!({"schema":"ledger-workflow-local-authority-snapshot/1","basis":basis,"basis_digest":records::hash("local-authority-basis",1,&basis)?,"verification_profile":"local-private-directory-owner/1","verified_at":at}),
    )
}
/// Separate inputs are already validated under the actual owned transaction.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble(
    req: &request::Request,
    evidence: Value,
    setup: &Setup,
    at: &Timestamp,
    sequence: i64,
    root: Document,
    documents: Vec<Document>,
    results: Vec<RecordRef>,
    semantic_key: Vec<u8>,
    alias_of: Option<Vec<u8>>,
    projection: Projection,
) -> Result<JournalAppend> {
    assemble_recipe(
        req,
        evidence,
        setup,
        at,
        sequence,
        root,
        documents,
        results,
        semantic_key,
        alias_of,
        projection,
        None,
    )
}
/// The source recipe is selected by the compiled coordinator; neither schema
/// nor alias classification is taken from an external prepared journal.
#[allow(clippy::too_many_arguments)]
pub(super) fn assemble_definition(
    req: &request::Request,
    evidence: Value,
    setup: &Setup,
    at: &Timestamp,
    sequence: i64,
    root: Document,
    documents: Vec<Document>,
    results: Vec<RecordRef>,
    semantic_key: Vec<u8>,
    alias_of: Option<Vec<u8>>,
    projection: Projection,
    semantic_alias: bool,
) -> Result<JournalAppend> {
    assemble_recipe(
        req,
        evidence,
        setup,
        at,
        sequence,
        root,
        documents,
        results,
        semantic_key,
        alias_of,
        projection,
        Some(semantic_alias),
    )
}
#[allow(clippy::too_many_arguments)]
fn assemble_recipe(
    req: &request::Request,
    evidence: Value,
    setup: &Setup,
    at: &Timestamp,
    sequence: i64,
    root: Document,
    documents: Vec<Document>,
    mut results: Vec<RecordRef>,
    semantic_key: Vec<u8>,
    alias_of: Option<Vec<u8>>,
    projection: Projection,
    source_alias: Option<bool>,
) -> Result<JournalAppend> {
    let key = req.key();
    if canonical_body(&root)?["basis"] != basis(setup) {
        return Err(ServiceError::IntegrityFailure);
    }
    let root_ref = root.reference.clone();
    let mut all = std::collections::BTreeMap::new();
    for d in documents.into_iter().chain(std::iter::once(root)) {
        if let Some(prior) = all.insert(d.reference.id.clone(), d) {
            let new = &all[&prior.reference.id];
            if prior.reference != new.reference
                || prior.bytes != new.bytes
                || prior.schema != new.schema
            {
                return Err(ServiceError::IntegrityFailure);
            }
        }
    }
    records::sort_refs(&mut results)?;
    let operation = json!({"schema":"ledger-workflow-operation-record/1","key":key,"kind":req.kind(),"contract":"workflow/1","request_evidence":evidence,"authority_snapshot":{"schema":"ledger-workflow-publication-authority/1","principal_id":setup.operator,"source":setup.source,"authority_refs":[root_ref],"rights":["admin"],"validated_at":at},"accepted_at":at,"commit_sequence":sequence.to_string(),"result_refs":results});
    let operation_ref = records::record_reference("workflow-operation", &key.scope, &operation)?;
    let documents: Vec<_> = all.into_values().collect();
    let mut members: Vec<_> = documents.iter().map(|d| d.reference.clone()).collect();
    members.push(operation_ref.clone());
    records::sort_refs(&mut members)?;
    let manifest = json!({"schema":if source_alias.is_some(){"ledger-workflow-definition-manifest/1"}else{"ledger-workflow-agreement-manifest/1"},"key":key,"members":members});
    let manifest_ref = records::record_reference("workflow-manifest", &key.scope, &manifest)?;
    let mut receipt = json!({"schema":"ledger-operation-receipt/2","key":key,"request_hash":evidence["request_hash"],"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":evidence["schema_bundle_digest"],"descriptor_closure_digest":evidence["descriptor_resolution"]["closure_digest"],"original_request_bytes_sha256":evidence["original_request_bytes_sha256"],"result_kind":if source_alias.unwrap_or(alias_of.is_some()){"semantic_alias"}else{"administrative"},"result_refs":results,"accepted_at":at,"commit_sequence":sequence.to_string(),"manifest_hash":manifest_ref.digest});
    receipt["receipt_hash"] = json!(records::hash("operation-receipt", 2, &receipt)?);
    let receipt_ref = records::record_reference("workflow-receipt", &key.scope, &receipt)?;
    let operation = JournalOperation {
        identity: key.bytes()?,
        kind: req.kind().into(),
        operation_ref,
        operation_bytes: records::bytes(&operation)?,
        request_hash: text(&evidence, "request_hash")?.into(),
        manifest_ref,
        manifest_bytes: records::bytes(&manifest)?,
        receipt_ref,
        receipt_bytes: records::bytes(&receipt)?,
        sequence,
        accepted_at_us: at.micros(),
        members,
    };
    let resolved_bytes = documents
        .iter()
        .try_fold(operation.operation_bytes.len(), |n, d| {
            n.checked_add(d.bytes.len())
        })
        .ok_or_else(|| reject("PROGRAM_BUDGET_EXCEEDED"))?;
    require(
        resolved_bytes <= 8 * 1024 * 1024 && operation.members.len() <= 4096,
        "PROGRAM_BUDGET_EXCEEDED",
    )?;
    Ok(JournalAppend {
        store: key.logical_store_id.clone(),
        tenant: key.scope[0].clone(),
        environment: key.scope[1].clone(),
        source: key.source.clone(),
        operation_id: key.operation_id.clone(),
        documents,
        operation,
        semantic_key,
        alias_of,
        projection,
    })
}
