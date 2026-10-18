//! Actual owning setup decoder for resource-accounted local management.
//! Uses the SAME derived Setup dictionary with raw Scope/time fields; no
//! current authority, receipt or admission follows from these normalized DATA.
use super::*;
use crate::service::workflow::core_error;
use ledgerlab_core::{
    canonical::audit_arena::ExternalBufferLease,
    validation_work::{
        ArithmeticOperation as Op, OriginalAuditSession, RepresentationKind,
        ValidationVisit as Visit,
    },
};
use std::mem;
mod permissions;
pub(crate) use permissions::{OwnedPermissionChange, PermissionRecipe, parse_permission};

pub(crate) struct OwnedSetup {
    setup: Setup,
    _storage: ExternalBufferLease,
    _permissions_storage: Option<ExternalBufferLease>,
}
impl OwnedSetup {
    pub(crate) fn setup(&self) -> &Setup {
        &self.setup
    }
}
fn arithmetic(a: usize, b: usize, op: Op, s: &mut OriginalAuditSession) -> Result<usize> {
    s.semantic_work()
        .arithmetic(op, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    match op {
        Op::Add => a.checked_add(b),
        Op::Multiply => a.checked_mul(b),
        _ => None,
    }
    .ok_or(crate::ServiceError::Unavailable)
}
fn fail(code: &str, s: &mut OriginalAuditSession) -> crate::ServiceError {
    if let Err(error) = s
        .semantic_work()
        .scan_bytes(code.len())
        .and_then(|_| s.semantic_work().write_bytes(code.len()))
    {
        return core_error(error);
    }
    reject(code)
}
fn ensure(s: &mut OriginalAuditSession, code: &str, check: impl FnOnce() -> bool) -> Result<()> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if check() { Ok(()) } else { Err(fail(code, s)) }
}
fn compare(
    a: impl Into<ledgerlab_core::validation_work::BigInt>,
    b: impl Into<ledgerlab_core::validation_work::BigInt>,
    s: &mut OriginalAuditSession,
) -> Result<std::cmp::Ordering> {
    let (a, b) = (a.into(), b.into());
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a, &b])
        .map_err(core_error)?;
    Ok(a.cmp(&b))
}
fn equal(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn mapped(
    error: ledgerlab_core::Error,
    code: &str,
    s: &mut OriginalAuditSession,
) -> crate::ServiceError {
    match error.code {
        "IDENTIFIER"
        | "TIMESTAMP"
        | "INPUT_PRECISION"
        | "ARITHMETIC_OVERFLOW"
        | "CURRENCY_MISMATCH" => fail(code, s),
        _ => core_error(error),
    }
}
fn member<'a>(v: &'a Value, key: &str, s: &mut OriginalAuditSession) -> Result<Option<&'a Value>> {
    ensure(s, "BILLING_SETUP", || v.is_object())?;
    for (candidate, value) in v.as_object().unwrap() {
        s.semantic_work()
            .visit(Visit::RefLookup)
            .map_err(core_error)?;
        if equal(candidate, key, s)? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}
fn decode_work(v: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    const FIELDS: &[&str] = &[
        "schema",
        "scope",
        "store_id",
        "operator",
        "source",
        "customer",
        "host",
        "agreement",
        "binding",
        "price",
        "unit",
        "maximum_quantity",
        "accepted_at",
        "acceptor",
        "assent_evidence",
        "operator_attestation",
        "finality_attestation",
        "permissions",
        "outcome_policy",
    ];
    ensure(s, "BILLING_SETUP", || v.is_object())?;
    for (key, value) in v.as_object().unwrap() {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let mut known = false;
        for expected in FIELDS {
            if equal(key, expected, s)? {
                known = true;
                break;
            }
        }
        ensure(s, "BILLING_SETUP", || known)?;
        if equal(key, "scope", s)? {
            ensure(s, "BILLING_SETUP", || value.is_array())?;
            let rows = value.as_array().unwrap();
            let length = compare(rows.len(), 2usize, s)?;
            ensure(s, "BILLING_SETUP", || length.is_eq())?;
            for row in rows {
                ensure(s, "BILLING_SETUP", || row.is_string())?;
            }
        } else if equal(key, "permissions", s)? {
            ensure(s, "BILLING_SETUP", || value.is_array())?;
            for row in value.as_array().unwrap() {
                ensure(s, "BILLING_SETUP", || row.is_string())?;
            }
        } else if equal(key, "outcome_policy", s)? {
            // Value is the exact dictionary's arbitrary JSON field; its private
            // typed copy is covered by the actual clone-footprint bound.
        } else if equal(key, "unit", s)? || equal(key, "maximum_quantity", s)? {
            ensure(s, "BILLING_SETUP", || value.is_null() || value.is_string())?;
        } else {
            ensure(s, "BILLING_SETUP", || value.is_string())?;
        }
    }
    Ok(())
}
/// Caller supplies the actual retained setup bytes under its held read. The
/// raw parse, typed fields, outcome Value copy, Scope/time copies and malformed
/// serde error prefix remain owned until this result is dropped.
pub(crate) fn parse(raw: &[u8], s: &mut OriginalAuditSession) -> Result<OwnedSetup> {
    let length = compare(raw.len(), 65536usize, s)?;
    ensure(s, "BILLING_SETUP", || !length.is_gt())?;
    let parsed = s
        .parse_representation(raw, RepresentationKind::Record)
        .map_err(core_error)?;
    let (arena, work) = s.parts();
    let copy = arena
        .value_copy_payload_bound(parsed.value(), work)
        .map_err(core_error)?;
    let mut payload = arithmetic(
        copy,
        mem::size_of::<OwnedSetup>() + mem::size_of::<Setup<[String; 2], String>>(),
        Op::Add,
        s,
    )?;
    // Scope is converted from two raw Strings, timestamp renders <=27 bytes,
    // and the fixed decimal/money temporaries remain live only in validation.
    let scope = member(parsed.value(), "scope", s)?;
    if let Some(Value::Array(values)) = scope {
        for value in values {
            if let Some(text) = value.as_str() {
                payload = arithmetic(payload, text.len(), Op::Add, s)?;
            }
        }
    }
    payload = arithmetic(
        payload,
        27 + 64
            + 3
            + 128
            + mem::size_of::<String>()
            + mem::size_of::<Scope>()
            + mem::size_of::<Timestamp>()
            + 2 * mem::size_of::<Decimal>()
            + 2 * mem::size_of::<Money>()
            + mem::size_of::<[Option<i64>; 4]>(),
        Op::Add,
        s,
    )?;
    // SAME pinned derived decoder's error can escape at most6 bytes/input byte;
    // growth/shrink/ErrorImpl overlap has three simultaneous allocations.
    let escaped = arithmetic(raw.len(), 6, Op::Multiply, s)?;
    const EXPECTED: &str = stringify!(schema scope store_id operator source customer host agreement binding price unit maximum_quantity accepted_at acceptor assent_evidence operator_attestation finality_attestation permissions outcome_policy String array object null boolean integer);
    let expected = arithmetic(EXPECTED.len(), 2, Op::Multiply, s)?;
    let overhead = arithmetic(expected, 256, Op::Add, s)?;
    let error = arithmetic(escaped, overhead, Op::Add, s)?;
    let errors = arithmetic(error, 3, Op::Multiply, s)?;
    payload = arithmetic(payload, errors, Op::Add, s)?;
    let storage = s.reserve_external_payload(payload).map_err(core_error)?;
    s.semantic_work().scan_bytes(payload).map_err(core_error)?;
    s.semantic_work().write_bytes(payload).map_err(core_error)?;
    decode_work(parsed.value(), s)?;
    let raw: Setup<[String; 2], String> =
        Setup::deserialize(parsed.value()).map_err(|_| fail("BILLING_SETUP", s))?;
    let scope = Scope::new_with_work(&raw.scope[0], &raw.scope[1], s.semantic_work())
        .map_err(|e| mapped(e, "BILLING_SETUP", s))?;
    let accepted_at = Timestamp::parse_with_work(&raw.accepted_at, s.semantic_work())
        .map_err(|e| mapped(e, "BILLING_SETUP", s))?;
    let setup = Setup {
        grant_revision: 1,
        schema: raw.schema,
        scope,
        store_id: raw.store_id,
        operator: raw.operator,
        source: raw.source,
        customer: raw.customer,
        host: raw.host,
        agreement: raw.agreement,
        binding: raw.binding,
        price: raw.price,
        unit: raw.unit,
        maximum_quantity: raw.maximum_quantity,
        accepted_at,
        acceptor: raw.acceptor,
        assent_evidence: raw.assent_evidence,
        operator_attestation: raw.operator_attestation,
        finality_attestation: raw.finality_attestation,
        permissions: raw.permissions,
        outcome_policy: raw.outcome_policy,
    };
    validate(&setup, s)?;
    Ok(OwnedSetup {
        setup,
        _storage: storage,
        _permissions_storage: None,
    })
}
fn identifier(v: &str, maximum: usize, s: &mut OriginalAuditSession) -> Result<()> {
    let length = compare(v.len(), maximum, s)?;
    ensure(s, "BILLING_IDENTIFIER", || !v.is_empty() && !length.is_gt())?;
    s.semantic_work().scan_bytes(v.len()).map_err(core_error)?;
    for ch in v.chars() {
        ensure(s, "BILLING_IDENTIFIER", || !ch.is_control())?;
    }
    Ok(())
}
fn validate(row: &Setup, s: &mut OriginalAuditSession) -> Result<()> {
    let one = equal(&row.schema, "ledger-local-billing/1", s)?;
    let two = if one {
        false
    } else {
        equal(&row.schema, "ledger-local-billing/2", s)?
    };
    ensure(s, "BILLING_SETUP", || one || two)?;
    if two {
        ensure(s, "BILLING_SETUP", || {
            row.unit.is_some() && row.maximum_quantity.is_some()
        })?;
        let unit = row.unit.as_deref().unwrap();
        let length = compare(unit.len(), 128usize, s)?;
        ensure(s, "BILLING_SETUP", || !unit.is_empty() && !length.is_gt())?;
        s.semantic_work()
            .scan_bytes(unit.len())
            .map_err(core_error)?;
        for byte in unit.bytes() {
            ensure(s, "BILLING_SETUP", || {
                byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
            })?;
        }
    } else {
        ensure(s, "BILLING_SETUP", || {
            row.unit.is_none() && row.maximum_quantity.is_none()
        })?;
    }
    for value in [
        &row.store_id,
        &row.operator,
        &row.customer,
        &row.host,
        &row.agreement,
        &row.binding,
        &row.acceptor,
    ] {
        identifier(value, 128, s)?;
    }
    identifier(&row.source, 256, s)?;
    let same = equal(&row.customer, &row.host, s)?;
    ensure(s, "BILLING_PARTIES", || !same)?;
    for value in [
        &row.assent_evidence,
        &row.operator_attestation,
        &row.finality_attestation,
    ] {
        s.semantic_work()
            .scan_bytes(value.len())
            .map_err(core_error)?;
        let length = compare(value.len(), 8192usize, s)?;
        ensure(s, "BILLING_ASSENT_REQUIRED", || {
            !value.trim().is_empty() && !length.is_gt()
        })?;
    }
    let price = Decimal::parse_with_work(&row.price, s.semantic_work())
        .map_err(|e| mapped(e, "BILLING_PRICE", s))?;
    let zero = price
        .is_zero_with_work(s.semantic_work())
        .map_err(core_error)?;
    ensure(s, "BILLING_PRICE", || !zero)?;
    if two {
        let maximum =
            Decimal::parse_with_work(row.maximum_quantity.as_deref().unwrap(), s.semantic_work())
                .map_err(|e| mapped(e, "BILLING_QUANTITY", s))?;
        let zero = maximum
            .is_zero_with_work(s.semantic_work())
            .map_err(core_error)?;
        ensure(s, "BILLING_QUANTITY", || !zero)?;
        let maximum = maximum
            .atoms_exact_with_work(0, s.semantic_work())
            .map_err(|e| mapped(e, "BILLING_QUANTITY", s))?;
        let price = price
            .atoms_exact_with_work(18, s.semantic_work())
            .map_err(|e| mapped(e, "BILLING_PRICE", s))?;
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&price.into(), &maximum.into()])
            .map_err(core_error)?;
        let atoms = price
            .checked_mul(maximum)
            .ok_or_else(|| fail("BILLING_PRICE", s))?;
        Money::new_with_work("USD", 18, atoms, s.semantic_work())
            .map_err(|e| mapped(e, "BILLING_PRICE", s))?;
    }
    ensure(s, "BILLING_POLICY", || row.outcome_policy.is_object())?;
    let document = member(&row.outcome_policy, "document", s)?;
    ensure(s, "BILLING_POLICY", || document.is_none())?;
    let length = compare(row.permissions.len(), 3usize, s)?;
    ensure(s, "BILLING_PERMISSIONS", || !length.is_gt())?;
    for (index, permission) in row.permissions.iter().enumerate() {
        let mut known = false;
        for allowed in ["read", "submit", "correct"] {
            if equal(permission, allowed, s)? {
                known = true;
                break;
            }
        }
        ensure(s, "BILLING_PERMISSIONS", || known)?;
        for earlier in &row.permissions[..index] {
            let duplicate = equal(permission, earlier, s)?;
            ensure(s, "BILLING_PERMISSIONS", || !duplicate)?;
        }
    }
    policy(row, s)?;
    Ok(())
}
fn policy_field<'a>(
    value: &'a Value,
    name: &str,
    s: &mut OriginalAuditSession,
) -> Result<&'a Value> {
    ensure(s, "BILLING_POLICY", || value.is_object())?;
    member(value, name, s)?.ok_or_else(|| fail("BILLING_POLICY", s))
}
fn keys(value: &Value, names: &[&str], s: &mut OriginalAuditSession) -> Result<()> {
    ensure(s, "BILLING_POLICY", || value.is_object())?;
    let count = compare(value.as_object().unwrap().len(), names.len(), s)?;
    ensure(s, "BILLING_POLICY", || count.is_eq())?;
    for name in names {
        policy_field(value, name, s)?;
    }
    Ok(())
}
fn policy_text<'a>(value: &'a Value, s: &mut OriginalAuditSession) -> Result<&'a str> {
    ensure(s, "BILLING_POLICY", || value.is_string())?;
    Ok(value.as_str().unwrap())
}
fn slug(value: &Value, s: &mut OriginalAuditSession) -> Result<()> {
    let text = policy_text(value, s)?;
    let length = compare(text.len(), 128usize, s)?;
    ensure(s, "BILLING_POLICY", || !text.is_empty() && !length.is_gt())?;
    s.semantic_work()
        .scan_bytes(text.len())
        .map_err(core_error)?;
    for byte in text.bytes() {
        ensure(s, "BILLING_POLICY", || {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })?;
    }
    Ok(())
}
fn policy_money(value: &Value, scale: u8, s: &mut OriginalAuditSession) -> Result<Money> {
    keys(value, &["currency", "scale", "atoms"], s)?;
    let currency = policy_text(policy_field(value, "currency", s)?, s)?;
    let same = equal(currency, "USD", s)?;
    ensure(s, "BILLING_POLICY", || same)?;
    let raw_scale = policy_field(value, "scale", s)?;
    ensure(s, "BILLING_POLICY", || raw_scale.as_u64().is_some())?;
    let same = compare(raw_scale.as_u64().unwrap(), scale, s)?;
    ensure(s, "BILLING_POLICY", || same.is_eq())?;
    let atoms = policy_text(policy_field(value, "atoms", s)?, s)?;
    let atoms = ledgerlab_core::money::parse_atoms_with_work(atoms, s.semantic_work())
        .map_err(|e| mapped(e, "BILLING_POLICY", s))?;
    Money::new_with_work(currency, scale, atoms, s.semantic_work())
        .map_err(|e| mapped(e, "BILLING_POLICY", s))
}
fn policy(row: &Setup, s: &mut OriginalAuditSession) -> Result<()> {
    let p = &row.outcome_policy;
    keys(p, &["version", "families", "limits"], s)?;
    let version = policy_text(policy_field(p, "version", s)?, s)?;
    let length = compare(version.len(), 128usize, s)?;
    ensure(s, "BILLING_POLICY", || {
        !version.is_empty() && !length.is_gt()
    })?;
    let families = policy_field(p, "families", s)?;
    let limits = policy_field(p, "limits", s)?;
    ensure(s, "BILLING_POLICY", || {
        families.is_array() && limits.is_array()
    })?;
    let (families, limits) = (families.as_array().unwrap(), limits.as_array().unwrap());
    let fcount = compare(families.len(), 1usize, s)?;
    let lcount = compare(limits.len(), 1usize, s)?;
    ensure(s, "BILLING_POLICY", || fcount.is_eq() && lcount.is_eq())?;
    let f = &families[0];
    keys(
        f,
        &[
            "family",
            "binding_id",
            "source",
            "correction_source",
            "evidence_required",
            "ordinary",
            "corrections",
            "codes",
            "replacement_codes",
            "allow_reversal",
        ],
        s,
    )?;
    slug(policy_field(f, "family", s)?, s)?;
    for (field, expected) in [
        ("binding_id", row.binding.as_str()),
        ("source", row.source.as_str()),
        ("correction_source", row.source.as_str()),
    ] {
        let same = equal(policy_text(policy_field(f, field, s)?, s)?, expected, s)?;
        ensure(s, "BILLING_POLICY", || same)?;
    }
    let evidence = policy_field(f, "evidence_required", s)?;
    let reversal = policy_field(f, "allow_reversal", s)?;
    ensure(s, "BILLING_POLICY", || {
        evidence.as_bool() == Some(true) && reversal.is_boolean()
    })?;
    for name in ["ordinary", "corrections"] {
        let window = policy_field(f, name, s)?;
        keys(
            window,
            &["starts_at", "occurs_before", "received_by", "accepted_by"],
            s,
        )?;
        let mut times = [None; 4];
        for (target, field) in
            times
                .iter_mut()
                .zip(["starts_at", "occurs_before", "received_by", "accepted_by"])
        {
            let raw = policy_text(policy_field(window, field, s)?, s)?;
            // Keep the original four-parse prefix: malformed syntax becomes
            // None, while a resource failure is propagated without recasting.
            *target = match Timestamp::parse_with_work(raw, s.semantic_work()) {
                Ok(time) => Some(time.micros()),
                Err(error) if error.code == "TIMESTAMP" => None,
                Err(error) => return Err(core_error(error)),
            };
        }
        ensure(s, "BILLING_POLICY", || times.iter().all(Option::is_some))?;
        let start = compare(times[0].unwrap(), times[1].unwrap(), s)?;
        let receipt = compare(times[1].unwrap(), times[2].unwrap(), s)?;
        let accepted = compare(times[2].unwrap(), times[3].unwrap(), s)?;
        ensure(s, "BILLING_POLICY", || {
            start.is_lt() && !receipt.is_gt() && !accepted.is_gt()
        })?;
    }
    let codes = policy_field(f, "codes", s)?;
    ensure(s, "BILLING_POLICY", || codes.is_array())?;
    let codes = codes.as_array().unwrap();
    let count = compare(codes.len(), 32usize, s)?;
    ensure(s, "BILLING_POLICY", || !codes.is_empty() && !count.is_gt())?;
    for (index, code) in codes.iter().enumerate() {
        keys(code, &["code", "amount"], s)?;
        let name = policy_field(code, "code", s)?;
        slug(name, s)?;
        let name = policy_text(name, s)?;
        for earlier in &codes[..index] {
            let same = equal(name, policy_text(policy_field(earlier, "code", s)?, s)?, s)?;
            ensure(s, "BILLING_POLICY", || !same)?;
        }
        let amount = policy_field(code, "amount", s)?;
        keys(amount, &["kind", "money"], s)?;
        let same = equal(
            policy_text(policy_field(amount, "kind", s)?, s)?,
            "fixed",
            s,
        )?;
        ensure(s, "BILLING_POLICY", || same)?;
        policy_money(
            policy_field(amount, "money", s)?,
            if equal(&row.schema, "ledger-local-billing/2", s)? {
                18
            } else {
                2
            },
            s,
        )?;
    }
    let replacements = policy_field(f, "replacement_codes", s)?;
    ensure(s, "BILLING_POLICY", || replacements.is_array())?;
    let replacements = replacements.as_array().unwrap();
    let count = compare(replacements.len(), 32usize, s)?;
    ensure(s, "BILLING_POLICY", || !count.is_gt())?;
    for (index, replacement) in replacements.iter().enumerate() {
        let name = policy_text(replacement, s)?;
        let mut known = false;
        for code in codes {
            if equal(name, policy_text(policy_field(code, "code", s)?, s)?, s)? {
                known = true;
                break;
            }
        }
        ensure(s, "BILLING_POLICY", || known)?;
        for earlier in &replacements[..index] {
            let same = equal(name, policy_text(earlier, s)?, s)?;
            ensure(s, "BILLING_POLICY", || !same)?;
        }
    }
    keys(&limits[0], &["binding_id", "premium"], s)?;
    let same = equal(
        policy_text(policy_field(&limits[0], "binding_id", s)?, s)?,
        &row.binding,
        s,
    )?;
    ensure(s, "BILLING_POLICY", || same)?;
    let premium = policy_field(&limits[0], "premium", s)?;
    let scale = if equal(&row.schema, "ledger-local-billing/2", s)? {
        18
    } else {
        2
    };
    policy_money(premium, scale, s)?;
    // Preserve the actual second money parse in the old final predicate.
    let money = policy_money(premium, scale, s)?;
    let sign = compare(money.atoms(), 0i128, s)?;
    ensure(s, "BILLING_POLICY", || !sign.is_lt())
}
