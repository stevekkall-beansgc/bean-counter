//! One owning assembly of the actual complete interpreter's outputs. It is
//! private and noncommittable until the full future/resource boundary closes.
use super::super::super::selected_program::SelectedProgram;
use super::super::{capacity, data::*, shape, working_value::owning_reference, CoreMeter};
use crate::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{self as w, ArithmeticOperation, ValidationWork},
    Error, Result,
};
use std::mem;

pub(in crate::policy::workflow::transition_constructor) struct Assembly<'inv, 'raw> {
    pub construction: Construction<'inv, 'raw>,
    pub schemas: Vec<String>,
    pub result_refs: Vec<ExactRef>,
    pub authority_position: usize,
    pub source_observations: Vec<super::ObservationAssociation>,
    pub actual_members: capacity::ActualMembers,
    reservations: super::reservations::OwnedReservations,
    _metadata: ExternalBufferLease,
}
fn copy(m: &mut CoreMeter, s: &str) -> Result<String> {
    m.scan_bytes(s.len())?;
    m.write_bytes(s.len())?;
    Ok(s.to_owned())
}
fn copy_ref(m: &mut CoreMeter, r: &ExactRef) -> Result<ExactRef> {
    Ok(ExactRef {
        kind: copy(m, &r.kind)?,
        id: copy(m, &r.id)?,
        digest: copy(m, &r.digest)?,
    })
}
fn product(m: &mut CoreMeter, a: usize, b: usize) -> Result<usize> {
    ValidationWork::arithmetic(m, ArithmeticOperation::Multiply, &[&a.into(), &b.into()])?;
    a.checked_mul(b)
        .ok_or_else(|| shape::fail("candidate assembly metadata product"))
}
fn ref_bytes(m: &mut CoreMeter, r: &ExactRef) -> Result<usize> {
    let n = w::usize_add(m, r.kind.len(), r.id.len())?;
    w::usize_add(m, n, r.digest.len())
}
/// The program owner is not an audit-only substitute for immediate inputs.
/// Bind each actual compiled source and plan to the caller's selected I body
/// before counting the unique manifest union. These are never saved N rows.
fn program_inputs(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &SelectedProgram,
    records: &mut super::super::records::Records,
) -> Result<()> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning compiled input membership"))?;
    for document in program
        .source_documents()
        .iter()
        .chain(std::iter::once(program.compiled_plan()))
    {
        m.visit("candidate.compiled_immediate_input")?;
        let r = document.reference();
        let n = w::usize_add(m, r.kind().len(), r.id().len())?;
        let n = w::usize_add(m, n, r.digest().len())?;
        let n = w::usize_add(m, n, mem::size_of::<ExactRef>())?;
        let _owner = arena.reserve_external_payload(n, m)?;
        let reference = ExactRef {
            kind: copy(m, r.kind())?,
            id: copy(m, r.id())?,
            digest: copy(m, r.digest())?,
        };
        let schema = shape::text(m, document.canonical_value(), "schema")?;
        records.get(m, &c.key, &reference, r.kind(), schema)?;
        let selected = records.canonical(m, &reference)?;
        if !shape::same_bytes(m, selected, document.canonical_bytes())? {
            return Err(shape::fail(
                "selected program input differs from exact compiled body",
            ));
        }
    }
    Ok(())
}
/// Recipe selected by the actual normalized payload. No action-count inference
/// and no supplied receipt/result list participates in this dispatch.
pub(in crate::policy::workflow::transition_constructor) fn operation(
    m: &mut CoreMeter,
    p: &serde_json::Value,
) -> Result<&'static str> {
    let schema = shape::text(m, p, "schema")?;
    for (s, kind) in [
        ("ledger-workflow-work/1", "work.accept"),
        ("ledger-workflow-outcome/1", "outcome.submit"),
        ("ledger-workflow-outcome-correction/1", "outcome.correct"),
        ("ledger-workflow-outcome-close/2", "outcome.close"),
        ("ledger-workflow-quantity-correction/2", "quantity.correct"),
        ("ledger-workflow-work-reverse/2", "work.reverse"),
        ("ledger-workflow-cost-finalize/2", "cost.finalize"),
        ("ledger-workflow-cost-correct/2", "cost.correct"),
        ("ledger-workflow-fact-attest/1", "evidence.retain"),
        ("ledger-workflow-family-close-attest/1", "evidence.retain"),
        ("ledger-workflow-app-observe/1", "evidence.retain"),
        ("ledger-workflow-app-cost-attest/1", "evidence.retain"),
    ] {
        m.visit("candidate.executed_payload_recipe")?;
        if shape::same_text(m, schema, s)? {
            return Ok(kind);
        }
    }
    Err(Error::new(
        "UNSUPPORTED_SCHEMA_PROFILE",
        "candidate payload recipe is not implemented",
    ))
}

/// Root reversal emits independent dependent authorities for the SAME operation.
/// Its own newly constructed Work decision names the authoritative root Ref;
/// emitted order and operation-name equality alone cannot select that identity.
fn local_reversal_authority(
    m: &mut CoreMeter,
    c: &DraftContextData,
    payload: &serde_json::Value,
    outputs: &[super::super::emitted_record::ConstructedEnvelope],
) -> Result<usize> {
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning root authority association"))?;
    let key = super::super::cost_postings::operation_key(m, &arena, c)?;
    let work = super::super::working_value::parse_owned_reference(m, &arena, &payload["work_ref"])?;
    let work = owning_reference(m, &arena, work.reference())?;
    let mut selected = None;
    for output in outputs {
        m.visit("candidate.root_reversal_authority_association")?;
        if !shape::same_text(m, &output.reference.kind, "workflow-decision")? {
            continue;
        }
        let value = output.decoded(m, 4194304)?;
        let schema = shape::text(m, &value, "schema")?;
        if !shape::same_text(m, schema, "ledger-workflow-decision-revision/1")? {
            continue;
        }
        let kind = shape::text(m, &value, "kind")?;
        if !shape::same_text(m, kind, "work")? {
            continue;
        }
        if !shape::same(m, &value["subject_ref"], work.value())? {
            continue;
        }
        let transition = shape::text(m, &value, "transition")?;
        if !shape::same_text(m, transition, "work_reverse")? {
            return Err(shape::fail("root reversal decision transition"));
        }
        shape::equal(m, &value["operation_key"], key.value())?;
        if selected.is_some() {
            return Err(shape::fail(
                "ambiguous root reversal decision authority association",
            ));
        }
        selected = Some(super::super::working_value::parse_owned_reference(
            m,
            &arena,
            &value["authority_ref"],
        )?);
    }
    let selected = selected
        .ok_or_else(|| shape::fail("root reversal decision authority association missing"))?;
    let mut position = None;
    for (i, output) in outputs.iter().enumerate() {
        m.visit("candidate.root_reversal_authority_result")?;
        if !shape::same_ref(m, &output.reference, selected.reference())? {
            continue;
        }
        if !shape::same_text(m, &output.reference.kind, "workflow-economic-authority")? {
            return Err(shape::fail("root reversal authority kind"));
        }
        let value = output.decoded(m, 4194304)?;
        let schema = shape::text(m, &value, "schema")?;
        let operation = shape::text(m, &value, "operation")?;
        if !shape::same_text(m, schema, "ledger-workflow-economic-authority/8")?
            || !shape::same_text(m, operation, "work.reverse")?
        {
            return Err(shape::fail("root reversal authority recipe"));
        }
        if position.is_some() {
            return Err(shape::fail("ambiguous root operation authority result"));
        }
        position = Some(i);
    }
    position.ok_or_else(|| shape::fail("actual root authority result missing"))
}
impl<'inv, 'raw> Assembly<'inv, 'raw> {
    pub(in crate::policy::workflow::transition_constructor) fn derive(
        m: &mut CoreMeter,
        c: &DraftContextData,
        program: &SelectedProgram,
        state: &EconomicStateData,
        mut t: Construction<'inv, 'raw>,
    ) -> Result<Self> {
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning candidate assembly"))?;
        let payload = m.parse_owned_representation(
            &arena,
            &t.normalized_payload,
            crate::validation_work::RepresentationKind::OriginalRequest,
        )?;
        let kind = operation(m, payload.value())?;
        let local = program.uses_local_execution_recipe();
        if local && !shape::same_text(m, kind, "evidence.retain")? {
            capacity::local::produce(m, c, program, state, &mut t)?;
        }
        if !local
            && m.check("capacity.retained_original_derivation_needed", || {
                t.original_reservations.is_none()
            })?
            && !shape::same_text(m, kind, "evidence.retain")?
        {
            if let OriginalWorkPayload::Retained(work) = &t.economic.original_work {
                // Independent same-child resolution, not a saved maximum or a
                // parent inventory/cache transfer. The immutable Arc clone only
                // permits disjoint borrowing while the one Records table is used.
                if !m.check("capacity.retained_work_has_owning_body", || {
                    matches!(work, super::super::record_body::RecordBody::Owned(_))
                })? {
                    return Err(shape::fail("retained capacity requires owning work body"));
                }
                let _view = arena.reserve_external_payload(
                    mem::size_of::<super::super::record_body::RecordBody>(),
                    m,
                )?;
                m.write_bytes(mem::size_of::<super::super::record_body::RecordBody>())?;
                let work = work.clone();
                t.original_reservations = Some(capacity::original::derive_for_work(
                    &arena,
                    m,
                    &mut t.records,
                    c,
                    program,
                    &work,
                    &t.economic.terms,
                )?);
            }
        }
        if !local
            && m.check("capacity.retained_ancestor_head_needed", || {
                t.current_capacity.is_none()
                    && matches!(&t.economic.original_work, OriginalWorkPayload::Retained(_))
            })?
            && !shape::same_text(m, kind, "evidence.retain")?
        {
            let original = t
                .original_reservations
                .as_ref()
                .ok_or_else(|| shape::fail("retained ancestor original reservations missing"))?;
            t.current_capacity = Some(capacity::current::retained_original_head(
                m,
                &mut t.records,
                c,
                &original.original_root_ref,
                &original.root_decision_id,
                state,
            )?);
        }
        if let (Some(original), Some(current)) = (&t.original_reservations, &t.current_capacity) {
            // This only closes original decision/action/edge equality on a graph
            // with no retirements. Numerical record/byte/work, changed ancestor
            // graphs and all resource proofs remain the final refusing boundary.
            let _complete_original_structure = capacity::current::compare_original_structure(
                m,
                &mut t.records,
                c,
                original,
                current,
            )?;
        }
        let mut bytes = product(
            m,
            t.new_envelopes.len(),
            mem::size_of::<String>()
                + mem::size_of::<ExactRef>()
                + mem::size_of::<super::CandidateEnvelope>(),
        )?;
        bytes = w::usize_add(m, bytes, mem::size_of::<Self>())?;
        if let Some(reference) = &t.reused_result {
            let payload = ref_bytes(m, reference)?;
            bytes = w::usize_add(m, bytes, payload)?;
            bytes = w::usize_add(m, bytes, mem::size_of::<ExactRef>())?;
        }
        for row in &t.new_envelopes {
            m.visit("candidate.assembly_output_dimension")?;
            let value = row.decoded(m, 4194304)?;
            let schema = shape::text(m, &value, "schema")?;
            bytes = w::usize_add(m, bytes, schema.len())?;
            let reference_bytes = ref_bytes(m, &row.reference)?;
            bytes = w::usize_add(m, bytes, reference_bytes)?;
        }
        for value in [
            &c.key.logical_store_id,
            &c.key.scope[0],
            &c.key.scope[1],
            &c.key.source,
            &c.key.operation_id,
        ] {
            bytes = w::usize_add(m, bytes, value.len())?;
        }
        bytes = w::usize_add(m, bytes, 4 * (64 + 128 + 71) + 71)?;
        let ordinary = t
            .candidate_introductions
            .as_ref()
            .map(|x| x.claims.len())
            .unwrap_or(0);
        let app = t.app_effects.as_ref().map(|x| x.claims.len()).unwrap_or(0);
        let claim_count = w::usize_add(m, ordinary, app)?;
        let claims_bytes = product(m, claim_count, mem::size_of::<super::ClaimEffect>())?;
        bytes = w::usize_add(m, bytes, claims_bytes)?;
        let associations = product(
            m,
            c.authorities.len(),
            mem::size_of::<super::ObservationAssociation>() + 2 * (64 + 128 + 71),
        )?;
        bytes = w::usize_add(m, bytes, associations)?;
        let metadata = arena.reserve_external_payload(bytes, m)?;
        // Includes all native controls before any Vec/String/ExactRef allocation;
        // payload copy helpers retain their actual subsequent scan/write charges.
        m.write_bytes(bytes)?;
        let mut schemas = Vec::new();
        schemas
            .try_reserve_exact(t.new_envelopes.len())
            .map_err(|_| shape::fail("candidate schema vector"))?;
        let result_count = w::usize_add(
            m,
            t.new_envelopes.len(),
            usize::from(t.reused_result.is_some()),
        )?;
        let mut result_refs = Vec::new();
        result_refs
            .try_reserve_exact(result_count)
            .map_err(|_| shape::fail("candidate result reference vector"))?;
        let root_authority = if local && shape::same_text(m, kind, "work.reverse")? {
            Some(local_reversal_authority(
                m,
                c,
                payload.value(),
                &t.new_envelopes,
            )?)
        } else {
            None
        };
        let mut authority = None;
        for (i, row) in t.new_envelopes.iter().enumerate() {
            m.visit("candidate.assembly_authoritative_result")?;
            let value = row.decoded(m, 4194304)?;
            let schema = shape::text(m, &value, "schema")?;
            if shape::same_text(m, &row.reference.kind, "workflow-economic-authority")?
                && shape::same_text(
                    m,
                    schema,
                    if local {
                        "ledger-workflow-economic-authority/8"
                    } else {
                        "ledger-workflow-economic-authority/7"
                    },
                )?
            {
                let actual = shape::text(m, &value, "operation")?;
                if shape::same_text(m, actual, kind)? {
                    let root = match &root_authority {
                        Some(expected) => w::compare(m, i, *expected)?.is_eq(),
                        None => true,
                    };
                    if root {
                        if authority.is_some() {
                            return Err(shape::fail("ambiguous root operation authority result"));
                        }
                        authority = Some(i);
                    }
                }
            }
            schemas.push(copy(m, schema)?);
            result_refs.push(copy_ref(m, &row.reference)?);
        }
        if let Some(reference) = &t.reused_result {
            m.visit("candidate.reused_result_partition")?;
            if !matches!(
                t.records.used_canonical(m, reference)?,
                super::super::records::UsedCanonical::Preexisting(_)
            ) {
                return Err(shape::fail("reused result must be consumed old I"));
            }
            for row in &t.new_envelopes {
                m.visit("candidate.reused_result_not_new")?;
                if shape::same_ref(m, reference, &row.reference)? {
                    return Err(shape::fail("reused result also introduced"));
                }
            }
            result_refs.push(copy_ref(m, reference)?);
        }
        let authority_position =
            authority.ok_or_else(|| shape::fail("actual root authority result missing"))?;
        let mut source_observations = Vec::new();
        source_observations
            .try_reserve_exact(c.authorities.len())
            .map_err(|_| shape::fail("candidate source association vector"))?;
        for source in c.authorities.iter().filter(|_| !local) {
            m.visit("candidate.assembly_source_observation")?;
            for prior in &source_observations {
                let prior: &super::ObservationAssociation = prior;
                if shape::same_ref(
                    m,
                    &prior.observation_ref,
                    source.required_observation_ref()?,
                )? {
                    return Err(shape::fail("duplicate candidate source observation"));
                }
            }
            let observation = t.records.get(
                m,
                &c.key,
                source.required_observation_ref()?,
                "workflow-authority-prefix-observation",
                "ledger-workflow-authority-prefix-observation/1",
            )?;
            let verifier = super::super::working_value::parse_owned_reference(
                m,
                &arena,
                &observation["verifier_ref"],
            )?;
            t.records.get(
                m,
                &c.key,
                verifier.reference(),
                "workflow-authority-audit-verifier",
                "ledger-authority-audit-verifier/2",
            )?;
            source_observations.push(super::ObservationAssociation {
                observation_ref: copy_ref(m, source.required_observation_ref()?)?,
                verifier_ref: copy_ref(m, verifier.reference())?,
            });
        }
        let heads = t
            .candidate_heads
            .as_ref()
            .ok_or_else(|| shape::fail("actual candidate head effects missing"))?;
        for head in &heads.effects {
            m.visit("candidate.assembly_head_position")?;
            if !w::compare(m, head.introduced_position, t.new_envelopes.len())?.is_lt() {
                return Err(shape::fail("candidate head output position"));
            }
            if !shape::same_ref(
                m,
                &head.proposed,
                &t.new_envelopes[head.introduced_position].reference,
            )? {
                return Err(shape::fail("candidate head output exact association"));
            }
        }
        t.candidate_introductions
            .as_ref()
            .ok_or_else(|| shape::fail("actual candidate introductions missing"))?;
        if !local {
            let financial = t
                .financial_observation
                .as_ref()
                .ok_or_else(|| shape::fail("actual financial observation missing"))?;
            let observed = owning_reference(m, &arena, &financial.observation_ref)?;
            // The selected pair has already been checked against the complete held
            // preoperation projections by execute_complete; keep the owner attached.
            let verifier = owning_reference(m, &arena, &financial.verification_ref)?;
            shape::equal(m, &financial.observation["verifier_ref"], verifier.value())?;
            let _ = observed;
        }
        let reservations = super::reservations::derive(m, c, state, &mut t, local)?;
        program_inputs(m, c, program, &mut t.records)?;
        if !local {
            super::super::financial_observation::containment::check(m, c, &mut t)?;
        }
        let actual_members = capacity::actual_members(m, &t)?;
        Ok(Self {
            construction: t,
            schemas,
            result_refs,
            authority_position,
            source_observations,
            actual_members,
            reservations,
            _metadata: metadata,
        })
    }
    /// This is the explicit unfinished eligibility boundary, AFTER the actual
    /// interpreter, head/claim/link/funding/observation assembly and I∪N count.
    /// No profile maximum, caller flag or DraftTransition can satisfy it.
    pub(in crate::policy::workflow::transition_constructor) fn require_complete_capacity_and_resources(
        &self,
        m: &mut CoreMeter,
    ) -> Result<()> {
        m.visit("candidate.complete_future_capacity_boundary")?;
        if let Some(original) = &self.construction.original_reservations {
            let arena = m
                .representation_arena()
                .ok_or_else(|| shape::fail("candidate capacity owner"))?;
            let _union = original.structural_union(&arena, m)?;
            if original.future_records.is_none() {
                return Err(Error::new(
                    "UNSUPPORTED_SCHEMA_PROFILE",
                    "future executor record derivation is incomplete",
                ));
            }
            // Actual checked composition, not stored maxima or observed runtime
            // totals. Missing containing phases below still prevent any writer.
            let (_accounted_source, _accounted_pricing) =
                capacity::accounted_root_callbacks(m, original)?;
            let _monetary_new_body_bytes = capacity::accounted_root_new_bodies(m, original)?;
            let (_changing_source_i_bytes, _source_observation_n_bytes) =
                capacity::accounted_root_source_bodies(m, original)?;
            // Authority.app_cost_revision_refs independently reaches every older
            // active cost revision. Prior-field cuts do not make that I constant.
            // This history-sized subtotal is not yet the complete P/F/E union.
            let _app_history = capacity::history::derive(m, original)?;
        }
        Err(Error::new("UNSUPPORTED_SCHEMA_PROFILE","complete all-ancestor dependency/capacity and original S/C/bytes/arena proof is not implemented"))
    }
}

// Existing component leases move with their actual payloads. None can be
// detached by a public setter, serialization or a DraftTransition conversion.
pub(in crate::policy::workflow::transition_constructor) struct CandidateOwnership {
    heads: super::heads::OwnedHeads,
    introductions: super::introductions::OwnedIntroductions,
    funding: Option<super::super::funding::OwnedFundingEffects>,
    consumed: super::super::records::OwnedConsumed,
    controls: [Option<ExternalBufferLease>; 5],
    metadata: ExternalBufferLease,
    reservations: super::reservations::OwnedReservations,
    head_transfer: ExternalBufferLease,
    _financial_reservations: Option<capacity::original::FinancialReservations>,
}
impl Assembly<'_, '_> {
    pub(in crate::policy::workflow::transition_constructor) fn into_complete(
        self,
        c: &DraftContextData,
        digest: &str,
        m: &mut CoreMeter,
    ) -> Result<super::EconomicCandidate> {
        let financial = self
            .construction
            .financial_observation
            .as_ref()
            .ok_or_else(|| shape::fail("candidate financial owner"))?;
        let financial_observation = super::ObservationAssociation {
            observation_ref: copy_ref(m, &financial.observation_ref)?,
            verifier_ref: copy_ref(m, &financial.verification_ref)?,
        };
        let sources = self
            .source_observations
            .iter()
            .map(|s| {
                Ok(super::ObservationAssociation {
                    observation_ref: copy_ref(m, &s.observation_ref)?,
                    verifier_ref: copy_ref(m, &s.verifier_ref)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let execution = super::CandidateExecutionMetadata::ExperimentalAudit9(
            super::ExperimentalAuditMetadata {
                source_observations: sources,
                financial_observation,
                combined_audit_digest: copy(m, digest)?,
            },
        );
        self.into_candidate(c, execution, m)
    }
    pub(in crate::policy::workflow::transition_constructor) fn into_local(
        self,
        c: &DraftContextData,
        m: &mut CoreMeter,
    ) -> Result<super::EconomicCandidate> {
        if self.construction.financial_observation.is_some() || !self.source_observations.is_empty()
        {
            return Err(shape::fail(
                "local candidate cannot carry experimental observations",
            ));
        }
        self.into_candidate(c, super::CandidateExecutionMetadata::LocalSelected1, m)
    }
    fn into_candidate(
        self,
        c: &DraftContextData,
        execution: super::CandidateExecutionMetadata,
        m: &mut CoreMeter,
    ) -> Result<super::EconomicCandidate> {
        let Self {
            mut construction,
            schemas,
            result_refs,
            authority_position,
            source_observations: _,
            actual_members: _,
            mut reservations,
            _metadata,
        } = self;
        let authority_ref = copy_ref(m, &construction.new_envelopes[authority_position].reference)?;
        let mut introduced = Vec::new();
        introduced
            .try_reserve_exact(construction.new_envelopes.len())
            .map_err(|_| shape::fail("candidate owning envelope vector"))?;
        for (row, schema) in construction.new_envelopes.drain(..).zip(schemas) {
            m.visit("candidate.move_owning_envelope")?;
            introduced.push(super::CandidateEnvelope {
                reference: row.reference,
                schema,
                body: row.canonical_body,
            });
        }
        let mut heads = construction
            .candidate_heads
            .take()
            .ok_or_else(|| shape::fail("candidate held head owner"))?;
        let mut introductions = construction
            .candidate_introductions
            .take()
            .ok_or_else(|| shape::fail("candidate introduction owner"))?;
        let app_claims = construction.app_effects.as_mut().map(|x| &mut x.claims);
        let total = w::usize_add(
            m,
            introductions.claims.len(),
            app_claims.as_ref().map(|x| x.len()).unwrap_or(0),
        )?;
        let mut claims = Vec::new();
        claims
            .try_reserve_exact(total)
            .map_err(|_| shape::fail("candidate claim ownership vector"))?;
        claims.append(&mut introductions.claims);
        if let Some(app) = app_claims {
            claims.append(app);
        }
        let links = std::mem::take(&mut introductions.links);
        let mut head_effects = std::mem::take(&mut heads.effects);
        let extra = reservations.heads.len();
        let total_heads = w::usize_add(m, head_effects.len(), extra)?;
        let head_bytes = product(m, total_heads, mem::size_of::<super::HeadEffect>())?;
        let arena = m
            .representation_arena()
            .ok_or_else(|| shape::fail("owning capacity head transfer"))?;
        let head_transfer = arena.reserve_external_payload(head_bytes, m)?;
        head_effects
            .try_reserve_exact(extra)
            .map_err(|_| shape::fail("capacity head transfer vector"))?;
        head_effects.append(&mut reservations.heads);
        let dependencies = std::mem::take(&mut reservations.dependencies);
        let mut funding = construction.funding_effects.take();
        let funding_effects = match &mut funding {
            Some(v) => std::mem::take(&mut v.effects),
            None => Vec::new(),
        };
        let mut consumed = construction.records.into_consumed_owned();
        let consumed_refs = std::mem::take(&mut consumed.references);
        let key = OperationKey {
            logical_store_id: copy(m, &c.key.logical_store_id)?,
            scope: [copy(m, &c.key.scope[0])?, copy(m, &c.key.scope[1])?],
            source: copy(m, &c.key.source)?,
            operation_id: copy(m, &c.key.operation_id)?,
        };
        let result = super::CandidateResultMetadata {
            kind: super::CandidateResultKind::Economic,
            result_refs,
            authority_ref,
            execution,
        };
        Ok(super::EconomicCandidate {
            key,
            normalized_payload: construction.normalized_payload,
            introduced,
            consumed_refs,
            heads: head_effects,
            claims,
            links,
            dependencies,
            funding: funding_effects,
            account_enrollments: Vec::new(),
            result,
            _ownership: CandidateOwnership {
                heads,
                introductions,
                funding,
                consumed,
                controls: construction.control_owners,
                metadata: _metadata,
                reservations,
                head_transfer,
                _financial_reservations: construction.financial_reservations.take(),
            },
        })
    }
}

#[cfg(test)]
#[path = "assembly_authority_tests.rs"]
mod assembly_authority_tests;
