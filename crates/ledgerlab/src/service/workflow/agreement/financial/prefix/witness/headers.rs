//! Closed observation framing from the actual first-pass result. These owning
//! records remain DATA until current custody and the second exact union pass.
use super::super::super::wire;
use super::*;
use ledgerlab_core::{
    canonical::audit_arena::AuditedBytes,
    policy::workflow::transition_constructor::ProducedOriginPrefix,
};

const SOURCE: &[&str] = &[
    "audited_retained_bytes",
    "current_grant_ref",
    "current_state_ref",
    "document_count",
    "enrollment_ref",
    "head_revision",
    "installation_mode",
    "logical_store_id",
    "observed_at",
    "origin_count",
    "original_grant_refs",
    "prefix_digest",
    "root_ref",
    "schema",
    "scope",
    "verification_profile",
    "verified_through_sequence",
    "verifier_ref",
];
const FINANCIAL: &[&str] = &[
    "audited_retained_bytes",
    "document_count",
    "head_pins",
    "history_edges",
    "installation_mode",
    "logical_store_id",
    "observed_at",
    "origin_count",
    "pending_operation_key",
    "preexisting_input_refs",
    "prefix_digest",
    "root_ref",
    "schema",
    "scope",
    "snapshot_through_sequence",
    "verifier_ref",
];

fn frame(
    source: bool,
    seed: &Value,
    prefix: &ProducedOriginPrefix,
    accounting: &WitnessAccounting,
    s: &mut OriginalAuditSession,
) -> Result<wire::SourceRecord> {
    let names = if source { SOURCE } else { FINANCIAL };
    let (kind, schema) = if source {
        (
            "workflow-authority-prefix-observation",
            "ledger-workflow-authority-prefix-observation/1",
        )
    } else {
        (
            "workflow-financial-history-observation",
            "ledger-workflow-financial-history-observation/3",
        )
    };
    // The simultaneous parts and borrowed dictionary controls are bounded and
    // owned before any canonical producer runs. Each part has its own owner.
    let _control = s
        .reserve_external_payload(
            mem::size_of::<[Option<AuditedBytes>; 18]>() + mem::size_of::<[(&str, &[u8]); 18]>(),
        )
        .map_err(core_error)?;
    s.semantic_work()
        .write_bytes(mem::size_of::<[Option<AuditedBytes>; 18]>())
        .map_err(core_error)?;
    let mut parts: [Option<AuditedBytes>; 18] = [const { None }; 18];
    for (target, name) in parts.iter_mut().zip(names) {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let encoded = match *name {
            "audited_retained_bytes" => wire::counter_string(accounting.retained_bytes, s)?,
            "document_count" => wire::counter_string(accounting.documents, s)?,
            "origin_count" => wire::counter_string(prefix.origin_count(), s)?,
            "prefix_digest" => wire::string(prefix.prefix_digest(), s)?,
            "schema" => wire::string(schema, s)?,
            "verification_profile" => wire::string("ledger-authority-origin-audit/1", s)?,
            "current_grant_ref" => {
                let Some(value) = request_wrapper::optional_field(seed, name, s)? else {
                    continue;
                };
                s.canonical_representation(value, RepresentationKind::Record)
                    .map_err(core_error)?
            }
            _ => {
                let value = request_wrapper::field(seed, name, s)?;
                s.canonical_representation(value, RepresentationKind::Record)
                    .map_err(core_error)?
            }
        };
        s.semantic_work()
            .write_bytes(mem::size_of::<Option<AuditedBytes>>())
            .map_err(core_error)?;
        *target = Some(encoded);
    }
    s.semantic_work()
        .write_bytes(mem::size_of::<[(&str, &[u8]); 18]>())
        .map_err(core_error)?;
    let mut fields = [("", b"".as_slice()); 18];
    let mut output = fields.iter_mut();
    let mut count = 0;
    for (name, part) in names.iter().zip(&parts) {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        if let Some(part) = part {
            s.semantic_work()
                .write_bytes(mem::size_of::<(&str, &[u8])>())
                .map_err(core_error)?;
            *output.next().ok_or(ServiceError::IntegrityFailure)? = (name, part.bytes());
            count = add(count, 1, s)?;
        }
    }
    let body = wire::object(&fields[..count], 4194304, s)?;
    wire::source_record(kind, schema, body, s)
}

impl WitnessPrefixProduction<'_, '_, '_> {
    pub(in super::super) fn emit_source_header(
        &mut self,
        source_index: usize,
        seed: &Value,
    ) -> Result<wire::SourceRecord> {
        let result = (|| {
            self.session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            self.session
                .semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let prefix = self
                .produced
                .sources()
                .get(source_index)
                .ok_or(ServiceError::IntegrityFailure)?;
            self.session
                .semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let accounting = self
                .accounting
                .as_ref()
                .and_then(|counts| counts.get(source_index))
                .ok_or(ServiceError::IntegrityFailure)?;
            frame(true, seed, prefix, accounting, self.session)
        })();
        if result.is_err() {
            self.session.invalidate();
        }
        result
    }
    pub(in super::super) fn emit_financial_header(
        &mut self,
        seed: &Value,
    ) -> Result<wire::SourceRecord> {
        let result = (|| {
            self.session
                .semantic_work()
                .visit(Visit::Constraint)
                .map_err(core_error)?;
            self.session
                .semantic_work()
                .visit(Visit::RefLookup)
                .map_err(core_error)?;
            let accounting = self
                .accounting
                .as_ref()
                .and_then(|counts| counts.get(self.produced.sources().len()))
                .ok_or(ServiceError::IntegrityFailure)?;
            frame(
                false,
                seed,
                self.produced.financial(),
                accounting,
                self.session,
            )
        })();
        if result.is_err() {
            self.session.invalidate();
        }
        result
    }
}
