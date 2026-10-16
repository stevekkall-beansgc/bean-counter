//! Fresh admission checks under the actual held local writer. No caller supplies
//! an authenticated grant, current head, actor, clock or installation mode.
use super::super::*;
use super::{AcceptInput, resolve::PublicationInputs};
use crate::store::ports::AcceptanceTx;
use crate::{
    service::billing::Setup,
    service::workflow::{local::LocalWorkflowLedger, request::OperationKey},
};

pub(super) struct Admission {
    pub(super) setup: Setup,
    pub(super) at: Timestamp,
    pub(super) source: context::ObservedSource,
    pub(super) documents: Vec<Document>,
}
async fn proof(tx: &mut SqliteTx, key: &OperationKey, r: &RecordRef) -> Result<Document> {
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before authority proof kind={} id={}",
        key.operation_id, r.kind, r.id
    ));
    verify::document_origin(tx, key, r).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after authority proof kind={} id={}",
        key.operation_id, r.kind, r.id
    ));
    loaded(tx, key, r).await
}
fn ref_value(v: &Value) -> Result<RecordRef> {
    serde_json::from_value(v.clone()).map_err(|_| reject("INVALID_REFERENCE"))
}
fn private_body<'a>(docs: &'a [Document], r: &RecordRef) -> Result<&'a Document> {
    docs.iter()
        .find(|d| d.reference == *r)
        .ok_or_else(|| reject("INVALID_REFERENCE"))
}
fn contains(v: &Value, s: &str) -> bool {
    v.as_array()
        .is_some_and(|a| a.iter().any(|x| x.as_str() == Some(s)))
}
fn selected(v: &Value, name: &str) -> bool {
    v.as_array().is_some_and(|selectors| {
        selectors.iter().any(|s| {
            contains(&s["names"], name)
                || s["namespace"]
                    .as_str()
                    .is_some_and(|prefix| name.starts_with(prefix))
        })
    })
}
/// Original accepted causal rights are constrained by the exact same selected
/// catalog/instantiation/policy. This is data compatibility, never a grant.
fn causal_coverage(
    binding: &Value,
    terms: &Value,
    definitions: &PublicationInputs,
    docs: &[Document],
) -> Result<()> {
    if !matches!(
        terms["schema"].as_str(),
        Some("ledger-workflow-slot-binding-terms/2" | "ledger-workflow-slot-binding-terms/3")
    ) {
        return Ok(());
    }
    let body = |field: &str| -> Result<Value> {
        let r = ref_value(&binding[field])?;
        canonical::parse(&private_body(docs, &r)?.bytes).map_err(|_| ServiceError::IntegrityFailure)
    };
    let catalog = body("slot_catalog_ref")?;
    let instantiated = body("slot_instantiation_ref")?;
    fn array(v: &Value) -> Result<&Vec<Value>> {
        v.as_array().ok_or(ServiceError::IntegrityFailure)
    }
    let clauses = array(&terms["causal_reversal_clauses"])?;
    let components = array(&catalog["components"])?;
    let families = array(&catalog["families"])?;
    let component_map = array(&instantiated["component_map"])?;
    let family_map = array(&instantiated["family_map"])?;
    let selected = array(&definitions.policy["families"])?;
    for clause in clauses {
        let root = components
            .iter()
            .find(|c| c["slot_id"] == clause["root_component_slot"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let family = families
            .iter()
            .find(|f| f["slot_id"] == clause["family_slot"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let root_mapping = component_map
            .iter()
            .find(|m| m["slot_id"] == root["slot_id"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let family_mapping = family_map
            .iter()
            .find(|m| m["slot_id"] == family["slot_id"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let policy_family = selected
            .iter()
            .find(|f| f["family_key"] == family_mapping["template_key"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        require(
            root["root_slot"] == root["slot_id"]
                && root["book"] == terms["book"]
                && root["role_slot"] == terms["role_slot"]
                && family["target_component_slot"] == root["slot_id"]
                && policy_family["selector"]["component_key"] == root_mapping["template_key"]
                && policy_family["selector"]["binding_slot"] == terms["slot"]
                && policy_family["correction_source_slot"] == clause["correction_source_slot"],
            "AUTHORITY_REQUIRED",
        )?;
    }
    // Presence is required even for a potentially zero/none/unclaimed family.
    for family in selected
        .iter()
        .filter(|f| f["selector"]["binding_slot"] == terms["slot"])
    {
        let root = component_map
            .iter()
            .find(|m| m["template_key"] == family["selector"]["component_key"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        let mapped = family_map
            .iter()
            .find(|m| m["template_key"] == family["family_key"])
            .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        require(
            clauses.iter().any(|c| {
                c["root_component_slot"] == root["slot_id"] && c["family_slot"] == mapped["slot_id"]
            }),
            "AUTHORITY_REQUIRED",
        )?;
    }
    Ok(())
}
/// Data compatibility only; the caller has separately normalized the private
/// documents and authenticated source/assent/funding dependencies.
fn coverage(binding: &Value, definitions: &PublicationInputs, docs: &[Document]) -> Result<()> {
    let entries = binding["bindings"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    let slots = definitions.policy["binding_slots"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require(entries.len() == slots.len(), "INVALID_REFERENCE")?;
    let product = definitions.product.canonical_value();
    let template = definitions.template.canonical_value();
    for entry in entries {
        let reference = ref_value(&entry["binding_ref"])?;
        let terms = canonical::parse(&private_body(docs, &reference)?.bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let slot = slots
            .iter()
            .find(|s| s["name"] == entry["slot"])
            .ok_or_else(|| reject("INVALID_REFERENCE"))?;
        require(
            terms["slot"] == slot["name"]
                && terms["book"] == slot["book"]
                && terms["funding_leg"] == slot["funding_leg"]
                && terms["correction_source_slot"] == slot["correction_source_slot"]
                && terms["allowed_activities"]
                    .as_array()
                    .is_some_and(|activities| {
                        activities.iter().all(|activity| {
                            activity
                                .as_str()
                                .is_some_and(|name| contains(&slot["allowed_activities"], name))
                        })
                    })
                && binding["funding_legs"]
                    .as_array()
                    .is_some_and(|legs| legs.iter().any(|leg| leg["name"] == terms["funding_leg"])),
            "INVALID_REFERENCE",
        )?;
        causal_coverage(binding, &terms, definitions, docs)?;
        for rule in definitions.policy["rules"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
            .iter()
            .filter(|r| r["binding_slot"] == entry["slot"])
        {
            let activity = text(rule, "activity")?;
            let declaration = product["activities"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .iter()
                .find(|a| a["name"] == activity)
                .ok_or_else(|| reject("INVALID_REFERENCE"))?;
            let component = template["components"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .iter()
                .find(|c| c["key"] == rule["component_key"])
                .ok_or_else(|| reject("INVALID_REFERENCE"))?;
            require(
                contains(&terms["allowed_activities"], activity)
                    && contains(&slot["allowed_activities"], activity)
                    && terms["role_slot"] == component["role_slot"]
                    && terms["book"] == component["book"]
                    && binding["source_slots"].as_array().is_some_and(|sources| {
                        sources
                            .iter()
                            .any(|source| source["name"] == declaration["source_slot"])
                    }),
                "INVALID_REFERENCE",
            )?;
        }
    }
    Ok(())
}
pub(super) async fn check(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: &AcceptInput,
    definitions: &PublicationInputs,
    private_documents: &[Document],
) -> Result<Admission> {
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!("{} before actual fresh admin", key.operation_id));
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!("{} after actual fresh admin", key.operation_id));
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!("{} before installation load", key.operation_id));
    let installation = tx
        .load_installation()
        .await
        .map_err(crate::service::store_error)?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!("{} after installation load", key.operation_id));
    require(installation.mode == "real", "INSTALLATION_MODE_MISMATCH")?;
    let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let normalized = input.value()?;
    let binding = &normalized["binding_inputs"];
    let grants = binding["authority_refs"]
        .as_array()
        .ok_or_else(|| reject("INVALID_SHAPE"))?;
    // One actor/customer enrollment is the selected initial profile. Never
    // invent a scalar revision by taking the maximum across unrelated states.
    require(grants.len() == 1, "UNSUPPORTED_COMPOSITION")?;
    let requested = ref_value(&grants[0])?;
    let granted = proof(tx, key, &requested).await?;
    let declared = ledgerlab_core::policy::workflow::SourceGrant::parse(&granted.bytes)
        .map_err(typed_error)?;
    let enrollment = ref_value(&json!(declared.original_ceiling_ref()))?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before latest enrolled source observation",
        key.operation_id
    ));
    let source = context::observe_source(tx, key, &enrollment).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after latest enrolled source observation",
        key.operation_id
    ));
    let documents = verify_capture(
        tx,
        key,
        input,
        definitions,
        private_documents,
        AdmissionContext {
            actor: &setup.operator,
            at: &at,
            source: &source,
        },
    )
    .await?;
    Ok(Admission {
        setup,
        at,
        source,
        documents,
    })
}

/// Captured original actor/time and origin-verified pinned source. These data
/// arguments do not authorize a current write. Replay must authenticate the
/// original root/authority and source refs before calling this verifier.
pub(in crate::service::workflow::agreement) struct AdmissionContext<'a> {
    pub(in crate::service::workflow::agreement) actor: &'a str,
    pub(in crate::service::workflow::agreement) at: &'a Timestamp,
    pub(in crate::service::workflow::agreement) source: &'a context::ObservedSource,
}
/// Reapplies immutable eligibility/evidence relationships without loading
/// current setup, clock, latest source head, compiler or executable identity.
pub(in crate::service::workflow::agreement) async fn verify_capture(
    tx: &mut SqliteTx,
    key: &OperationKey,
    input: &AcceptInput,
    definitions: &PublicationInputs,
    private_documents: &[Document],
    capture: AdmissionContext<'_>,
) -> Result<Vec<Document>> {
    let source = capture.source;
    let normalized = input.value()?;
    let binding = &normalized["binding_inputs"];
    require(
        binding["authority_refs"].as_array().map(Vec::len) == Some(1),
        "UNSUPPORTED_COMPOSITION",
    )?;
    coverage(binding, definitions, private_documents)?;
    let requested = ref_value(&binding["authority_refs"][0])?;
    let current = source.grant().ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let current_ref = records::source_document(
        "workflow-source-grant",
        current.canonical_value().map_err(typed_error)?,
    )?
    .reference;
    require(
        current_ref == requested
            && current.ceiling().principal_id() == capture.actor
            && current.ceiling().source() == key.source
            && current.ceiling().canonical_economic_namespace() == binding["source_namespace"]
            && current.ceiling().contains_time(capture.at),
        "AUTHORITY_REQUIRED",
    )?;
    let ceiling = current.ceiling().canonical_value().map_err(typed_error)?;
    require(
        contains(&ceiling["customer_scope"], input.customer())
            && contains(&ceiling["permissions"], "read")
            && contains(&ceiling["permissions"], "submit")
            && contains(&ceiling["permissions"], "correct")
            && contains(&ceiling["operations"], "work.accept")
            && contains(&ceiling["operations"], "quantity.correct")
            && selected(
                &ceiling["product_selectors"],
                definitions.product.canonical_value()["product_id"]
                    .as_str()
                    .ok_or(ServiceError::IntegrityFailure)?,
            ),
        "AUTHORITY_REQUIRED",
    )?;
    let customer_id = tx
        .workflow_agreement_customer(&key.logical_store_id, &key.scope, input.customer())
        .await
        .map_err(crate::service::store_error)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    let customer = tx
        .workflow_document_by_id(
            &customer_id,
            &key.logical_store_id,
            &key.scope[0],
            &key.scope[1],
        )
        .await
        .map_err(crate::service::store_error)?
        .ok_or(ServiceError::IntegrityFailure)?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before registered customer origin",
        key.operation_id
    ));
    verify::document_origin(tx, key, &customer.reference).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after registered customer origin",
        key.operation_id
    ));
    let registration = canonical_body(&customer)?;
    require(
        registration["customer"] == input.customer()
            && registration["installation_mode"] == "real_local"
            && source
                .enrollment()
                .customer_refs()
                .iter()
                .any(|r| json!(r) == json!(customer.reference)),
        "AUTHORITY_REQUIRED",
    )?;
    let mut roots = vec![customer.reference.clone()];
    for row in binding["source_slots"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let name = text(row, "name")?;
        require(
            contains(&ceiling["source_slots"], name)
                && row["sources"]
                    .as_array()
                    .is_some_and(|a| a.len() == 1 && a[0] == key.source),
            "AUTHORITY_REQUIRED",
        )?;
    }
    for entry in binding["bindings"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let r = ref_value(&entry["binding_ref"])?;
        let terms = canonical::parse(&private_body(private_documents, &r)?.bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        let slot = definitions.policy["binding_slots"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
            .iter()
            .find(|s| s["name"] == entry["slot"])
            .ok_or_else(|| reject("INVALID_REFERENCE"))?;
        require(
            terms["slot"] == entry["slot"]
                && terms["book"] == "retail"
                && terms["book"] == slot["book"]
                && terms["funding_leg"] == slot["funding_leg"]
                && contains(
                    &ceiling["source_slots"],
                    text(&terms, "correction_source_slot")?,
                ),
            "AUTHORITY_REQUIRED",
        )?;
        for activity in terms["allowed_activities"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            require(
                selected(
                    &ceiling["activity_selectors"],
                    activity.as_str().ok_or(ServiceError::IntegrityFailure)?,
                ),
                "AUTHORITY_REQUIRED",
            )?;
        }
        for original in terms["source_grant_refs"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?
        {
            let r = ref_value(original)?;
            let doc = proof(tx, key, &r).await?;
            let grant = ledgerlab_core::policy::workflow::SourceGrant::parse(&doc.bytes)
                .map_err(typed_error)?;
            require(
                grant.original_ceiling_ref() == source.enrollment().reference(),
                "AUTHORITY_REQUIRED",
            )?;
            roots.push(r);
        }
    }
    for r in binding["assent_refs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let r = ref_value(r)?;
        let assent = canonical::parse(&private_body(private_documents, &r)?.bytes)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        require(
            assent["mode"] == "real"
                && assent["operator"] == capture.actor
                && assent["acceptor"] == registration["acceptor"]
                && assent["terms_digest"] == input.terms().terms_digest(),
            "ASSENT_TERMS_MISMATCH",
        )?;
        let consent_time = Timestamp::parse(text(&assent, "accepted_at")?)
            .map_err(|_| reject("INVALID_SCALAR"))?;
        require(
            consent_time.micros() <= capture.at.micros(),
            "ASSENT_TERMS_MISMATCH",
        )?;
        let consent_ref = ref_value(&assent["evidence_ref"])?;
        let attestation_ref = ref_value(&assent["operator_attestation_ref"])?;
        let consent = proof(tx, key, &consent_ref).await?;
        let attestation = proof(tx, key, &attestation_ref).await?;
        for (doc, purpose) in [(&consent, "assent"), (&attestation, "operator_attestation")] {
            let body = canonical_body(doc)?;
            let evidence = evidence_input(&body)?;
            require(
                evidence.purpose() == purpose
                    && evidence.customer() == input.customer()
                    && evidence.agreement_id() == Some(input.agreement_id())
                    && body["installation_mode"] == "real_local"
                    && body["retained_by"] == capture.actor
                    && evidence.metadata()["terms_digest"] == input.terms().terms_digest()
                    && evidence.metadata()["acceptor"] == registration["acceptor"],
                "ASSENT_TERMS_MISMATCH",
            )?;
        }
        let consent_body = canonical_body(&consent)?;
        let attestation_body = canonical_body(&attestation)?;
        require(
            consent_body["metadata"]["accepted_at"] == assent["accepted_at"]
                && attestation_body["metadata"]["assent_evidence_ref"] == json!(consent_ref)
                && attestation_body["metadata"]["statement"]
                    == "operator_verified_exact_terms_acceptance",
            "ASSENT_TERMS_MISMATCH",
        )?;
        roots.extend([consent_ref, attestation_ref]);
    }
    for leg in binding["funding_legs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        require(
            leg["mode"] == "customer_local"
                && leg["resource_owner"] == input.customer()
                && leg["payer"] == input.customer()
                && leg.get("provider").is_none()
                && leg.get("supplier_binding_slot").is_none(),
            "UNSUPPORTED_COMPOSITION",
        )?;
        let evidence = leg["evidence_refs"]
            .as_array()
            .ok_or(ServiceError::IntegrityFailure)?;
        require(!evidence.is_empty(), "AUTHORITY_REQUIRED")?;
        for r in evidence {
            let r = ref_value(r)?;
            let doc = proof(tx, key, &r).await?;
            let body = canonical_body(&doc)?;
            let proof = evidence_input(&body)?;
            require(
                proof.purpose() == "funding"
                    && proof.customer() == input.customer()
                    && proof.agreement_id() == Some(input.agreement_id())
                    && body["installation_mode"] == "real_local"
                    && proof.metadata()["mode"] == leg["mode"]
                    && proof.metadata()["resource_owner"] == leg["resource_owner"]
                    && proof.metadata()["payer"] == leg["payer"],
                "AUTHORITY_REQUIRED",
            )?;
            roots.push(r);
        }
    }
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} before authority full root closure",
        key.operation_id
    ));
    let mut documents = verify::authenticated_closure(tx, key, &roots).await?;
    #[cfg(test)]
    verify::diagnostic_checkpoint(&format!(
        "{} after authority full root closure",
        key.operation_id
    ));
    documents.extend(source.documents().iter().map(|d| Document {
        reference: d.reference.clone(),
        schema: d.schema.clone(),
        bytes: d.bytes.clone(),
    }));
    Ok(documents)
}

#[cfg(test)]
mod tests;
