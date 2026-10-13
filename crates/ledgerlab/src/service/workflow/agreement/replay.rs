//! Original acceptance reconstruction. No current head, clock, build or grant
//! is consulted. The certificate caller authenticates captured authority.
use super::super::request::OperationKey;
use super::acceptance::{AcceptInput, PublicationInputs};
use super::*;
/// Closed scoped data traversal only. The retained certificate worklist proves
/// all returned origins once; fresh callers authenticate the root certificate
/// before using this traversal. No caller cache or trust flag replaces proof.
pub(super) async fn predecessor_closure(
    tx: &mut SqliteTx,
    key: &OperationKey,
    head: &RecordRef,
) -> Result<Vec<Document>> {
    let mut todo = vec![head.clone()];
    let mut docs = std::collections::BTreeMap::<String, Document>::new();
    let mut bytes = 0usize;
    while let Some(r) = todo.pop() {
        if let Some(d) = docs.get(&r.id) {
            if d.reference != r {
                return Err(ServiceError::IntegrityFailure);
            }
            continue;
        }
        if docs.len() >= 4096 {
            return Err(ServiceError::IntegrityFailure);
        }
        let d = loaded(tx, key, &r).await?;
        let body = canonical::parse_bounded(
            &d.bytes,
            if r.kind == "workflow-compiled-plan" {
                4194304
            } else {
                262144
            },
        )
        .map_err(|_| ServiceError::IntegrityFailure)?;
        if records::bytes(&body)? != d.bytes || body["schema"] != d.schema {
            return Err(ServiceError::IntegrityFailure);
        }
        let mut deps = vec![];
        let mut add = |v: &Value| -> Result<()> {
            deps.push(
                serde_json::from_value::<RecordRef>(v.clone())
                    .map_err(|_| ServiceError::IntegrityFailure)?,
            );
            Ok(())
        };
        match r.kind.as_str() {
            "workflow-head-transition" => {
                let saved = OperationKey::parse(&records::bytes(&body["operation_key"])?)
                    .map_err(|_| ServiceError::IntegrityFailure)?;
                let revision = text(&body, "new_revision")?
                    .parse::<u64>()
                    .map_err(|_| ServiceError::IntegrityFailure)?;
                if saved.logical_store_id != key.logical_store_id
                    || saved.scope != key.scope
                    || record::semantic_document(
                        &r.kind,
                        &saved,
                        text(&body, "customer")?,
                        text(&body, "agreement_id")?,
                        revision,
                        body.clone(),
                    )?
                    .reference
                        != r
                {
                    return Err(ServiceError::IntegrityFailure);
                }
                add(&body["accepted_binding_ref"])?;
                if let Some(prior) = body.get("prior_head_ref") {
                    add(prior)?;
                }
            }
            "workflow-binding" => {
                canonical_body(&d)?;
                for field in [
                    "slot_catalog_ref",
                    "slot_instantiation_ref",
                    "policy_ref",
                    "product_ref",
                    "compiled_plan_ref",
                ] {
                    add(&body[field])?;
                }
                for field in [
                    "roles_refs",
                    "assent_refs",
                    "authority_refs",
                    "fact_schema_refs",
                ] {
                    for r in body[field]
                        .as_array()
                        .ok_or(ServiceError::IntegrityFailure)?
                    {
                        add(r)?;
                    }
                }
                add(&body["accepted_context"]["fact_schema_ref"])?;
                for b in body["bindings"]
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    add(&b["binding_ref"])?;
                }
                for leg in body["funding_legs"]
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    for r in leg["evidence_refs"]
                        .as_array()
                        .ok_or(ServiceError::IntegrityFailure)?
                    {
                        add(r)?;
                    }
                }
            }
            "workflow-slot-instantiation" => {
                canonical_body(&d)?;
                add(&body["template_ref"])?;
                add(&body["catalog_ref"])?;
            }
            "workflow-slot-terms" => {
                canonical_body(&d)?;
                add(&body["role_ref"])?;
                for r in body["source_grant_refs"]
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    add(r)?;
                }
            }
            "workflow-assent" => {
                canonical_body(&d)?;
                add(&body["evidence_ref"])?;
                add(&body["operator_attestation_ref"])?;
            }
            "workflow-roles" | "workflow-slot-catalog" => {
                canonical_body(&d)?;
            }
            "workflow-compiled-plan" => {
                let digest = records::hash("workflow-plan", 1, &body)?;
                if r.id != format!("wplan_{}", &digest[7..]) || r.digest != digest {
                    return Err(ServiceError::IntegrityFailure);
                }
                add(&body["compiler_ref"])?;
                for r in body["source_refs"]
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    add(r)?;
                }
            }
            "workflow-policy"
            | "workflow-product"
            | "workflow-slot-template"
            | "workflow-fact-schema"
            | "workflow-compiler-descriptor" => {
                canonical_body(&d)?;
                deps.extend(
                    ledgerlab_core::policy::workflow::candidate_dependency_refs(&d.bytes)
                        .map_err(|_| ServiceError::IntegrityFailure)?
                        .into_iter()
                        .map(|r| RecordRef {
                            kind: r.kind().into(),
                            id: r.id().into(),
                            digest: r.digest().into(),
                        }),
                );
            }
            // Existing closed administrative recipes validate lineage, evidence
            // and their own transitive root dependencies without a new variant.
            "workflow-authority"
            | "workflow-source-grant"
            | "workflow-source-enrollment"
            | "workflow-source-state"
            | "workflow-evidence"
            | "workflow-customer" => {
                for proof in verify::closure(tx, key, &[r.clone()]).await? {
                    if !docs.contains_key(&proof.reference.id) {
                        todo.push(proof.reference);
                    }
                }
            }
            _ => return Err(ServiceError::IntegrityFailure),
        }
        bytes = bytes
            .checked_add(d.bytes.len())
            .ok_or(ServiceError::IntegrityFailure)?;
        if bytes > 8388608 {
            return Err(ServiceError::IntegrityFailure);
        }
        todo.extend(deps);
        docs.insert(r.id, d);
    }
    Ok(docs.into_values().collect())
}

pub(super) struct AcceptanceData {
    pub(super) input: AcceptInput,
    pub(super) definitions: PublicationInputs,
    pub(super) publication: context::ObservedPublication,
    pub(super) private_documents: Vec<Document>,
    pub(super) descriptors: Vec<RecordRef>,
}
pub(super) async fn acceptance_data(
    tx: &mut SqliteTx,
    key: &OperationKey,
    req: &request::Request,
) -> Result<AcceptanceData> {
    let publication_ref: RecordRef =
        serde_json::from_value(req.payload()["publication_ref"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
    let publication = context::observe_publication(tx, key, &publication_ref).await?;
    let definitions = PublicationInputs::original(&publication)?;
    let mut input = AcceptInput::parse(
        &records::bytes(req.payload())?,
        &key.scope,
        &definitions.context,
    )?;
    let mut retained = vec![];
    for r in input.private_refs()? {
        if let Some(d) = tx
            .workflow_document(
                &r,
                &key.logical_store_id,
                &key.scope[0],
                &key.scope[1],
                None,
            )
            .await
            .map_err(crate::service::store_error)?
        {
            retained.push(d);
        }
    }
    let private_documents = input.normalize_private_documents(
        &definitions.template,
        &definitions.product,
        &retained,
    )?;
    let mut descriptors = publication
        .sources()
        .filter(|d| {
            matches!(
                d.reference.kind.as_str(),
                "workflow-fact-schema" | "workflow-compiler-descriptor"
            )
        })
        .map(|d| d.reference.clone())
        .collect();
    records::sort_refs(&mut descriptors)?;
    Ok(AcceptanceData {
        input,
        definitions,
        publication,
        private_documents,
        descriptors,
    })
}
