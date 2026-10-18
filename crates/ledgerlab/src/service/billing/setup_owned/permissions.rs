//! Same existing permission dictionaries and subset/revision/canonical guards.
//! A normalized change is DATA until actual retained control association is
//! checked by the held metadata caller; it is never an authority capability.
use super::*;
use ledgerlab_core::canonical::audit_arena::AuditedBytes;

pub(crate) enum PermissionRecipe<'a> {
    Legacy,
    Scoped { customer: &'a str, source: &'a str },
}
pub(crate) struct OwnedPermissionChange {
    permissions: Vec<String>,
    change_id: Option<String>,
    prior_revision: u64,
    revision: u64,
    canonical: AuditedBytes,
    _storage: ExternalBufferLease,
}
impl OwnedPermissionChange {
    pub(crate) fn change_id(&self) -> Option<&str> {
        self.change_id.as_deref()
    }
    pub(crate) fn canonical_bytes(&self) -> &[u8] {
        self.canonical.bytes()
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }
}
fn invalid(scoped: bool, code: &str, s: &mut OriginalAuditSession) -> crate::ServiceError {
    if scoped {
        crate::ServiceError::IntegrityFailure
    } else {
        fail(code, s)
    }
}
fn checked(
    scoped: bool,
    code: &str,
    s: &mut OriginalAuditSession,
    check: impl FnOnce() -> bool,
) -> Result<()> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if check() {
        Ok(())
    } else {
        Err(invalid(scoped, code, s))
    }
}
fn revision(raw: &str, scoped: bool, s: &mut OriginalAuditSession) -> Result<u64> {
    checked(scoped, "STALE_REVISION", s, || !raw.is_empty())?;
    let length = compare(raw.len(), 20usize, s)?;
    checked(scoped, "STALE_REVISION", s, || !length.is_gt())?;
    s.semantic_work()
        .scan_bytes(raw.len())
        .map_err(core_error)?;
    let multiple = compare(raw.len(), 1usize, s)?.is_gt();
    checked(scoped, "STALE_REVISION", s, || {
        !multiple || !raw.starts_with('0')
    })?;
    let mut n = 0u64;
    for byte in raw.bytes() {
        s.semantic_work()
            .visit(Visit::ScalarDigit)
            .map_err(core_error)?;
        checked(scoped, "STALE_REVISION", s, || byte.is_ascii_digit())?;
        s.semantic_work()
            .arithmetic(Op::Subtract, &[&byte.into(), &b'0'.into()])
            .map_err(core_error)?;
        let digit = u64::from(byte - b'0');
        s.semantic_work()
            .arithmetic(Op::Multiply, &[&n.into(), &10u64.into()])
            .map_err(core_error)?;
        n = n
            .checked_mul(10)
            .ok_or_else(|| invalid(scoped, "STALE_REVISION", s))?;
        s.semantic_work()
            .arithmetic(Op::Add, &[&n.into(), &digit.into()])
            .map_err(core_error)?;
        n = n
            .checked_add(digit)
            .ok_or_else(|| invalid(scoped, "STALE_REVISION", s))?;
    }
    Ok(n)
}
fn rights(
    permissions: &[String],
    reason: &str,
    initial: &Setup,
    scoped: bool,
    s: &mut OriginalAuditSession,
) -> Result<()> {
    s.semantic_work()
        .scan_bytes(reason.len())
        .map_err(core_error)?;
    let length = compare(reason.len(), 8192usize, s)?;
    let count = compare(permissions.len(), 3usize, s)?;
    checked(scoped, "BILLING_PERMISSIONS", s, || {
        !reason.trim().is_empty() && !length.is_gt() && !count.is_gt()
    })?;
    for (index, permission) in permissions.iter().enumerate() {
        let mut allowed = false;
        for original in &initial.permissions {
            if equal(permission, original, s)? {
                allowed = true;
                break;
            }
        }
        checked(scoped, "BILLING_PERMISSIONS", s, || allowed)?;
        for old in &permissions[..index] {
            let same = equal(permission, old, s)?;
            checked(scoped, "BILLING_PERMISSIONS", s, || !same)?;
        }
    }
    Ok(())
}
fn decode_work(v: &Value, scoped: bool, s: &mut OriginalAuditSession) -> Result<()> {
    checked(scoped, "BILLING_PERMISSIONS", s, || v.is_object())?;
    let expected: &[&str] = if scoped {
        &[
            "schema",
            "customer",
            "source",
            "change_id",
            "expected_revision",
            "permissions",
            "reason",
        ]
    } else {
        &["schema", "expected_revision", "permissions", "reason"]
    };
    let count = compare(v.as_object().unwrap().len(), expected.len(), s)?;
    checked(scoped, "BILLING_PERMISSIONS", s, || count.is_eq())?;
    for (key, value) in v.as_object().unwrap() {
        let mut known = false;
        for field in expected {
            if equal(key, field, s)? {
                known = true;
                break;
            }
        }
        checked(scoped, "BILLING_PERMISSIONS", s, || known)?;
        if equal(key, "permissions", s)? {
            checked(scoped, "BILLING_PERMISSIONS", s, || value.is_array())?;
            for value in value.as_array().unwrap() {
                checked(scoped, "BILLING_PERMISSIONS", s, || value.is_string())?;
            }
        } else {
            checked(scoped, "BILLING_PERMISSIONS", s, || value.is_string())?;
        }
    }
    Ok(())
}
pub(crate) fn parse_permission(
    raw: &[u8],
    recipe: PermissionRecipe<'_>,
    initial: &OwnedSetup,
    current: &OwnedSetup,
    s: &mut OriginalAuditSession,
) -> Result<OwnedPermissionChange> {
    let scoped = matches!(recipe, PermissionRecipe::Scoped { .. });
    let parsed = s
        .parse_representation(raw, RepresentationKind::Authority)
        .map_err(core_error)?;
    let (arena, work) = s.parts();
    let copy = arena
        .value_copy_payload_bound(parsed.value(), work)
        .map_err(core_error)?;
    let mut payload = arithmetic(
        copy,
        mem::size_of::<super::super::permissions::Change>()
            + mem::size_of::<super::super::permissions::ChangeV2>()
            + mem::size_of::<OwnedPermissionChange>(),
        Op::Add,
        s,
    )?;
    let message = arithmetic(raw.len(), 6, Op::Multiply, s)?;
    const EXPECTED: &str = stringify!(schema customer source change_id expected_revision permissions reason String array object);
    let expected = arithmetic(EXPECTED.len(), 2, Op::Multiply, s)?;
    let overhead = arithmetic(expected, 256, Op::Add, s)?;
    let message = arithmetic(message, overhead, Op::Add, s)?;
    let errors = arithmetic(message, 3, Op::Multiply, s)?;
    payload = arithmetic(payload, errors, Op::Add, s)?;
    let storage = s.reserve_external_payload(payload).map_err(core_error)?;
    s.semantic_work().scan_bytes(payload).map_err(core_error)?;
    s.semantic_work().write_bytes(payload).map_err(core_error)?;
    decode_work(parsed.value(), scoped, s)?;
    let (permissions, change_id, reason, expected, schema, body_customer, body_source) =
        match &recipe {
            PermissionRecipe::Legacy => {
                let row = super::super::permissions::Change::deserialize(parsed.value())
                    .map_err(|_| invalid(false, "BILLING_PERMISSIONS", s))?;
                (
                    row.permissions,
                    None,
                    row.reason,
                    row.expected_revision,
                    row.schema,
                    None,
                    None,
                )
            }
            PermissionRecipe::Scoped { .. } => {
                let row = super::super::permissions::ChangeV2::deserialize(parsed.value())
                    .map_err(|_| invalid(true, "BILLING_PERMISSIONS", s))?;
                (
                    row.permissions,
                    Some(row.change_id),
                    row.reason,
                    row.expected_revision,
                    row.schema,
                    Some(row.customer),
                    Some(row.source),
                )
            }
        };
    let prior = current.setup.grant_revision;
    if !scoped {
        let schema = equal(&schema, "ledger-billing-permissions/1", s)?;
        let expected = revision(&expected, false, s)?;
        let same = compare(expected, prior, s)?.is_eq();
        checked(false, "STALE_REVISION", s, || schema && same)?;
        rights(&permissions, &reason, &initial.setup, false, s)?;
    }
    let canonical = s
        .canonical_representation(parsed.value(), RepresentationKind::Authority)
        .map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(canonical.bytes().len())
        .map_err(core_error)?;
    s.semantic_work()
        .scan_bytes(raw.len())
        .map_err(core_error)?;
    if canonical.bytes() != raw {
        return Err(crate::ServiceError::IntegrityFailure);
    }
    s.semantic_work()
        .arithmetic(Op::Add, &[&prior.into(), &1u64.into()])
        .map_err(core_error)?;
    let next = prior
        .checked_add(1)
        .ok_or(crate::ServiceError::IntegrityFailure)?;
    if let PermissionRecipe::Scoped { customer, source } = recipe {
        let schema = equal(&schema, "ledger-billing-permissions/2", s)?;
        let c = equal(body_customer.as_deref().unwrap(), customer, s)?;
        let r = equal(body_source.as_deref().unwrap(), source, s)?;
        let expected = revision(&expected, true, s)?;
        let same = compare(expected, prior, s)?.is_eq();
        let length = compare(change_id.as_deref().unwrap().len(), 128usize, s)?;
        checked(true, "BILLING_PERMISSIONS", s, || {
            schema && c && r && same && !length.is_gt()
        })?;
        rights(&permissions, &reason, &initial.setup, true, s)?;
    }
    Ok(OwnedPermissionChange {
        permissions,
        change_id,
        prior_revision: prior,
        revision: next,
        canonical,
        _storage: storage,
    })
}
impl OwnedSetup {
    pub(crate) fn apply_permissions(
        &mut self,
        change: OwnedPermissionChange,
        s: &mut OriginalAuditSession,
    ) -> Result<()> {
        let same = compare(self.setup.grant_revision, change.prior_revision, s)?.is_eq();
        checked(true, "BILLING_PERMISSIONS", s, || same)?;
        s.semantic_work()
            .write_bytes(
                mem::size_of::<Vec<String>>()
                    + mem::size_of::<u64>()
                    + mem::size_of::<Option<ExternalBufferLease>>(),
            )
            .map_err(core_error)?;
        self.setup.permissions = change.permissions;
        self.setup.grant_revision = change.revision;
        self._permissions_storage = Some(change._storage);
        Ok(())
    }
}
