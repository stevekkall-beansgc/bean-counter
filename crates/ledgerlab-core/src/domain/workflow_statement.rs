//! Workflow statement projection DATA, never an original-producer certificate.
//! The host must finish original semantic/journal verification and the complete
//! held cutoff before using these rows in an atomic presentation and claim set.
use crate::{
    canonical::audit_arena::{AuditedBytes, AuditedValue},
    validation_work::{
        self as work, ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
    Error, Result,
};
use num_bigint::BigInt;
use num_integer::Integer;
use num_traits::{ToPrimitive, Zero};
use serde_json::Value;
use sha2::{Digest, Sha256};
pub mod manual_credit;

pub struct WorkflowRecordData<'a> {
    pub reference: &'a Value,
    pub body: &'a Value,
}
pub struct WorkflowActionData<'a> {
    pub action: WorkflowRecordData<'a>,
    pub decision: WorkflowRecordData<'a>,
    pub evaluation: WorkflowRecordData<'a>,
    pub binding: WorkflowRecordData<'a>,
    pub producer_receipt_ref: &'a Value,
    pub producer_key: &'a Value,
}

fn failure() -> Error {
    Error::new(
        "WORKFLOW_STATEMENT_INTEGRITY",
        "workflow statement DATA association",
    )
}
fn require(ok: bool, s: &mut OriginalAuditSession) -> Result<()> {
    s.work().visit(Visit::Constraint)?;
    if ok {
        Ok(())
    } else {
        Err(failure())
    }
}
fn same_text(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.work().visit(Visit::MemberCompare)?;
    s.work().scan_bytes(a.len())?;
    s.work().scan_bytes(b.len())?;
    Ok(a == b)
}
fn optional_field<'a>(
    v: &'a Value,
    name: &str,
    s: &mut OriginalAuditSession,
) -> Result<Option<&'a Value>> {
    for (k, v) in v.as_object().ok_or_else(failure)? {
        s.work().visit(Visit::Field)?;
        if same_text(k, name, s)? {
            return Ok(Some(v));
        }
    }
    Ok(None)
}
fn field<'a>(v: &'a Value, name: &str, s: &mut OriginalAuditSession) -> Result<&'a Value> {
    optional_field(v, name, s)?.ok_or_else(failure)
}
fn text<'a>(v: &'a Value, name: &str, s: &mut OriginalAuditSession) -> Result<&'a str> {
    field(v, name, s)?.as_str().ok_or_else(failure)
}
fn closed(v: &Value, names: &[&str], s: &mut OriginalAuditSession) -> Result<()> {
    let object = v.as_object().ok_or_else(failure)?;
    require(object.len() == names.len(), s)?;
    for name in names {
        field(v, name, s)?;
    }
    Ok(())
}
fn same(a: &Value, b: &Value, s: &mut OriginalAuditSession) -> Result<bool> {
    let a = s.canonical_representation(a, RepresentationKind::Record)?;
    let b = s.canonical_representation(b, RepresentationKind::Record)?;
    s.work().visit(Visit::MemberCompare)?;
    s.work().scan_bytes(a.bytes().len())?;
    s.work().scan_bytes(b.bytes().len())?;
    Ok(a.bytes() == b.bytes())
}
fn reference(v: &Value, kind: &str, s: &mut OriginalAuditSession) -> Result<()> {
    closed(v, &["digest", "id", "kind"], s)?;
    require(same_text(text(v, "kind", s)?, kind, s)?, s)?;
    let id = text(v, "id", s)?;
    let prefix = match kind {
        "workflow-action" => "waction_",
        "workflow-decision" => "wdrev_",
        "workflow-receipt" => "wreceipt_",
        _ => "wdoc_",
    };
    s.work().scan_bytes(id.len())?;
    super::prefixed(id, prefix)?;
    let digest = text(v, "digest", s)?;
    s.work().scan_bytes(digest.len())?;
    super::prefixed(digest, "sha256:")
}
fn member_once(array: &Value, selected: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let mut found = false;
    for item in array.as_array().ok_or_else(failure)? {
        s.work().visit(Visit::Element)?;
        if same(item, selected, s)? {
            require(!found, s)?;
            found = true;
        }
    }
    require(found, s)
}
fn basis(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let mut prior: Option<AuditedBytes> = None;
    for r in v.as_array().ok_or_else(failure)? {
        s.work().visit(Visit::Element)?;
        reference(r, "workflow-action", s)?;
        let bytes = s.canonical_representation(r, RepresentationKind::Record)?;
        if let Some(p) = &prior {
            s.work().visit(Visit::MemberCompare)?;
            s.work().scan_bytes(p.bytes().len())?;
            s.work().scan_bytes(bytes.bytes().len())?;
            require(p.bytes() < bytes.bytes(), s)?;
        }
        prior = Some(bytes);
    }
    Ok(())
}
// A conservative 64-byte temporary bound matching the existing arithmetic
// parser. The producer's closed Action schema remains the accepted wire
// language; this DATA helper does not enlarge it. No floating-point money.
fn atoms<'a>(amount: &'a Value, s: &mut OriginalAuditSession) -> Result<&'a str> {
    closed(amount, &["atoms", "currency", "scale"], s)?;
    require(same_text(text(amount, "currency", s)?, "USD", s)?, s)?;
    require(field(amount, "scale", s)?.as_u64() == Some(18), s)?;
    let text = text(amount, "atoms", s)?;
    signed_atoms(text, s)
}
fn signed_atoms<'a>(text: &'a str, s: &mut OriginalAuditSession) -> Result<&'a str> {
    s.work().scan_bytes(text.len())?;
    require(!text.is_empty() && text.len() <= 64, s)?;
    let digits = text.strip_prefix('-').unwrap_or(text);
    require(
        !digits.is_empty() && !(digits.len() > 1 && digits.starts_with('0')) && text != "-0",
        s,
    )?;
    for d in digits.bytes() {
        s.work().visit(Visit::ScalarDigit)?;
        require(d.is_ascii_digit(), s)?;
    }
    Ok(text)
}
fn number(text: &str, s: &mut OriginalAuditSession) -> Result<BigInt> {
    let mut n = BigInt::zero();
    let ten = BigInt::from(10);
    for d in text.trim_start_matches('-').bytes() {
        s.work().visit(Visit::ScalarDigit)?;
        s.work().arithmetic(Op::Multiply, &[&n, &ten])?;
        let product = &n * &ten;
        s.work()
            .arithmetic(Op::Subtract, &[&d.into(), &b'0'.into()])?;
        let digit = BigInt::from(d - b'0');
        s.work().arithmetic(Op::Add, &[&product, &digit])?;
        n = product + digit;
    }
    if text.starts_with('-') {
        s.work().arithmetic(Op::Negate, &[&n])?;
        n = -n;
    }
    Ok(n)
}

/// Check the joins of an already certified producer, then copy its exact facts.
/// The returned owner retains its allocation in the supplied attempt's arena.
pub fn project_action(
    input: WorkflowActionData<'_>,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let a = input.action.body;
    let d = input.decision.body;
    let e = input.evaluation.body;
    let b = input.binding.body;
    for (r, kind) in [
        (input.action.reference, "workflow-action"),
        (input.decision.reference, "workflow-decision"),
        (input.evaluation.reference, "workflow-evaluation"),
        (input.binding.reference, "workflow-binding"),
        (input.producer_receipt_ref, "workflow-receipt"),
    ] {
        reference(r, kind, s)?;
    }
    require(
        same_text(text(a, "schema", s)?, "ledger-workflow-action/1", s)?,
        s,
    )?;
    require(
        matches!(
            text(b, "schema", s)?,
            "ledger-workflow-binding/1" | "ledger-workflow-binding/2"
        ),
        s,
    )?;
    require(
        same_text(
            text(e, "schema", s)?,
            "ledger-workflow-evaluation-proof/1",
            s,
        )?,
        s,
    )?;
    require(
        matches!(
            text(d, "schema", s)?,
            "ledger-workflow-decision-revision/1"
                | "ledger-workflow-decision-revision/2"
                | "ledger-workflow-decision-revision/3"
        ),
        s,
    )?;
    for (left, right) in [
        (field(a, "operation_key", s)?, input.producer_key),
        (field(d, "operation_key", s)?, input.producer_key),
        (field(a, "decision_id", s)?, field(d, "decision_id", s)?),
        (field(a, "revision", s)?, field(d, "revision", s)?),
        (field(d, "evaluation_ref", s)?, input.evaluation.reference),
        (field(d, "original_terms_ref", s)?, input.binding.reference),
        (field(e, "binding_ref", s)?, input.binding.reference),
    ] {
        require(same(left, right, s)?, s)?;
    }
    member_once(field(d, "actions_appended", s)?, input.action.reference, s)?;
    let role = text(a, "posting_role", s)?;
    require(matches!(role, "original" | "inverse" | "replacement"), s)?;
    let inverse = a.as_object().ok_or_else(failure)?.get("inverse_of");
    require(inverse.is_some() == (role == "inverse"), s)?;
    if let Some(r) = inverse {
        reference(r, "workflow-action", s)?;
    }
    require(matches!(text(a, "book", s)?, "retail" | "supplier"), s)?;
    let amount = field(a, "amount", s)?;
    require(atoms(amount, s)? != "0", s)?;
    let bases = field(a, "basis_refs", s)?;
    basis(bases, s)?;
    let mut step_found = false;
    for step in field(e, "steps", s)?.as_array().ok_or_else(failure)? {
        s.work().visit(Visit::Element)?;
        if let Some(r) = step.as_object().ok_or_else(failure)?.get("action_ref") {
            if same(r, input.action.reference, s)? {
                require(!step_found, s)?;
                step_found = true;
                require(same(field(step, "rounded", s)?, amount, s)?, s)?;
                require(same(field(step, "basis_refs", s)?, bases, s)?, s)?;
                require(
                    same(
                        field(step, "component_slot", s)?,
                        field(a, "component_slot", s)?,
                        s,
                    )?,
                    s,
                )?;
                let disposition = if role == "inverse" {
                    "inverse"
                } else {
                    "posted"
                };
                require(same_text(text(step, "disposition", s)?, disposition, s)?, s)?;
            }
        }
    }
    require(step_found, s)?;
    let roles = field(a, "role_ref", s)?;
    let terms = field(a, "slot_terms_ref", s)?;
    reference(roles, "workflow-roles", s)?;
    reference(terms, "workflow-slot-terms", s)?;
    member_once(field(b, "roles_refs", s)?, roles, s)?;
    let mut terms_found = false;
    for binding in field(b, "bindings", s)?.as_array().ok_or_else(failure)? {
        s.work().visit(Visit::Element)?;
        if same(field(binding, "binding_ref", s)?, terms, s)? {
            require(!terms_found, s)?;
            terms_found = true;
        }
    }
    require(terms_found, s)?;
    let mut funding_found = false;
    for leg in field(b, "funding_legs", s)?
        .as_array()
        .ok_or_else(failure)?
    {
        s.work().visit(Visit::Element)?;
        if same(field(leg, "name", s)?, field(a, "funding_leg", s)?, s)? {
            require(!funding_found, s)?;
            funding_found = true;
        }
    }
    require(funding_found, s)?;
    let fields = [
        ("source_record_ref", input.action.reference),
        ("decision_ref", input.decision.reference),
        ("evaluation_ref", input.evaluation.reference),
        ("producer_receipt_ref", input.producer_receipt_ref),
        ("binding_ref", input.binding.reference),
        ("agreement_id", field(b, "agreement_id", s)?),
        ("agreement_version", field(b, "version", s)?),
        ("book", field(a, "book", s)?),
        ("role_ref", roles),
        ("slot_terms_ref", terms),
        ("funding_leg", field(a, "funding_leg", s)?),
        ("component_slot", field(a, "component_slot", s)?),
        ("root_component_slot", field(a, "root_component_slot", s)?),
        ("posting_role", field(a, "posting_role", s)?),
        ("amount", amount),
        ("basis_refs", bases),
        ("inverse_of", inverse.unwrap_or(&Value::Null)),
    ];
    let len = if inverse.is_some() {
        fields.len()
    } else {
        fields.len() - 1
    };
    // Fixed stack field controls, with all retained payload copies paid by arena.
    let arena = s.arena().shared_handle();
    arena.object_from_fields(&fields[..len], s.work())
}

const LINE_FIELDS: [&str; 16] = [
    "source_record_ref",
    "decision_ref",
    "evaluation_ref",
    "producer_receipt_ref",
    "binding_ref",
    "agreement_id",
    "agreement_version",
    "book",
    "role_ref",
    "slot_terms_ref",
    "funding_leg",
    "component_slot",
    "root_component_slot",
    "posting_role",
    "amount",
    "basis_refs",
];

/// New workflow-domain ID only; the legacy M5 line identity is unchanged.
pub fn line(
    view_kind: &str,
    view_identity: &Value,
    workflow_scope: &Value,
    data: &Value,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    match view_kind {
        "standard" => closed(view_identity, &["customer", "period_id"], s)?,
        "ad_hoc" => closed(view_identity, &["customer", "command_id"], s)?,
        _ => return Err(failure()),
    }
    closed(workflow_scope, &["store", "scope"], s)?;
    let inverse = data.as_object().ok_or_else(failure)?.get("inverse_of");
    require(
        data.as_object().ok_or_else(failure)?.len()
            == LINE_FIELDS.len() + usize::from(inverse.is_some()),
        s,
    )?;
    for name in LINE_FIELDS {
        field(data, name, s)?;
    }
    require(
        inverse.is_some() == (text(data, "posting_role", s)? == "inverse"),
        s,
    )?;
    let arena = s.arena().shared_handle();
    let kind = arena.string_value(view_kind, s.work())?;
    let identity = arena.object_from_fields(
        &[
            ("view_kind", kind.value()),
            ("view_identity", view_identity),
            ("workflow_scope", workflow_scope),
            ("line", data),
        ],
        s.work(),
    )?;
    let encoded = s.canonical_representation(identity.value(), RepresentationKind::Record)?;
    const DOMAIN: &[u8] = b"bean-counter/workflow/1/statement-line\0";
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
    let mut fields = [("", &Value::Null); 18];
    for (i, name) in LINE_FIELDS.iter().enumerate() {
        fields[i] = (name, field(data, name, s)?);
    }
    fields[16] = ("line_id", id.value());
    if let Some(inverse) = inverse {
        fields[17] = ("inverse_of", inverse);
    }
    arena.object_from_fields(&fields[..17 + usize::from(inverse.is_some())], s.work())
}

struct Group {
    first: usize,
    legacy: bool,
    key: AuditedBytes,
    total: BigInt,
}

/// Totals of already certified line DATA. Workflow obligations and legacy
/// payer/recipient obligations remain disjoint closed variants. This helper
/// never certifies a legacy calculation or an original financial producer.
pub fn totals(lines: &[Value], s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    totals_impl(lines, false, s)
}
/// Additive statement/6 totals. Only this closed profile admits manual credits;
/// old statement/5 callers retain their original accepted language.
pub fn totals_with_manual(lines: &[Value], s: &mut OriginalAuditSession) -> Result<AuditedValue> {
    totals_impl(lines, true, s)
}
fn totals_impl(
    lines: &[Value],
    allow_manual: bool,
    s: &mut OriginalAuditSession,
) -> Result<AuditedValue> {
    let arena = s.arena().shared_handle();
    // 64 decimal input bytes plus usize::BITS is a conservative decimal sum
    // bound for any addressable slice. Cover group integers and parse/add/render
    // overlap before constructing them; output Values own separate arena leases.
    let digits = 64 + usize::BITS as usize;
    let per = std::mem::size_of::<Group>()
        + std::mem::size_of::<AuditedValue>()
        + std::mem::size_of::<&Value>()
        + 4 * (digits + 8);
    s.work()
        .arithmetic(Op::Multiply, &[&lines.len().into(), &per.into()])?;
    let bytes = lines.len().checked_mul(per).ok_or_else(failure)?;
    let _controls = s.reserve_external_payload(bytes)?;
    s.work().write_bytes(bytes)?;
    let mut groups: Vec<Group> = Vec::new();
    groups
        .try_reserve_exact(lines.len())
        .map_err(|_| failure())?;
    require(groups.capacity() <= lines.len(), s)?;
    for (i, line) in lines.iter().enumerate() {
        s.work().visit(Visit::Element)?;
        let workflow_source = optional_field(line, "source_record_ref", s)?;
        let legacy_sources = optional_field(line, "source_records", s)?;
        let manual_source = if allow_manual {
            optional_field(line, "manual_credit_ref", s)?
        } else {
            None
        };
        require(
            (workflow_source.is_some() != legacy_sources.is_some()) != manual_source.is_some()
                && !(workflow_source.is_some()
                    && legacy_sources.is_some()
                    && manual_source.is_some()),
            s,
        )?;
        let legacy = legacy_sources.is_some();
        let (key, n) = if let Some(source) = workflow_source.or(manual_source) {
            if manual_source.is_some() {
                manual_credit::validate_line(line, s)?;
            } else {
                reference(source, "workflow-action", s)?;
            }
            for prior in &lines[..i] {
                s.work().visit(Visit::Element)?;
                let name = if manual_source.is_some() {
                    "manual_credit_ref"
                } else {
                    "source_record_ref"
                };
                if let Some(prior) = optional_field(prior, name, s)? {
                    require(!same(source, prior, s)?, s)?;
                }
            }
            let key = arena.object_from_fields(
                &[
                    ("book", field(line, "book", s)?),
                    ("role_ref", field(line, "role_ref", s)?),
                    ("slot_terms_ref", field(line, "slot_terms_ref", s)?),
                    ("funding_leg", field(line, "funding_leg", s)?),
                ],
                s.work(),
            )?;
            (key, number(atoms(field(line, "amount", s)?, s)?, s)?)
        } else {
            closed(
                line,
                &[
                    "agreement_id",
                    "agreement_version",
                    "amount_atoms",
                    "basis",
                    "calculation",
                    "currency",
                    "line_id",
                    "payer",
                    "recipient",
                    "scale",
                    "source_records",
                ],
                s,
            )?;
            require(
                legacy_sources
                    .and_then(Value::as_array)
                    .is_some_and(|v| !v.is_empty()),
                s,
            )?;
            require(
                !text(line, "payer", s)?.is_empty() && !text(line, "recipient", s)?.is_empty(),
                s,
            )?;
            require(same_text(text(line, "currency", s)?, "USD", s)?, s)?;
            require(field(line, "scale", s)?.as_u64() == Some(18), s)?;
            let key = arena.object_from_fields(
                &[
                    ("payer", field(line, "payer", s)?),
                    ("recipient", field(line, "recipient", s)?),
                ],
                s.work(),
            )?;
            (
                key,
                number(signed_atoms(text(line, "amount_atoms", s)?, s)?, s)?,
            )
        };
        let key = s.canonical_representation(key.value(), RepresentationKind::Record)?;
        let mut found = None;
        for (j, group) in groups.iter().enumerate() {
            s.work().visit(Visit::MemberCompare)?;
            s.work().scan_bytes(group.key.bytes().len())?;
            s.work().scan_bytes(key.bytes().len())?;
            if group.key.bytes() == key.bytes() {
                found = Some(j);
                break;
            }
        }
        if let Some(j) = found {
            s.work().arithmetic(Op::Add, &[&groups[j].total, &n])?;
            groups[j].total += n;
        } else {
            groups.push(Group {
                first: i,
                legacy,
                key,
                total: n,
            });
        }
    }
    work::try_sort_by(&mut groups, s.work(), |a, b, w| {
        w.visit(Visit::MemberCompare)?;
        w.scan_bytes(a.key.bytes().len())?;
        w.scan_bytes(b.key.bytes().len())?;
        Ok(a.key.bytes().cmp(b.key.bytes()))
    })?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(groups.len())
        .map_err(|_| failure())?;
    require(result.capacity() <= lines.len(), s)?;
    for group in &groups {
        s.work().visit(Visit::Element)?;
        let source = &lines[group.first];
        s.work()
            .arithmetic(Op::Compare, &[&group.total, &BigInt::zero()])?;
        let direction = match group.total.cmp(&BigInt::zero()) {
            std::cmp::Ordering::Less => "payable",
            std::cmp::Ordering::Equal => "none",
            std::cmp::Ordering::Greater => "receivable",
        };
        // Render the exact charged divisions themselves, without a second
        // unmetered formatter. The fixed stack buffer covers the proven bound.
        let mut rendering = group.total.clone();
        s.work().arithmetic(Op::Absolute, &[&rendering])?;
        let negative = rendering < BigInt::zero();
        if negative {
            rendering = -rendering;
        }
        let ten = BigInt::from(10);
        let mut decimal = [0u8; 64 + usize::BITS as usize];
        let mut cursor = decimal.len();
        loop {
            s.work().visit(Visit::ScalarDigit)?;
            s.work()
                .arithmetic(Op::DivideRemainder, &[&rendering, &ten])?;
            let (quotient, remainder) = rendering.div_rem(&ten);
            cursor = cursor.checked_sub(1).ok_or_else(failure)?;
            s.work().write_bytes(1)?;
            decimal[cursor] = b'0' + remainder.to_u8().ok_or_else(failure)?;
            rendering = quotient;
            if rendering.is_zero() {
                break;
            }
        }
        if negative {
            cursor = cursor.checked_sub(1).ok_or_else(failure)?;
            s.work().write_bytes(1)?;
            decimal[cursor] = b'-';
        }
        let atoms = arena.string_value(
            std::str::from_utf8(&decimal[cursor..]).map_err(|_| failure())?,
            s.work(),
        )?;
        let template = if group.legacy {
            source
        } else {
            field(source, "amount", s)?
        };
        let amount = arena.object_from_fields(
            &[
                ("atoms", atoms.value()),
                ("currency", field(template, "currency", s)?),
                ("scale", field(template, "scale", s)?),
            ],
            s.work(),
        )?;
        let direction = arena.string_value(direction, s.work())?;
        let row = if group.legacy {
            arena.object_from_fields(
                &[
                    ("payer", field(source, "payer", s)?),
                    ("recipient", field(source, "recipient", s)?),
                    ("amount", amount.value()),
                    ("direction", direction.value()),
                ],
                s.work(),
            )?
        } else {
            arena.object_from_fields(
                &[
                    ("book", field(source, "book", s)?),
                    ("role_ref", field(source, "role_ref", s)?),
                    ("slot_terms_ref", field(source, "slot_terms_ref", s)?),
                    ("funding_leg", field(source, "funding_leg", s)?),
                    ("amount", amount.value()),
                    ("direction", direction.value()),
                ],
                s.work(),
            )?
        };
        result.push(row);
    }
    let mut values = Vec::new();
    values
        .try_reserve_exact(result.len())
        .map_err(|_| failure())?;
    require(values.capacity() <= lines.len(), s)?;
    for row in &result {
        s.work().visit(Visit::Element)?;
        values.push(row.value());
    }
    arena.array_from_values(&values, s.work())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::audit_arena::AuditArena;
    use serde_json::json;

    fn r(kind: &str, digit: char) -> Value {
        let prefix = match kind {
            "workflow-action" => "waction_",
            "workflow-decision" => "wdrev_",
            "workflow-receipt" => "wreceipt_",
            _ => "wdoc_",
        };
        let hash = digit.to_string().repeat(64);
        json!({"kind":kind,"id":format!("{prefix}{hash}"),"digest":format!("sha256:{hash}")})
    }
    // These are join fixtures, not a fake completed-producer certificate.
    fn fixture(digit: char, atoms: &str, posting: &str) -> Value {
        let action = r("workflow-action", digit);
        let binding = r("workflow-binding", 'a');
        let evaluation = r("workflow-evaluation", 'b');
        let roles = r("workflow-roles", 'c');
        let terms = r("workflow-slot-terms", 'd');
        let key =
            json!({"store":"store","scope":"scope","source":"source","operation_id":"operation"});
        let amount = json!({"currency":"USD","scale":18,"atoms":atoms});
        let mut body = json!({"schema":"ledger-workflow-action/1","decision_id":"decision","revision":"1",
            "operation_key":key,"posting_role":posting,"book":"retail","component_slot":"service",
            "root_component_slot":"service","role_ref":roles,"slot_terms_ref":terms,"funding_leg":"customer",
            "amount":amount,"basis_refs":[]});
        if posting == "inverse" {
            body["inverse_of"] = r("workflow-action", 'f');
        }
        json!({"action_ref":action,"action":body,"decision_ref":r("workflow-decision",'e'),
            "decision":{"schema":"ledger-workflow-decision-revision/3","decision_id":"decision","revision":"1",
                "operation_key":key,"evaluation_ref":evaluation,"original_terms_ref":binding,"actions_appended":[action]},
            "evaluation_ref":evaluation,"evaluation":{"schema":"ledger-workflow-evaluation-proof/1","binding_ref":binding,
                "steps":[{"action_ref":action,"rounded":amount,"basis_refs":[],"component_slot":"service",
                    "disposition":if posting=="inverse" {"inverse"} else {"posted"}}]},
            "binding_ref":binding,"binding":{"schema":"ledger-workflow-binding/2","agreement_id":"agreement","version":"1",
                "customer":"customer","roles_refs":[roles],"bindings":[{"binding_ref":terms,"slot":"retail"}],
                "funding_legs":[{"name":"customer"}]},"receipt":r("workflow-receipt",'f'),"key":key})
    }
    fn project(v: &Value, s: &mut OriginalAuditSession) -> Result<AuditedValue> {
        project_action(
            WorkflowActionData {
                action: WorkflowRecordData {
                    reference: &v["action_ref"],
                    body: &v["action"],
                },
                decision: WorkflowRecordData {
                    reference: &v["decision_ref"],
                    body: &v["decision"],
                },
                evaluation: WorkflowRecordData {
                    reference: &v["evaluation_ref"],
                    body: &v["evaluation"],
                },
                binding: WorkflowRecordData {
                    reference: &v["binding_ref"],
                    body: &v["binding"],
                },
                producer_receipt_ref: &v["receipt"],
                producer_key: &v["key"],
            },
            s,
        )
    }
    #[test]
    fn correction_keeps_five_actions_and_exact_signed_total() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let rows = [
            ('1', "1000000000000000000", "original"),
            ('2', "2000000000000000000", "original"),
            ('3', "250000000000000000", "original"),
            ('4', "-2000000000000000000", "inverse"),
            ('5', "1600000000000000000", "replacement"),
        ]
        .into_iter()
        .map(|(id, n, p)| {
            project(&fixture(id, n, p), &mut session)
                .unwrap()
                .value()
                .clone()
        })
        .collect::<Vec<_>>();
        let result = totals(&rows, &mut session).unwrap();
        assert_eq!(result.value().as_array().unwrap().len(), 1);
        assert_eq!(result.value()[0]["amount"]["atoms"], "2850000000000000000");
        assert_eq!(result.value()[0]["direction"], "receivable");
        assert_eq!(rows[3]["inverse_of"], r("workflow-action", 'f'));
        assert!(rows[4].get("inverse_of").is_none());
    }
    #[test]
    fn projection_requires_actual_appended_and_evaluated_action() {
        for mutation in 0..5 {
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut value = fixture('1', "100", "original");
            match mutation {
                0 => value["decision"]["actions_appended"] = json!([]),
                1 => value["evaluation"]["steps"][0]["rounded"]["atoms"] = json!("101"),
                2 => value["decision"]["operation_key"]["scope"] = json!("other"),
                3 => value["binding"]["roles_refs"] = json!([r("workflow-roles", '0')]),
                _ => value["action"]["inverse_of"] = r("workflow-action", 'f'),
            }
            assert!(
                project(&value, &mut session).is_err(),
                "mutation {mutation}"
            );
        }
    }
    #[test]
    fn totals_preserve_obligations_and_refuse_duplicate_membership() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let first = project(&fixture('1', "10", "original"), &mut session).unwrap();
        let mut second = project(&fixture('2', "-10", "inverse"), &mut session)
            .unwrap()
            .value()
            .clone();
        second["book"] = json!("supplier");
        let rows = vec![first.value().clone(), second];
        let result = totals(&rows, &mut session).unwrap();
        assert_eq!(result.value().as_array().unwrap().len(), 2);
        assert_eq!(result.value()[0]["book"], "retail");
        assert_eq!(result.value()[0]["direction"], "receivable");
        assert_eq!(result.value()[1]["direction"], "payable");
        assert!(totals(&[rows[0].clone(), rows[0].clone()], &mut session).is_err());
    }
    #[test]
    fn mixed_totals_keep_legacy_counterparties_separate_and_reject_ambiguous_lines() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let workflow = project(&fixture('1', "10", "original"), &mut session).unwrap();
        let original = totals(&[workflow.value().clone()], &mut session).unwrap();
        let legacy = |id: &str, recipient: &str, atoms: &str| {
            json!({
                "line_id":id,"source_records":[{"customer":"payer","source":"source","kind":"receipt","id":id}],
                "agreement_id":"legacy-agreement","agreement_version":"1","basis":"fixed",
                "calculation":{"kind":"fixed"},"payer":"payer","recipient":recipient,
                "currency":"USD","scale":18,"amount_atoms":atoms
            })
        };
        let first = legacy("one", "recipient", "50");
        let second = legacy("two", "recipient", "-20");
        let third = legacy("three", "other-recipient", "-10");
        let rows = [first.clone(), workflow.value().clone(), second, third];
        let result = totals(&rows, &mut session).unwrap();
        assert_eq!(result.value().as_array().unwrap().len(), 3);
        assert_eq!(result.value()[0], original.value()[0]);
        assert_eq!(
            result.value()[1],
            json!({"payer":"payer","recipient":"other-recipient",
            "amount":{"atoms":"-10","currency":"USD","scale":18},"direction":"payable"})
        );
        assert_eq!(
            result.value()[2],
            json!({"payer":"payer","recipient":"recipient",
            "amount":{"atoms":"30","currency":"USD","scale":18},"direction":"receivable"})
        );
        for mutation in 0..4 {
            let mut bad = first.clone();
            match mutation {
                0 => bad["source_record_ref"] = r("workflow-action", '2'),
                1 => bad["amount_atoms"] = json!("-0"),
                2 => bad["currency"] = json!("EUR"),
                _ => bad["extra"] = json!(true),
            }
            assert!(totals(&[bad], &mut session).is_err(), "mutation {mutation}");
        }
    }
    #[test]
    fn workflow_line_identity_has_exact_scope_and_view_framing() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let data = project(&fixture('1', "100", "original"), &mut session).unwrap();
        let scope = json!({"store":"store","scope":"scope"});
        let view = json!({"customer":"customer","period_id":"period"});
        let line1 = line("standard", &view, &scope, data.value(), &mut session).unwrap();
        // Independently calculated from the approved domain and ASCII JCS
        // fixture, rather than recomputing the implementation inside this test.
        assert_eq!(
            line1.value()["line_id"],
            "86b19d5bf3b3ee3235d48ec9e76b30aa0aa22532c85ef57e364831c9a139c9de"
        );
        let line2 = line(
            "standard",
            &view,
            &json!({"store":"store","scope":"other"}),
            data.value(),
            &mut session,
        )
        .unwrap();
        assert_ne!(line1.value()["line_id"], line2.value()["line_id"]);
        assert_eq!(
            line1.value()["source_record_ref"],
            data.value()["source_record_ref"]
        );
        assert!(line(
            "ad_hoc",
            &json!({"customer":"customer","statement_id":"old-field"}),
            &scope,
            data.value(),
            &mut session
        )
        .is_err());
        assert!(line(
            "ad_hoc",
            &json!({"customer":"customer","command_id":"command"}),
            &scope,
            data.value(),
            &mut session
        )
        .is_ok());
    }

    #[test]
    fn exact_zero_total_keeps_both_source_lines() {
        let arena = AuditArena::new().unwrap();
        let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
        let first = project(&fixture('1', "10", "original"), &mut session).unwrap();
        let second = project(&fixture('2', "-10", "inverse"), &mut session).unwrap();
        let rows = [first.value().clone(), second.value().clone()];
        let result = totals(&rows, &mut session).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(result.value()[0]["amount"]["atoms"], "0");
        assert_eq!(result.value()[0]["direction"], "none");
    }
}
