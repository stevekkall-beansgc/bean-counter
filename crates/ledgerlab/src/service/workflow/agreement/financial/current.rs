//! Actual original-binding/current-source observations under the held writer.
//! This is NOT a completed financial prefix, eligibility or prepared commit.
use super::*;
use ledgerlab_core::policy::workflow::SourceGrant;
use ledgerlab_core::{
    canonical::audit_arena::{AuditedValue, ExternalBufferLease},
    policy::workflow::transition_constructor::ExactRef,
    validation_work::{ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind},
};
use std::collections::BTreeMap;

mod chain;
pub(super) mod party;
pub(super) use chain::ObservedChain;

pub(super) struct CurrentSelection {
    pub(super) setup: crate::service::billing::Setup,
    pub(super) accepted: context::ObservedAcceptedBinding,
    pub(super) activation: RecordRef,
    pub(super) sources: Vec<(RecordRef, context::ObservedSource)>,
    pub(super) at: Timestamp,
    pub(super) before_sequence: i64,
    pub(super) chain: Option<ObservedChain>,
}
fn reference(value: &Value, kind: &str) -> Result<RecordRef> {
    ledgerlab_core::policy::workflow::AgreementRef::parse(&records::bytes(value)?)
        .map_err(|_| reject("INVALID_REFERENCE"))?;
    let r: RecordRef =
        serde_json::from_value(value.clone()).map_err(|_| reject("INVALID_REFERENCE"))?;
    require(r.kind == kind, "INVALID_REFERENCE")?;
    Ok(r)
}
/// The literal work binding is a locator only. Its scope/ownership, original
/// acceptance, whole activation schedule and nominated source origins are all
/// acquired from the actual held store. The core must still check rights,
/// subject/time/selector intersection and every financial obligation.
pub(super) async fn work(
    ledger: &LocalWorkflowLedger,
    tx: &mut SqliteTx,
    key: &OperationKey,
    payload: &Value,
    session: &mut OriginalAuditSession,
) -> Result<CurrentSelection> {
    let setup = ledger.admin.fresh(&ledger.store, tx, key).await?;
    let installation = tx
        .load_installation()
        .await
        .map_err(crate::service::store_error)?;
    require(installation.mode == "real", "INSTALLATION_MODE_MISMATCH")?;
    let before_sequence = tx
        .workflow_sequence()
        .await
        .map_err(crate::service::store_error)?;
    let at = crate::local::now().map_err(|_| ServiceError::Unavailable)?;
    let binding_ref = reference(&payload["binding_ref"], "workflow-binding")?;
    let accepted =
        context::observe_accepted_binding(tx, key, &binding_ref, before_sequence).await?;
    let binding = canonical_body(&accepted.binding)?;
    let customer = text(&binding, "customer")?;
    let agreement = text(&binding, "agreement_id")?;
    require(
        payload["customer"] == customer,
        "AGREEMENT_OWNERSHIP_CONFLICT",
    )?;
    let history =
        activation_verify::schedule::history_before(tx, key, customer, agreement, before_sequence)
            .await?;
    let selected = activation_verify::schedule::selected_binding(&history, &at)?
        .ok_or_else(|| reject("AUTHORITY_REQUIRED"))?;
    require(selected == binding_ref, "AUTHORITY_REQUIRED")?;
    let activation = activation_verify::schedule::boundary_at(&history, &at)?
        .ok_or(ServiceError::IntegrityFailure)?
        .activation
        .reference
        .clone();
    // Original grant nominations come from the accepted binding and exact
    // slot terms, never from a new payload field or current maximum revision.
    let mut grants = BTreeMap::new();
    let mut add = |values: &Value| -> Result<()> {
        for value in values.as_array().ok_or(ServiceError::IntegrityFailure)? {
            let r = reference(value, "workflow-source-grant")?;
            if let Some(old) = grants.insert(r.id.clone(), r.clone()) {
                require(old == r, "INVALID_REFERENCE")?;
            }
        }
        Ok(())
    };
    add(&binding["authority_refs"])?;
    for entry in binding["bindings"]
        .as_array()
        .ok_or(ServiceError::IntegrityFailure)?
    {
        let terms_ref = reference(&entry["binding_ref"], "workflow-slot-terms")?;
        let terms = accepted
            .documents
            .iter()
            .find(|d| d.reference == terms_ref)
            .ok_or(ServiceError::IntegrityFailure)?;
        add(&canonical_body(terms)?["source_grant_refs"])?;
    }
    let mut enrollments = BTreeMap::new();
    for grant_ref in grants.into_values() {
        verify::document_origin(tx, key, &grant_ref).await?;
        let grant = loaded(tx, key, &grant_ref).await?;
        let typed = SourceGrant::parse(&grant.bytes).map_err(typed_error)?;
        let r = reference(
            &json!(typed.original_ceiling_ref()),
            "workflow-source-enrollment",
        )?;
        if let Some(old) = enrollments.insert(r.id.clone(), r.clone()) {
            require(old == r, "INVALID_REFERENCE")?;
        }
    }
    let mut sources = Vec::new();
    for r in enrollments.into_values() {
        let source = context::observe_source(tx, key, &r).await?;
        sources.push((r, source));
    }
    // The chain projection is read under this SAME writer and original-audit
    // owner. It remains DATA until the chronological inventory authenticates
    // its exact immutable body and core validates the selected original joins.
    let chain = chain::observe(tx, key, customer, payload, before_sequence, session).await?;
    Ok(CurrentSelection {
        setup,
        accepted,
        activation,
        sources,
        at,
        before_sequence,
        chain,
    })
}
