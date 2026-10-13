//! Fresh transition coordination under the held writer; data helpers grant no rights.
use super::super::*;
use crate::store::ports::AcceptanceTx;
fn copy(d: &Document) -> Document {
    Document {
        reference: d.reference.clone(),
        schema: d.schema.clone(),
        bytes: d.bytes.clone(),
    }
}
use crate::service::workflow::local::LocalWorkflowLedger;
use crate::store::sqlite::workflow::agreement::acceptance::HeadKind;

pub(super) async fn fresh(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    req: &request::Request,
) -> Result<JournalAppend> {
    let key = req.key();
    let sequence = tx
        .workflow_sequence()
        .await
        .map_err(crate::service::store_error)?;
    let binding_ref: RecordRef =
        serde_json::from_value(req.payload()["accepted_binding_ref"].clone())
            .map_err(|_| reject("INVALID_REFERENCE"))?;
    let accepted = context::observe_accepted_binding(tx, key, &binding_ref, sequence).await?;
    let binding = canonical_body(&accepted.binding)?;
    let customer = text(&binding, "customer")?;
    let agreement = text(&binding, "agreement_id")?;
    let payload = activation_verify::normalize(req, &accepted.binding)?;
    let old = text(&payload, "expected_revision")?
        .parse::<i64>()
        .map_err(|_| reject("INVALID_SCALAR"))?;
    let revision = old.checked_add(1).ok_or_else(|| reject("INVALID_SCALAR"))?;
    let semantic_key = records::bytes(&json!([
        customer,
        agreement,
        "activation",
        revision.to_string()
    ]))?;
    if let Some(identity) = tx
        .workflow_agreement_semantic(&key.logical_store_id, &key.scope, req.kind(), &semantic_key)
        .await
        .map_err(crate::service::store_error)?
    {
        let original = tx
            .workflow_agreement_lookup(&identity)
            .await
            .map_err(crate::service::store_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let original_key = super::super::super::request::OperationKey::parse(&identity)?;
        let certificate = verify::retained(tx, &original_key, &original).await?;
        require(
            certificate["request_evidence"]["normalized_request"]["payload"] == payload,
            "SEMANTIC_CONFLICT",
        )?;
        // Dedicated alias/3 constructor is required; no fresh eligibility/CAS here.
        return Err(reject("UNSUPPORTED_SCHEMA_PROFILE"));
    }
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let installation = tx
        .load_installation()
        .await
        .map_err(crate::service::store_error)?;
    require(installation.mode == "real", "INSTALLATION_MODE_MISMATCH")?;
    let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let original_grants = binding["authority_refs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?;
    require(original_grants.len() == 1, "UNSUPPORTED_COMPOSITION")?;
    let original_grant: RecordRef = serde_json::from_value(original_grants[0].clone())
        .map_err(|_| ServiceError::IntegrityFailure)?;
    verify::document_origin(tx, key, &original_grant).await?;
    let original_grant = loaded(tx, key, &original_grant).await?;
    let grant = ledgerlab_core::policy::workflow::SourceGrant::parse(&original_grant.bytes)
        .map_err(typed_error)?;
    let enrollment: RecordRef = serde_json::from_value(json!(grant.original_ceiling_ref()))
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let source = context::observe_source(tx, key, &enrollment).await?;
    let purpose = if req.kind() == "agreement.revoke" {
        "stop"
    } else {
        "admit"
    };
    let mut documents = activation_verify::verify_capture(
        tx,
        key,
        &accepted,
        &source,
        &setup.operator,
        &at,
        &payload,
        purpose,
    )
    .await?;
    let head = tx
        .workflow_agreement_head(
            &key.logical_store_id,
            &key.scope,
            customer,
            agreement,
            HeadKind::Activation,
        )
        .await
        .map_err(crate::service::store_error)?;
    require(
        head.as_ref().map_or(0, |h| h.revision) == old,
        "STALE_REVISION",
    )?;
    let history =
        activation_verify::schedule::history_before(tx, key, customer, agreement, sequence).await?;
    require(
        history.last().map_or(0, |t| t.revision) == old,
        "INTEGRITY_FAILURE",
    )?;
    let target_field = if req.kind() == "agreement.revoke" {
        "transition_ref"
    } else {
        "replaces_transition_ref"
    };
    let target: Option<RecordRef> = payload
        .get(target_field)
        .map(|v| serde_json::from_value(v.clone()).map_err(|_| reject("INVALID_REFERENCE")))
        .transpose()?;
    let built = activation_verify::build_records(
        req,
        &payload,
        &activation_verify::TransitionMetadata {
            key,
            actor: &setup.operator,
            at: &at,
            sequence,
            binding: &accepted.binding,
            prior: head.map(|h| h.transition),
            target,
        },
    )?;
    let new_head = copy(
        built
            .documents
            .iter()
            .find(|d| d.reference.kind == "workflow-head-transition")
            .ok_or(ServiceError::IntegrityFailure)?,
    );
    let new_transition = copy(
        built
            .documents
            .iter()
            .find(|d| d.reference.kind == "workflow-activation")
            .ok_or(ServiceError::IntegrityFailure)?,
    );
    let next = activation_verify::schedule::transition(
        key,
        customer,
        agreement,
        new_head,
        new_transition,
        copy(&accepted.binding),
    )?;
    activation_verify::schedule::check_next(&history, &next)
        .map_err(|_| reject("ACTIVATION_SCHEDULE_CONFLICT"))?;
    let root = journal::fresh_root(&setup, &at)?;
    let observation = if purpose == "admit" {
        let grant = source.grant().ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
        json!({"schema":"ledger-workflow-agreement-authority/2","purpose":"admit","operation":req.kind(),"actor":setup.operator,"source":setup.source,"scope":key.scope,"customer":customer,"agreement_id":agreement,"local_admin_ref":root.reference,"canonical_economic_namespace":grant.ceiling().canonical_economic_namespace(),"source_grant_refs":[source.state().current_grant_ref()],"assent_refs":binding["assent_refs"],"source_revision":source.state().revision().to_string(),"source_state_refs":[source.state().reference()],"observed_at":at})
    } else {
        json!({"schema":"ledger-workflow-agreement-authority/2","purpose":"stop","operation":req.kind(),"actor":setup.operator,"source":setup.source,"scope":key.scope,"customer":customer,"agreement_id":agreement,"local_admin_ref":root.reference,"accepted_agreement_ref":accepted.accepted.reference,"binding_ref":accepted.binding.reference,"target_transition_ref":payload["transition_ref"],"expected_activation_revision":payload["expected_revision"],"binding_source_namespace":source.enrollment().ceiling().canonical_economic_namespace(),"source_observation":activation_verify::source_observation(&source,&at)?,"evidence_refs":payload["evidence_refs"],"observed_at":at})
    };
    let authority = records::source_document("workflow-authority", observation)?;
    documents.extend(
        accepted
            .documents
            .iter()
            .filter(|d| {
                !matches!(
                    d.reference.kind.as_str(),
                    "workflow-agreement-intent" | "workflow-head-transition"
                )
            })
            .map(copy),
    );
    documents.extend(source.documents().iter().map(copy));
    if let Some(prior) = history.last() {
        documents
            .extend(verify::authenticated_closure(tx, key, &[prior.head.reference.clone()]).await?);
    }
    if let Some(target) = payload.get(target_field) {
        let target: RecordRef =
            serde_json::from_value(target.clone()).map_err(|_| ServiceError::IntegrityFailure)?;
        let transition = history
            .iter()
            .find(|t| t.activation.reference == target)
            .ok_or(ServiceError::IntegrityFailure)?;
        documents.push(copy(&transition.activation));
        documents.extend(
            verify::authenticated_closure(tx, key, &[transition.head.reference.clone()]).await?,
        );
    }
    for r in payload["evidence_refs"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let r: RecordRef =
            serde_json::from_value(r.clone()).map_err(|_| ServiceError::IntegrityFailure)?;
        let d = loaded(tx, key, &r).await?;
        let body = canonical_body(&d)?;
        let root_ref: RecordRef = serde_json::from_value(body["authority_ref"].clone())
            .map_err(|_| ServiceError::IntegrityFailure)?;
        documents.push(loaded(tx, key, &root_ref).await?);
        documents.push(d);
    }
    documents.extend(built.documents);
    let evidence = descriptor::evidence(req, &payload, &accepted.acceptance.descriptors)?;
    journal::assemble_agreement(
        req,
        evidence,
        journal::AgreementCommit {
            setup: &setup,
            at: &at,
            sequence,
            root,
            authority,
            documents,
            results: built.results,
            semantic_key,
            alias_of: None,
            projection: built.projection,
        },
    )
}
