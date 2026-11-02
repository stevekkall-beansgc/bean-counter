//! Explicit local-operator credit DATA. The host owns local write admission,
//! completed original producers, the held cutoff, M5 identity and atomic append.
//! This module neither constructs a Workflow Action nor changes a causal head.
use super::*;

pub const REQUEST: &str = "ledger-billing-manual-credit/1";
pub const RECORD: &str = "ledger-billing-manual-credit-record/1";
pub const REF_KIND: &str = "billing-manual-credit";
pub const REQUEST_FIELDS: [&str; 9] = [
    "schema",
    "customer",
    "adjustment_id",
    "workflow_scope",
    "amount",
    "reason",
    "work_ref",
    "stage_instance_ref",
    "source_action_refs",
];
pub const DATA_FIELDS: [&str; 20] = [
    "schema",
    "customer",
    "adjustment_id",
    "workflow_scope",
    "amount",
    "reason",
    "work_ref",
    "stage_instance_ref",
    "source_action_refs",
    "binding_ref",
    "agreement_id",
    "agreement_version",
    "book",
    "role_ref",
    "slot_terms_ref",
    "funding_leg",
    "accepted_at",
    "operator",
    "workflow_boundary",
    "host",
];
pub const LINE_FIELDS: [&str; 14] = [
    "manual_credit_ref",
    "binding_ref",
    "agreement_id",
    "agreement_version",
    "book",
    "role_ref",
    "slot_terms_ref",
    "funding_leg",
    "amount",
    "reason",
    "work_ref",
    "stage_instance_ref",
    "source_action_refs",
    "line_id",
];

/// Each body is borrowed from the actual, completed original producer inventory.
/// `projected` is the existing project_action result for this exact Action.
pub struct Source<'a> {
    pub projected: &'a Value,
    pub decision: &'a Value,
    pub subject: WorkflowRecordData<'a>,
    pub dependencies: &'a [WorkflowRecordData<'a>],
}
pub struct Originals<'a> {
    pub work: WorkflowRecordData<'a>,
    pub stage: WorkflowRecordData<'a>,
    pub binding: WorkflowRecordData<'a>,
    pub roles: WorkflowRecordData<'a>,
    pub sources: &'a [Source<'a>],
}

fn typed_ref(v: &Value, kind: &str, prefix: &str, s: &mut OriginalAuditSession) -> Result<()> {
    closed(v, &["digest", "id", "kind"], s)?;
    require(same_text(text(v, "kind", s)?, kind, s)?, s)?;
    let id = text(v, "id", s)?;
    s.work().scan_bytes(id.len())?;
    crate::domain::prefixed(id, prefix)?;
    let digest = text(v, "digest", s)?;
    s.work().scan_bytes(digest.len())?;
    crate::domain::prefixed(digest, "sha256:")
}
pub fn credit_ref(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    typed_ref(v, REF_KIND, "m5r_", s)
}
fn work_ref(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    typed_ref(v, "workflow-work", "wevent_", s)
}
fn stage_ref(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    typed_ref(v, "workflow-stage-instance", "wstage_", s)
}
fn amount(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let n = atoms(v, s)?;
    s.work().scan_bytes(n.len())?;
    require(n.starts_with('-') && n.len() <= 31, s)
}
fn scope(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    closed(v, &["store", "scope"], s)?;
    require(!text(v, "store", s)?.is_empty(), s)?;
    let scope = field(v, "scope", s)?.as_array().ok_or_else(failure)?;
    require(scope.len() == 2, s)?;
    for component in scope {
        s.work().visit(Visit::Element)?;
        require(component.as_str().is_some_and(|v| !v.is_empty()), s)?;
    }
    Ok(())
}
/// Closed request DATA validation; never grants a mutation capability.
pub fn request(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    closed(v, &REQUEST_FIELDS, s)?;
    require(same_text(text(v, "schema", s)?, REQUEST, s)?, s)?;
    for name in ["customer", "adjustment_id"] {
        let value = text(v, name, s)?;
        s.work().scan_bytes(value.len())?;
        crate::domain::text(value, 128)?;
    }
    require(!text(v, "reason", s)?.is_empty(), s)?;
    scope(field(v, "workflow_scope", s)?, s)?;
    amount(field(v, "amount", s)?, s)?;
    work_ref(field(v, "work_ref", s)?, s)?;
    stage_ref(field(v, "stage_instance_ref", s)?, s)?;
    let refs = field(v, "source_action_refs", s)?;
    require(refs.as_array().is_some_and(|a| !a.is_empty()), s)?;
    basis(refs, s)
}
fn same_field(a: &Value, b: &Value, name: &str, s: &mut OriginalAuditSession) -> Result<()> {
    require(same(field(a, name, s)?, field(b, name, s)?, s)?, s)
}

/// Produce immutable credit DATA from already completed historical producers.
/// Explicit operator amount is not inferred, capped, or recomputed from sources.
pub fn project(
    req: &Value,
    originals: Originals<'_>,
    operator: &str,
    host: &str,
    accepted_at: &str,
    boundary: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    request(req, s)?;
    let accepted_time = crate::domain::Timestamp::parse_with_work(accepted_at, s.work())?.micros();
    let w = originals.work.body;
    let stage = originals.stage.body;
    let binding = originals.binding.body;
    let roles = originals.roles.body;
    require(
        same(field(req, "work_ref", s)?, originals.work.reference, s)?,
        s,
    )?;
    require(
        same(
            field(req, "stage_instance_ref", s)?,
            originals.stage.reference,
            s,
        )?,
        s,
    )?;
    require(
        same_text(text(w, "schema", s)?, "ledger-workflow-work-record/1", s)?,
        s,
    )?;
    require(
        same_text(
            text(stage, "schema", s)?,
            "ledger-workflow-stage-instance/1",
            s,
        )?,
        s,
    )?;
    let payload = field(w, "payload", s)?;
    require(
        same(
            field(payload, "stage_instance_ref", s)?,
            originals.stage.reference,
            s,
        )?,
        s,
    )?;
    for v in [payload, stage, binding, roles] {
        same_field(v, req, "customer", s)?;
    }
    for v in [payload, stage] {
        require(
            same(field(v, "binding_ref", s)?, originals.binding.reference, s)?,
            s,
        )?;
        same_field(v, stage, "chain", s)?;
    }
    same_field(stage, binding, "agreement_id", s)?;
    same_field(roles, binding, "agreement_id", s)?;
    let supplied_scope = field(req, "workflow_scope", s)?;
    let key = field(w, "key", s)?;
    require(
        same(
            field(key, "logical_store_id", s)?,
            field(supplied_scope, "store", s)?,
            s,
        )?,
        s,
    )?;
    same_field(key, supplied_scope, "scope", s)?;
    same_field(roles, supplied_scope, "scope", s)?;
    require(
        same_text(
            text(roles, "schema", s)?,
            "ledger-workflow-role-assignment/1",
            s,
        )?,
        s,
    )?;
    require(
        same_text(text(roles, "payer", s)?, text(req, "customer", s)?, s)?,
        s,
    )?;
    require(same_text(text(roles, "recipient", s)?, host, s)?, s)?;
    let mut member = false;
    for expected in field(stage, "expected", s)?
        .as_array()
        .ok_or_else(failure)?
    {
        s.work().visit(Visit::Element)?;
        if same(
            field(expected, "business_operation_id", s)?,
            field(payload, "business_operation_id", s)?,
            s,
        )? && same(field(expected, "source", s)?, field(key, "source", s)?, s)?
            && same(
                field(expected, "activity", s)?,
                field(payload, "activity", s)?,
                s,
            )?
        {
            require(!member, s)?;
            member = true;
        }
    }
    require(member, s)?;
    let requested = field(req, "source_action_refs", s)?
        .as_array()
        .ok_or_else(failure)?;
    s.work().arithmetic(
        Op::Compare,
        &[&requested.len().into(), &originals.sources.len().into()],
    )?;
    require(requested.len() == originals.sources.len(), s)?;
    let first = originals.sources.first().ok_or_else(failure)?.projected;
    let mut root_id = None;
    for source in originals.sources {
        s.work().visit(Visit::Element)?;
        if same(source.subject.reference, originals.work.reference, s)? {
            require(same(source.subject.body, w, s)?, s)?;
            let id = field(source.decision, "decision_id", s)?;
            if let Some(prior) = root_id {
                require(same(prior, id, s)?, s)?;
            }
            root_id = Some(id);
        }
    }
    let root_id = root_id.ok_or_else(failure)?;
    let mut saw_close = false;
    for (i, source) in originals.sources.iter().enumerate() {
        s.work().visit(Visit::Element)?;
        let line = source.projected;
        let decision = source.decision;
        let original_time =
            crate::domain::Timestamp::parse_with_work(text(decision, "accepted_at", s)?, s.work())?
                .micros();
        s.work()
            .arithmetic(Op::Compare, &[&accepted_time.into(), &original_time.into()])?;
        require(accepted_time >= original_time, s)?;
        let ar = field(line, "source_record_ref", s)?;
        member_once(field(req, "source_action_refs", s)?, ar, s)?;
        for prior in &originals.sources[..i] {
            s.work().visit(Visit::Element)?;
            require(
                !same(field(prior.projected, "source_record_ref", s)?, ar, s)?,
                s,
            )?;
        }
        require(same_text(text(line, "book", s)?, "retail", s)?, s)?;
        require(same_text(text(line, "posting_role", s)?, "original", s)?, s)?;
        let original_atoms = atoms(field(line, "amount", s)?, s)?;
        require(!original_atoms.starts_with('-') && original_atoms != "0", s)?;
        for name in [
            "binding_ref",
            "agreement_id",
            "agreement_version",
            "book",
            "role_ref",
            "slot_terms_ref",
            "funding_leg",
        ] {
            same_field(first, line, name, s)?;
        }
        require(
            same(
                field(line, "binding_ref", s)?,
                originals.binding.reference,
                s,
            )?,
            s,
        )?;
        require(
            same(field(line, "role_ref", s)?, originals.roles.reference, s)?,
            s,
        )?;
        require(
            same(
                field(decision, "subject_ref", s)?,
                source.subject.reference,
                s,
            )?,
            s,
        )?;
        require(
            same(
                field(decision, "original_terms_ref", s)?,
                originals.binding.reference,
                s,
            )?,
            s,
        )?;
        member_once(field(decision, "actions_appended", s)?, ar, s)?;
        require(
            same_text(text(decision, "transition", s)?, "initial", s)?,
            s,
        )?;
        if same(source.subject.reference, originals.work.reference, s)? {
            require(same_text(text(decision, "kind", s)?, "work", s)?, s)?;
        } else {
            let subject = source.subject.body;
            require(
                same_text(
                    text(subject, "schema", s)?,
                    "ledger-workflow-stage-closure-claim/1",
                    s,
                )?,
                s,
            )?;
            require(
                same(
                    field(subject, "stage_instance_ref", s)?,
                    originals.stage.reference,
                    s,
                )?,
                s,
            )?;
            require(same_text(text(decision, "kind", s)?, "outcome", s)?, s)?;
            let mut linked = false;
            for dependency in source.dependencies {
                s.work().visit(Visit::Element)?;
                member_once(
                    field(decision, "dependency_refs", s)?,
                    dependency.reference,
                    s,
                )?;
                let d = dependency.body;
                if same(field(d, "ancestor_decision_id", s)?, root_id, s)? {
                    require(!linked, s)?;
                    require(
                        same_text(text(d, "schema", s)?, "ledger-workflow-dependency/2", s)?,
                        s,
                    )?;
                    require(same_text(text(d, "kind", s)?, "work_basis", s)?, s)?;
                    require(
                        same(field(d, "target_ref", s)?, originals.stage.reference, s)?,
                        s,
                    )?;
                    require(
                        same(
                            field(d, "dependent_decision_id", s)?,
                            field(decision, "decision_id", s)?,
                            s,
                        )?,
                        s,
                    )?;
                    require(
                        same(
                            field(d, "original_terms_ref", s)?,
                            originals.binding.reference,
                            s,
                        )?,
                        s,
                    )?;
                    require(same(field(d, "operation_key", s)?, key, s)?, s)?;
                    linked = true;
                }
            }
            require(linked, s)?;
            saw_close = true;
        }
    }
    require(saw_close, s)?;
    closed(boundary, &["store", "scope", "commit_sequence"], s)?;
    same_field(boundary, supplied_scope, "store", s)?;
    same_field(boundary, supplied_scope, "scope", s)?;
    let arena = s.arena().shared_handle();
    let schema = arena.string_value(RECORD, s.work())?;
    let accepted = arena.string_value(accepted_at, s.work())?;
    let operator = arena.string_value(operator, s.work())?;
    let host = arena.string_value(host, s.work())?;
    arena.object_from_fields(
        &[
            ("schema", schema.value()),
            ("customer", field(req, "customer", s)?),
            ("adjustment_id", field(req, "adjustment_id", s)?),
            ("workflow_scope", supplied_scope),
            ("amount", field(req, "amount", s)?),
            ("reason", field(req, "reason", s)?),
            ("work_ref", originals.work.reference),
            ("stage_instance_ref", originals.stage.reference),
            ("source_action_refs", field(req, "source_action_refs", s)?),
            ("binding_ref", originals.binding.reference),
            ("agreement_id", field(first, "agreement_id", s)?),
            ("agreement_version", field(first, "agreement_version", s)?),
            ("book", field(first, "book", s)?),
            ("role_ref", originals.roles.reference),
            ("slot_terms_ref", field(first, "slot_terms_ref", s)?),
            ("funding_leg", field(first, "funding_leg", s)?),
            ("accepted_at", accepted.value()),
            ("operator", operator.value()),
            ("workflow_boundary", boundary),
            ("host", host.value()),
        ],
        s.work(),
    )
}

pub fn line(
    reference: &Value,
    record: &Value,
    view_kind: &str,
    view_identity: &Value,
    workflow_scope: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    credit_ref(reference, s)?;
    require(same_text(text(record, "schema", s)?, RECORD, s)?, s)?;
    require(
        same(field(record, "workflow_scope", s)?, workflow_scope, s)?,
        s,
    )?;
    require(
        same(
            field(record, "customer", s)?,
            field(view_identity, "customer", s)?,
            s,
        )?,
        s,
    )?;
    match view_kind {
        "standard" => closed(view_identity, &["customer", "period_id"], s)?,
        "ad_hoc" => closed(view_identity, &["customer", "command_id"], s)?,
        _ => return Err(failure()),
    }
    amount(field(record, "amount", s)?, s)?;
    let mut fields = [("", &Value::Null); 14];
    fields[0] = ("manual_credit_ref", reference);
    for (i, name) in LINE_FIELDS[1..13].iter().enumerate() {
        fields[i + 1] = (name, field(record, name, s)?);
    }
    let arena = s.arena().shared_handle();
    let data = arena.object_from_fields(&fields[..13], s.work())?;
    let kind = arena.string_value(view_kind, s.work())?;
    let framed = arena.object_from_fields(
        &[
            ("view_kind", kind.value()),
            ("view_identity", view_identity),
            ("workflow_scope", workflow_scope),
            ("line", data.value()),
        ],
        s.work(),
    )?;
    let encoded = s.canonical_representation(framed.value(), RepresentationKind::Record)?;
    const DOMAIN: &[u8] = b"bean-counter/m5/manual-credit-line/1\0";
    s.work().hash_bytes(DOMAIN.len())?;
    s.work().hash_bytes(encoded.bytes().len())?;
    let mut hash = Sha256::new();
    hash.update(DOMAIN);
    hash.update(encoded.bytes());
    let digest = hash.finalize();
    let mut hex = [0u8; 64];
    s.work().write_bytes(hex.len())?;
    for (i, b) in digest.iter().enumerate() {
        hex[2 * i] = b"0123456789abcdef"[(b >> 4) as usize];
        hex[2 * i + 1] = b"0123456789abcdef"[(b & 15) as usize];
    }
    let id = arena.string_value(std::str::from_utf8(&hex).map_err(|_| failure())?, s.work())?;
    fields[13] = ("line_id", id.value());
    arena.object_from_fields(&fields, s.work())
}

/// Closed manual line syntax. Full record/original proof remains the host owner.
pub(super) fn validate_line(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    closed(v, &LINE_FIELDS, s)?;
    credit_ref(field(v, "manual_credit_ref", s)?, s)?;
    reference(field(v, "binding_ref", s)?, "workflow-binding", s)?;
    reference(field(v, "role_ref", s)?, "workflow-roles", s)?;
    reference(field(v, "slot_terms_ref", s)?, "workflow-slot-terms", s)?;
    work_ref(field(v, "work_ref", s)?, s)?;
    stage_ref(field(v, "stage_instance_ref", s)?, s)?;
    amount(field(v, "amount", s)?, s)?;
    require(same_text(text(v, "book", s)?, "retail", s)?, s)?;
    for name in [
        "agreement_id",
        "agreement_version",
        "funding_leg",
        "reason",
        "line_id",
    ] {
        require(!text(v, name, s)?.is_empty(), s)?;
    }
    basis(field(v, "source_action_refs", s)?, s)
}

fn controls(
    count: usize,
    width: usize,
    s: &mut OriginalAuditSession,
) -> Result<crate::canonical::audit_arena::ExternalBufferLease> {
    s.work()
        .arithmetic(Op::Multiply, &[&count.into(), &width.into()])?;
    let bytes = count.checked_mul(width).ok_or_else(failure)?;
    let owner = s.reserve_external_payload(bytes)?;
    s.work().write_bytes(bytes)?;
    Ok(owner)
}
/// Copy completed-producer DATA while retaining its paid allocation. This is
/// packaging only; it does not complete or certify the supplied producer.
pub fn pack_source(source: Source<'_>, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    let arena = s.arena().shared_handle();
    let count = source.dependencies.len();
    let _owner = controls(
        count,
        std::mem::size_of::<AuditedValue>() + std::mem::size_of::<&Value>(),
        s,
    )?;
    let mut values = Vec::new();
    values.try_reserve_exact(count).map_err(|_| failure())?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(count).map_err(|_| failure())?;
    require(values.capacity() <= count && refs.capacity() <= count, s)?;
    for d in source.dependencies {
        s.work().visit(Visit::Element)?;
        values.push(
            arena.object_from_fields(&[("reference", d.reference), ("body", d.body)], s.work())?,
        );
    }
    for value in &values {
        s.work().visit(Visit::Element)?;
        refs.push(value.value());
    }
    let dependencies = arena.array_from_values(&refs, s.work())?;
    arena.object_from_fields(
        &[
            ("projected", source.projected),
            ("decision", source.decision),
            ("subject_ref", source.subject.reference),
            ("subject_body", source.subject.body),
            ("dependencies", dependencies.value()),
        ],
        s.work(),
    )
}
pub fn pack_originals(
    work: WorkflowRecordData<'_>,
    stage: WorkflowRecordData<'_>,
    binding: WorkflowRecordData<'_>,
    roles: WorkflowRecordData<'_>,
    sources: &[AuditedValue],
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let _owner = controls(sources.len(), std::mem::size_of::<&Value>(), s)?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(sources.len())
        .map_err(|_| failure())?;
    require(refs.capacity() <= sources.len(), s)?;
    for value in sources {
        s.work().visit(Visit::Element)?;
        refs.push(value.value());
    }
    let arena = s.arena().shared_handle();
    let sources = arena.array_from_values(&refs, s.work())?;
    arena.object_from_fields(
        &[
            ("work_ref", work.reference),
            ("work", work.body),
            ("stage_ref", stage.reference),
            ("stage", stage.body),
            ("binding_ref", binding.reference),
            ("binding", binding.body),
            ("roles_ref", roles.reference),
            ("roles", roles.body),
            ("sources", sources.value()),
        ],
        s.work(),
    )
}
pub fn project_packed(
    req: &Value,
    packed: &Value,
    operator: &str,
    host: &str,
    accepted: &str,
    boundary: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    closed(
        packed,
        &[
            "work_ref",
            "work",
            "stage_ref",
            "stage",
            "binding_ref",
            "binding",
            "roles_ref",
            "roles",
            "sources",
        ],
        s,
    )?;
    let rows = field(packed, "sources", s)?
        .as_array()
        .ok_or_else(failure)?;
    let _owner = controls(
        rows.len(),
        std::mem::size_of::<Vec<WorkflowRecordData<'_>>>() + std::mem::size_of::<Source<'_>>(),
        s,
    )?;
    // Reserve aggregate inner controls before allocation; they outlive every
    // borrowed view, including on an early error.
    let mut n = 0usize;
    for row in rows {
        s.work().visit(Visit::Element)?;
        let len = field(row, "dependencies", s)?
            .as_array()
            .ok_or_else(failure)?
            .len();
        s.work().arithmetic(Op::Add, &[&n.into(), &len.into()])?;
        n = n.checked_add(len).ok_or_else(failure)?;
    }
    let _dependencies = controls(n, std::mem::size_of::<WorkflowRecordData<'_>>(), s)?;
    let mut dependencies = Vec::new();
    dependencies
        .try_reserve_exact(rows.len())
        .map_err(|_| failure())?;
    let mut sources = Vec::new();
    sources
        .try_reserve_exact(rows.len())
        .map_err(|_| failure())?;
    require(
        dependencies.capacity() <= rows.len() && sources.capacity() <= rows.len(),
        s,
    )?;
    for row in rows {
        s.work().visit(Visit::Element)?;
        closed(
            row,
            &[
                "projected",
                "decision",
                "subject_ref",
                "subject_body",
                "dependencies",
            ],
            s,
        )?;
        let array = field(row, "dependencies", s)?
            .as_array()
            .ok_or_else(failure)?;
        let mut list = Vec::new();
        list.try_reserve_exact(array.len()).map_err(|_| failure())?;
        require(list.capacity() <= array.len(), s)?;
        for d in array {
            s.work().visit(Visit::Element)?;
            closed(d, &["reference", "body"], s)?;
            list.push(WorkflowRecordData {
                reference: field(d, "reference", s)?,
                body: field(d, "body", s)?,
            });
        }
        dependencies.push(list);
    }
    for (row, dependencies) in rows.iter().zip(&dependencies) {
        s.work().visit(Visit::Element)?;
        sources.push(Source {
            projected: field(row, "projected", s)?,
            decision: field(row, "decision", s)?,
            subject: WorkflowRecordData {
                reference: field(row, "subject_ref", s)?,
                body: field(row, "subject_body", s)?,
            },
            dependencies,
        });
    }
    project(
        req,
        Originals {
            work: WorkflowRecordData {
                reference: field(packed, "work_ref", s)?,
                body: field(packed, "work", s)?,
            },
            stage: WorkflowRecordData {
                reference: field(packed, "stage_ref", s)?,
                body: field(packed, "stage", s)?,
            },
            binding: WorkflowRecordData {
                reference: field(packed, "binding_ref", s)?,
                body: field(packed, "binding", s)?,
            },
            roles: WorkflowRecordData {
                reference: field(packed, "roles_ref", s)?,
                body: field(packed, "roles", s)?,
            },
            sources: &sources,
        },
        operator,
        host,
        accepted,
        boundary,
        s,
    )
}

/// Mechanical exact match after the host has completed the retained source
/// proof and projected it at the saved acceptance/cutoff. Envelope custody is
/// checked by the M5 store, separately from these economic DATA associations.
pub fn compare_record(
    projected: &Value,
    record: &Value,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    require(
        record
            .as_object()
            .is_some_and(|v| v.len() == DATA_FIELDS.len() + 1),
        s,
    )?;
    field(record, "record", s)?;
    for name in DATA_FIELDS {
        require(
            same(field(projected, name, s)?, field(record, name, s)?, s)?,
            s,
        )?;
    }
    Ok(())
}

/// Additive line assembly: old and manual source variants remain distinct,
/// while the exact retained obligation fields determine totals.
pub fn combine_lines(
    base: &Value,
    manual: &[AuditedValue],
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let base = base.as_array().ok_or_else(failure)?;
    s.work()
        .arithmetic(Op::Add, &[&base.len().into(), &manual.len().into()])?;
    let count = base.len().checked_add(manual.len()).ok_or_else(failure)?;
    let _owner = controls(count, std::mem::size_of::<&Value>(), s)?;
    let mut rows = Vec::new();
    rows.try_reserve_exact(count).map_err(|_| failure())?;
    require(rows.capacity() <= count, s)?;
    for row in base {
        s.work().visit(Visit::Element)?;
        rows.push(row);
    }
    for row in manual {
        s.work().visit(Visit::Element)?;
        rows.push(row.value());
    }
    crate::validation_work::try_sort_by(&mut rows, s.work(), |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        let a = a["line_id"].as_str().ok_or_else(failure)?;
        let b = b["line_id"].as_str().ok_or_else(failure)?;
        w.scan_bytes(a.len())?;
        w.scan_bytes(b.len())?;
        Ok(a.cmp(b))
    })?;
    for pair in rows.windows(2) {
        s.work().visit(Visit::Element)?;
        require(
            !same(
                field(pair[0], "line_id", s)?,
                field(pair[1], "line_id", s)?,
                s,
            )?,
            s,
        )?;
    }
    let arena = s.arena().shared_handle();
    arena.array_from_values(&rows, s.work())
}
pub fn line_references(
    lines: &[AuditedValue],
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let count = lines.len();
    let _owner = controls(
        count,
        std::mem::size_of::<(AuditedBytes, &Value)>() + std::mem::size_of::<&Value>(),
        s,
    )?;
    let mut sorted = Vec::new();
    sorted.try_reserve_exact(count).map_err(|_| failure())?;
    let mut refs = Vec::new();
    refs.try_reserve_exact(count).map_err(|_| failure())?;
    require(sorted.capacity() <= count && refs.capacity() <= count, s)?;
    for line in lines {
        s.work().visit(Visit::Element)?;
        let r = field(line.value(), "manual_credit_ref", s)?;
        sorted.push((
            s.canonical_representation(r, RepresentationKind::Record)?,
            r,
        ));
    }
    crate::validation_work::try_sort_by(&mut sorted, s.work(), |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(a.0.bytes().len())?;
        w.scan_bytes(b.0.bytes().len())?;
        Ok(a.0.bytes().cmp(b.0.bytes()))
    })?;
    for pair in sorted.windows(2) {
        s.work().visit(Visit::Element)?;
        require(pair[0].0.bytes() != pair[1].0.bytes(), s)?;
    }
    for (_, r) in &sorted {
        s.work().visit(Visit::Element)?;
        refs.push(*r);
    }
    let arena = s.arena().shared_handle();
    arena.array_from_values(&refs, s.work())
}

pub fn selector(v: &Value, read: bool, s: &mut OriginalAuditSession) -> Result<()> {
    closed(
        v,
        if read {
            &["schema", "customer", "workflow_scope", "credit_ref"]
        } else {
            &["schema", "customer", "workflow_scope", "adjustment_id"]
        },
        s,
    )?;
    require(
        same_text(
            text(v, "schema", s)?,
            if read {
                "ledger-billing-manual-credit-read/1"
            } else {
                "ledger-billing-manual-credit-status/1"
            },
            s,
        )?,
        s,
    )?;
    for key in if read {
        &["customer"][..]
    } else {
        &["customer", "adjustment_id"][..]
    } {
        let value = text(v, key, s)?;
        s.work().scan_bytes(value.len())?;
        crate::domain::text(value, 128)?;
    }
    scope(field(v, "workflow_scope", s)?, s)?;
    if read {
        credit_ref(field(v, "credit_ref", s)?, s)?;
    }
    Ok(())
}

pub fn identity(
    customer: &str,
    scope: &Value,
    id: &str,
    s: &mut OriginalAuditSession,
) -> Result<AuditedBytes> {
    let arena = s.arena().shared_handle();
    let customer = arena.string_value(customer, s.work())?;
    let id = arena.string_value(id, s.work())?;
    let key = arena.object_from_fields(
        &[("workflow_scope", scope), ("adjustment_id", id.value())],
        s.work(),
    )?;
    let schema = arena.string_value("ledger-billing-m5-command-identity/1", s.work())?;
    let domain = arena.string_value("customer-admin", s.work())?;
    let family = arena.string_value(REQUEST, s.work())?;
    let kind = arena.string_value("manual_credit", s.work())?;
    let value = arena.object_from_fields(
        &[
            ("schema", schema.value()),
            ("domain", domain.value()),
            ("family", family.value()),
            ("customer", customer.value()),
            ("key_kind", kind.value()),
            ("key", key.value()),
        ],
        s.work(),
    )?;
    let bytes = s.canonical_representation(value.value(), RepresentationKind::Record)?;
    require(bytes.bytes().len() <= 4096, s)?;
    Ok(bytes)
}
pub fn child_key(request: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let arena = s.arena().shared_handle();
    let role = arena.string_value("manual-credit", s.work())?;
    let key = arena.object_from_fields(
        &[
            ("role", role.value()),
            ("customer", field(request, "customer", s)?),
            ("workflow_scope", field(request, "workflow_scope", s)?),
            ("adjustment_id", field(request, "adjustment_id", s)?),
        ],
        s.work(),
    )?;
    s.canonical_representation(key.value(), RepresentationKind::Record)
}
pub fn claim_child_key(claim: &Value, s: &mut OriginalAuditSession) -> Result<AuditedBytes> {
    let arena = s.arena().shared_handle();
    let role = arena.string_value("manual-credit-membership", s.work())?;
    let key = arena.object_from_fields(
        &[
            ("role", role.value()),
            ("customer", field(claim, "customer", s)?),
            ("workflow_scope", field(claim, "workflow_scope", s)?),
            ("manual_credit_ref", field(claim, "manual_credit_ref", s)?),
        ],
        s.work(),
    )?;
    s.canonical_representation(key.value(), RepresentationKind::Record)
}
pub fn presentation_view(
    statement: &Value,
    standard: bool,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let arena = s.arena().shared_handle();
    let fields = if standard {
        [
            ("customer", field(statement, "customer", s)?),
            ("period_id", field(statement, "period_id", s)?),
        ]
    } else {
        [
            ("customer", field(statement, "customer", s)?),
            ("command_id", field(statement, "statement_id", s)?),
        ]
    };
    arena.object_from_fields(&fields, s.work())
}
fn hash_string(
    prefix: &str,
    parts: &[&[u8]],
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let mut hash = Sha256::new();
    for part in parts {
        s.work().visit(Visit::Element)?;
        s.work().hash_bytes(part.len())?;
        hash.update(part);
    }
    let digest = hash.finalize();
    let mut hex = [0u8; 71];
    require(prefix.len() <= 7, s)?;
    s.work().write_bytes(prefix.len() + 64)?;
    hex[..prefix.len()].copy_from_slice(prefix.as_bytes());
    for (i, b) in digest.iter().enumerate() {
        hex[prefix.len() + 2 * i] = b"0123456789abcdef"[(b >> 4) as usize];
        hex[prefix.len() + 2 * i + 1] = b"0123456789abcdef"[(b & 15) as usize];
    }
    let arena = s.arena().shared_handle();
    arena.string_value(
        std::str::from_utf8(&hex[..prefix.len() + 64]).map_err(|_| failure())?,
        s.work(),
    )
}
pub fn reference_for_record(record: &Value, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    let bytes = s.canonical_representation(record, RepresentationKind::Record)?;
    let digest = hash_string("sha256:", &[bytes.bytes()], s)?;
    let arena = s.arena().shared_handle();
    let kind = arena.string_value(REF_KIND, s.work())?;
    let reference = arena.object_from_fields(
        &[
            ("kind", kind.value()),
            ("id", field(field(record, "record", s)?, "record_id", s)?),
            ("digest", digest.value()),
        ],
        s.work(),
    )?;
    credit_ref(reference.value(), s)?;
    Ok(reference)
}
pub fn payload_hash(
    record: &Value,
    family: &str,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let names: &[&str] = if same_text(family, RECORD, s)? {
        &DATA_FIELDS
    } else if same_text(
        family,
        "ledger-billing-manual-credit-presentation-claim/1",
        s,
    )? {
        &[
            "schema",
            "customer",
            "workflow_scope",
            "manual_credit_ref",
            "presentation_kind",
            "statement_id",
            "statement_hash",
        ]
    } else {
        return Err(failure());
    };
    require(
        record
            .as_object()
            .is_some_and(|o| o.len() == names.len() + 1),
        s,
    )?;
    let envelope = field(record, "record", s)?;
    closed(
        envelope,
        &[
            "record_id",
            "sequence",
            "command_sequence",
            "accepted_at",
            "payload_hash",
        ],
        s,
    )?;
    let arena = s.arena().shared_handle();
    let unsigned = arena.object_from_fields(
        &[
            ("record_id", field(envelope, "record_id", s)?),
            ("sequence", field(envelope, "sequence", s)?),
            ("command_sequence", field(envelope, "command_sequence", s)?),
            ("accepted_at", field(envelope, "accepted_at", s)?),
        ],
        s.work(),
    )?;
    let mut fields = [("", &Value::Null); 21];
    for (i, name) in names.iter().enumerate() {
        s.work().visit(Visit::Field)?;
        fields[i] = (name, field(record, name, s)?);
    }
    fields[names.len()] = ("record", unsigned.value());
    let value = arena.object_from_fields(&fields[..names.len() + 1], s.work())?;
    let bytes = s.canonical_representation(value.value(), RepresentationKind::Record)?;
    hash_string(
        "",
        &[
            b"bean-counter/m5/record/1\0",
            family.as_bytes(),
            b"\0",
            bytes.bytes(),
        ],
        s,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::audit_arena::AuditArena;
    use serde_json::json;

    // These are authentic saved synthetic DATA inputs, not a substitute for
    // the host's original producer proof or current owner authorization.
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../../../contracts/candidates/manual-credit-m5/vectors/source-data.json"
        ))
        .unwrap()
    }
    fn projected(v: &Value, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
        project_packed(
            &v["request"],
            &v["packed"],
            v["operator"].as_str().unwrap(),
            v["host"].as_str().unwrap(),
            v["accepted_at"].as_str().unwrap(),
            &v["boundary"],
            s,
        )
    }
    #[test]
    fn manual_credit_data_preserves_original_join_and_explicit_credit() {
        let v = fixture();
        let arena = AuditArena::new().unwrap();
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let record = projected(&v, &mut s).unwrap();
        assert_eq!(record.value(), &v["expected_record"]);
        // The Stage anchor is B; the credited Work D is a different member,
        // proved through the actual closure dependency and expected source.
        let closure = v["packed"]["sources"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["subject_body"]["schema"] == "ledger-workflow-stage-closure-claim/1")
            .unwrap();
        assert_ne!(
            closure["subject_body"]["anchor_work_ref"],
            v["request"]["work_ref"]
        );
        let line = line(
            &v["test_credit_ref"],
            record.value(),
            "ad_hoc",
            &v["line_view"],
            &v["request"]["workflow_scope"],
            &mut s,
        )
        .unwrap();
        assert_eq!(line.value(), &v["expected_line"]);
        let total = super::super::totals_with_manual(&[line.value().clone()], &mut s).unwrap();
        assert_eq!(total.value()[0]["amount"]["atoms"], v["expected_atoms"]);
        assert_eq!(total.value()[0]["direction"], "payable");
        assert!(super::super::totals(&[line.value().clone()], &mut s).is_err());
        for mutation in 0..11 {
            let mut changed = fixture();
            match mutation {
                0 => changed["request"]["amount"]["atoms"] = json!("0"),
                1 => changed["request"]["amount"]["atoms"] = json!("-0"),
                2 => changed["request"]["amount"]["atoms"] = json!("1"),
                3 => {
                    changed["request"]["amount"]["atoms"] =
                        json!("-1000000000000000000000000000000")
                }
                4 => changed["request"]["amount"]["currency"] = json!("EUR"),
                5 => changed["packed"]["roles"]["payer"] = json!("other-customer"),
                6 => changed["packed"]["roles"]["recipient"] = json!("other-host"),
                7 => changed["packed"]["work"]["key"]["source"] = json!("urn:other:source"),
                8 => changed["accepted_at"] = json!("2026-10-31T00:00:00.000000Z"),
                9 => {
                    for source in changed["packed"]["sources"].as_array_mut().unwrap() {
                        if source["subject_body"]["schema"]
                            == "ledger-workflow-stage-closure-claim/1"
                        {
                            source["dependencies"] = json!([])
                        }
                    }
                }
                _ => changed["request"]["unrequested_authority"] = json!("operator"),
            }
            let arena = AuditArena::new().unwrap();
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            assert!(projected(&changed, &mut s).is_err(), "mutation {mutation}");
        }
    }
}
