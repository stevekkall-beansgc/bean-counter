//! Actual successor verification N and its exact pending/historical operand
//! partition. Pure output is never substituted for current held party state.
use super::*;
use ledgerlab_core::policy::workflow::transition_constructor::{
    self as core, EconomicRecordView, ExactRef, LocalSupplierConsentVerifications,
    PartyAuthorizationHead, SupplierConsentInputs,
};

pub(super) struct SupplierCoverage {
    records: Vec<wire::SourceRecord>,
    _derived: LocalSupplierConsentVerifications,
    retained: Vec<ExactRef>,
    _pool: super::super::local_inputs::LocalInputs,
    _heads: Vec<PartyAuthorizationHead>,
    _copies: Vec<ExternalBufferLease>,
    _retained_copies: Vec<ExternalBufferLease>,
    _output_control: ExternalBufferLease,
    _control: ExternalBufferLease,
}
impl SupplierCoverage {
    pub(super) fn records(&self) -> impl Iterator<Item = &wire::SourceRecord> {
        self.records.iter()
    }
    pub(super) fn selected_refs(&self) -> impl Iterator<Item = &ExactRef> {
        self.retained.iter()
    }
}
fn copy_text(value: &str, s: &mut OriginalAuditSession) -> Result<String> {
    s.semantic_work()
        .scan_bytes(value.len())
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(value.len())
        .map_err(core_error)?;
    Ok(value.to_owned())
}
fn retain_head(
    heads: &mut Vec<PartyAuthorizationHead>,
    owners: &mut Vec<ExternalBufferLease>,
    body: &Value,
    reference: &ExactRef,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    let spec = request_wrapper::field(body, "authorization", s)?;
    let id = request_wrapper::text(spec, "authorization_id", s)?;
    request_wrapper::same_text(
        request_wrapper::text(body, "schema", s)?,
        "ledger-workflow-party-authorization/1",
        s,
    )?;
    request_wrapper::same_text(request_wrapper::text(body, "status", s)?, "active", s)?;
    let revision = request_wrapper::text(body, "revision", s)?;
    request_wrapper::same_text(revision, "1", s)?;
    for head in heads.iter() {
        if same_recipe_text(&head.authorization_id, id, s)? {
            require(
                head.reference == *reference && head.revision == revision,
                "INTEGRITY_FAILURE",
            )?;
            return Ok(());
        }
    }
    let bytes = id.len()
        + revision.len()
        + reference.kind.len()
        + reference.id.len()
        + reference.digest.len();
    let owner = s.reserve_external_payload(bytes).map_err(core_error)?;
    let row = PartyAuthorizationHead {
        authorization_id: copy_text(id, s)?,
        revision: copy_text(revision, s)?,
        reference: ExactRef {
            kind: copy_text(&reference.kind, s)?,
            id: copy_text(&reference.id, s)?,
            digest: copy_text(&reference.digest, s)?,
        },
    };
    s.semantic_work()
        .write_bytes(
            mem::size_of::<PartyAuthorizationHead>() + mem::size_of::<ExternalBufferLease>(),
        )
        .map_err(core_error)?;
    heads.push(row);
    owners.push(owner);
    Ok(())
}

pub(super) async fn derive(
    tx: &mut SqliteTx,
    key: &OperationKey,
    selected: (
        &AppAcceptanceInput,
        &prefix::RecompiledPublication,
        &private::InlineDocuments,
    ),
    pending: (&wire::SourceRecord, &wire::SourceRecord, &consent::Coverage),
    source_inputs: (
        &[current::source::ObservedSource],
        &current::source::ObservedSource,
    ),
    capture: (&str, &Timestamp, i64, bool),
    s: &mut OriginalAuditSession,
) -> Result<Option<SupplierCoverage>> {
    let (input, publication, documents) = selected;
    let (binding, root, consents) = pending;
    let (sources, admission_source) = source_inputs;
    let (actor, at, before, historical) = capture;
    if !publication.program().uses_stage_lifecycle_recipe() {
        return Ok(None);
    }
    let mut pool = super::super::local_inputs::LocalInputs::new(s)?;
    pool.select_stage_recipe(publication.program());
    for reference in publication
        .required_refs()
        .chain(
            sources
                .iter()
                .flat_map(current::source::ObservedSource::record_refs),
        )
        .chain(consents.selected_refs())
        .chain(documents.retained_refs())
    {
        pool.add(reference, s)?;
    }
    let terms = input.terms_record(s)?;
    let count = documents.inline_records().count() + consents.generated_records().count() + 3;
    let control = s
        .reserve_external_payload(
            count
                * (mem::size_of::<EconomicRecordView<'_>>()
                    + mem::size_of::<PartyAuthorizationHead>()
                    + mem::size_of::<ExternalBufferLease>()
                    + mem::size_of::<wire::SourceRecord>()
                    + mem::size_of::<ExactRef>()),
        )
        .map_err(core_error)?;
    let mut pending = Vec::new();
    pending
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    for record in documents
        .inline_records()
        .chain(consents.generated_records())
        .chain([root, binding, &terms])
    {
        s.semantic_work()
            .write_bytes(mem::size_of::<EconomicRecordView<'_>>())
            .map_err(core_error)?;
        pending.push(EconomicRecordView {
            reference: record.reference(),
            canonical_body: record.bytes(),
        });
    }
    pool.seed_pending(key, &pending, s)?;
    pool.complete(tx, key, before, s).await?;
    let mut heads = Vec::new();
    let mut copies = Vec::new();
    heads
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    copies
        .try_reserve_exact(count)
        .map_err(|_| ServiceError::Unavailable)?;
    let mut originals = if historical {
        Some(super::super::local_original_heads::OriginalHeads::before(tx, key, before, s).await?)
    } else {
        None
    };
    // Provider-only Assent3 and actual platform mappings independently name
    // their exact original authorization. No first-party or current-head guess.
    let (views, _views_owner) = pool.owned_views(0, s)?;
    for view in &views {
        if view.reference.kind != "workflow-assent" {
            continue;
        }
        let parsed = s
            .parse_representation(view.canonical_body, Kind::Record)
            .map_err(core_error)?;
        if request_wrapper::text(parsed.value(), "schema", s)? != "ledger-workflow-party-assent/3" {
            continue;
        }
        let spec = request_wrapper::field(parsed.value(), "consent", s)?;
        let original_ref = request_wrapper::field(spec, "authorization_ref", s)?;
        if let Some(originals) = originals.as_mut() {
            let (r, _owner) = current::source::from_value(original_ref, "workflow-authority", s)?;
            let body = s
                .parse_representation(pool.body(&r)?, Kind::Record)
                .map_err(core_error)?;
            let authorization = request_wrapper::field(body.value(), "authorization", s)?;
            let id = request_wrapper::field(authorization, "authorization_id", s)?;
            let actual = originals
                .input("local-original-party", &[id], &views, s)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let actual_body = s
                .parse_representation(actual.canonical_body, Kind::Record)
                .map_err(core_error)?;
            retain_head(
                &mut heads,
                &mut copies,
                actual_body.value(),
                actual.reference,
                s,
            )?;
        } else {
            let observed = current::party::observe(tx, key, original_ref, at, before, s).await?;
            retain_head(
                &mut heads,
                &mut copies,
                observed.body(),
                observed.reference(),
                s,
            )?;
        }
    }
    for mapping in consents.platform_mappings() {
        if let Some(originals) = originals.as_mut() {
            let spec = request_wrapper::field(mapping.body(), "authorization", s)?;
            let id = request_wrapper::field(spec, "authorization_id", s)?;
            let actual = originals
                .input("local-original-party", &[id], &views, s)?
                .ok_or(ServiceError::IntegrityFailure)?;
            let actual_body = s
                .parse_representation(actual.canonical_body, Kind::Record)
                .map_err(core_error)?;
            retain_head(
                &mut heads,
                &mut copies,
                actual_body.value(),
                actual.reference,
                s,
            )?;
        } else {
            retain_head(
                &mut heads,
                &mut copies,
                mapping.body(),
                mapping.reference(),
                s,
            )?;
        }
    }
    let body = s
        .parse_representation(binding.bytes(), Kind::Record)
        .map_err(core_error)?;
    // This is the actual selected source birth, authenticated by held owner
    // custody on admission and original producer/prefix custody on replay.
    let mode = request_wrapper::text(
        admission_source.enrollment_record().body(),
        "installation_mode",
        s,
    )?;
    let grant = admission_source
        .current_grant_body()
        .ok_or(ServiceError::IntegrityFailure)?;
    let namespace = request_wrapper::text(grant, "canonical_economic_namespace", s)?;
    let (context, _context_owner, _key_owner) = super::super::local_cost_authority::context(
        key,
        actor,
        namespace,
        mode,
        at.as_str(),
        at.as_str(),
        s,
    )?;
    let args = SupplierConsentInputs {
        publication_ref: publication.publication().0,
        acceptance_terms_ref: terms.reference(),
        registered_customer_ref: consents.registered_customer()?.reference(),
        local_admin_ref: root.reference(),
        binding: body.value(),
        inputs: &views,
        current_party_authorizations: &heads,
    };
    let derived = core::derive_local_supplier_consent_verifications(
        publication.program(),
        &context,
        &args,
        s,
    )
    .map_err(core_error)?;
    let output_count = derived.records().len();
    let retained_count = derived.referenced_inputs().len();
    let output_control = s
        .reserve_external_payload(
            output_count * mem::size_of::<wire::SourceRecord>()
                + retained_count
                    * (mem::size_of::<ExactRef>() + mem::size_of::<ExternalBufferLease>()),
        )
        .map_err(core_error)?;
    let mut retained_copies = Vec::new();
    retained_copies
        .try_reserve_exact(retained_count)
        .map_err(|_| ServiceError::Unavailable)?;
    let mut records = Vec::new();
    records
        .try_reserve_exact(output_count)
        .map_err(|_| ServiceError::Unavailable)?;
    for row in derived.records() {
        let bytes = s
            .copy_representation(row.canonical_body, Kind::SourceDefinition)
            .map_err(core_error)?;
        let record = wire::source_record(
            "workflow-verification",
            "ledger-workflow-supplier-consent-verification/2",
            bytes,
            s,
        )?;
        require(record.reference() == row.reference, "INTEGRITY_FAILURE")?;
        s.semantic_work()
            .write_bytes(mem::size_of::<wire::SourceRecord>())
            .map_err(core_error)?;
        records.push(record);
    }
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(derived.referenced_inputs().len())
        .map_err(|_| ServiceError::Unavailable)?;
    for reference in derived.referenced_inputs() {
        let mut introduced = false;
        for row in &pending {
            if row.reference == reference {
                introduced = true;
                break;
            }
        }
        if introduced {
            continue;
        }
        prefix::verify_selected_association(tx, key, reference, before, s).await?;
        let bytes = reference.kind.len() + reference.id.len() + reference.digest.len();
        retained_copies.push(s.reserve_external_payload(bytes).map_err(core_error)?);
        s.semantic_work()
            .write_bytes(mem::size_of::<ExactRef>() + mem::size_of::<ExternalBufferLease>())
            .map_err(core_error)?;
        retained.push(ExactRef {
            kind: copy_text(&reference.kind, s)?,
            id: copy_text(&reference.id, s)?,
            digest: copy_text(&reference.digest, s)?,
        });
    }
    drop(views);
    drop(pending);
    Ok(Some(SupplierCoverage {
        records,
        _derived: derived,
        retained,
        _pool: pool,
        _heads: heads,
        _copies: copies,
        _retained_copies: retained_copies,
        _output_control: output_control,
        _control: control,
    }))
}
