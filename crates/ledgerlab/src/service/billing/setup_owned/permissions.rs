//! Same existing permission dictionaries and subset/revision/canonical guards.
//! A normalized change is DATA until actual retained control association is
//! checked by the held metadata caller; it is never an authority capability.
use super::*;
use ledgerlab_core::canonical::audit_arena::AuditedBytes;

pub(crate) enum PermissionRecipe<'a> {
    Legacy,
    Scoped { customer: &'a str, source: &'a str },
}
/// Immutable original rights only. The actual initial Setup is normalized
/// once, then moved into current state; later changes cannot rewrite this
/// separately owned original ceiling.
pub(crate) struct InitialPermissions {
    values: Vec<String>,
    _storage: ExternalBufferLease,
}
impl OwnedSetup {
    pub(crate) fn initial_permissions(
        &self,
        s: &mut OriginalAuditSession,
    ) -> Result<InitialPermissions> {
        let values = &self.setup.permissions;
        let count = compare(values.len(), 3usize, s)?;
        ensure(s, "BILLING_PERMISSIONS", || !count.is_gt())?;
        let slots = arithmetic(values.len(), mem::size_of::<String>(), Op::Multiply, s)?;
        let mut payload = arithmetic(slots, mem::size_of::<InitialPermissions>(), Op::Add, s)?;
        for value in values {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            payload = arithmetic(payload, value.len(), Op::Add, s)?;
            s.semantic_work()
                .scan_bytes(value.len())
                .map_err(core_error)?;
        }
        let storage = s.reserve_external_payload(payload).map_err(core_error)?;
        s.semantic_work().write_bytes(payload).map_err(core_error)?;
        let mut copied = Vec::new();
        copied
            .try_reserve_exact(values.len())
            .map_err(|_| crate::ServiceError::Unavailable)?;
        let capacity = compare(copied.capacity(), values.len(), s)?;
        ensure(s, "BILLING_PERMISSIONS", || !capacity.is_gt())?;
        for value in values {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            copied.push(value.to_owned());
        }
        Ok(InitialPermissions {
            values: copied,
            _storage: storage,
        })
    }
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
    permissions: &[Value],
    reason: &str,
    initial: &InitialPermissions,
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
        let permission = permission.as_str().expect("closed permission string");
        let mut allowed = false;
        for original in &initial.values {
            if equal(permission, original, s)? {
                allowed = true;
                break;
            }
        }
        checked(scoped, "BILLING_PERMISSIONS", s, || allowed)?;
        for old in &permissions[..index] {
            let same = equal(
                permission,
                old.as_str().expect("closed permission string"),
                s,
            )?;
            checked(scoped, "BILLING_PERMISSIONS", s, || !same)?;
        }
    }
    Ok(())
}
fn decode_work(v: &Value, scoped: bool, s: &mut OriginalAuditSession) -> Result<()> {
    checked(scoped, "BILLING_PERMISSIONS", s, || v.is_object())?;
    // The pinned serde_json Map is a BTreeMap ordered by Rust text. Compare
    // the complete closed key set once in that same order, rather than probing
    // every allowed name for every input member. The parsed owning map is
    // immutable throughout; no serialization or additional trusted flag.
    let expected: &[(&str, bool)] = if scoped {
        &[
            ("change_id", false),
            ("customer", false),
            ("expected_revision", false),
            ("permissions", true),
            ("reason", false),
            ("schema", false),
            ("source", false),
        ]
    } else {
        &[
            ("expected_revision", false),
            ("permissions", true),
            ("reason", false),
            ("schema", false),
        ]
    };
    let count = compare(v.as_object().unwrap().len(), expected.len(), s)?;
    checked(scoped, "BILLING_PERMISSIONS", s, || count.is_eq())?;
    for ((key, value), (field, array)) in v.as_object().unwrap().iter().zip(expected) {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        let known = equal(key, field, s)?;
        checked(scoped, "BILLING_PERMISSIONS", s, || known)?;
        s.semantic_work()
            .visit(Visit::Constraint)
            .map_err(core_error)?;
        if *array {
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
    initial: &InitialPermissions,
    current: &OwnedSetup,
    s: &mut OriginalAuditSession,
) -> Result<OwnedPermissionChange> {
    let scoped = matches!(recipe, PermissionRecipe::Scoped { .. });
    let parsed = s
        .parse_representation(raw, RepresentationKind::Authority)
        .map_err(core_error)?;
    // The exact closed dictionary above is the complete derived Change/ChangeV2
    // grammar (all fields required, strings except the string array). Borrow
    // its already-owning values during validation, retaining only the fields
    // actually needed by current state. No second DTO/whole-body clone or
    // malformed serde-error allocation is executed on this path.
    decode_work(parsed.value(), scoped, s)?;
    fn text<'a>(v: &'a Value, key: &str, s: &mut OriginalAuditSession) -> Result<&'a str> {
        s.semantic_work().visit(Visit::Field).map_err(core_error)?;
        Ok(v.get(key)
            .and_then(Value::as_str)
            .expect("closed permission dictionary"))
    }
    let value = parsed.value();
    let schema = text(value, "schema", s)?;
    let expected = text(value, "expected_revision", s)?;
    let reason = text(value, "reason", s)?;
    s.semantic_work().visit(Visit::Field).map_err(core_error)?;
    let permissions = value
        .get("permissions")
        .and_then(Value::as_array)
        .expect("closed permission array");
    let (change_id, body_customer, body_source) = match recipe {
        PermissionRecipe::Legacy => (None, None, None),
        PermissionRecipe::Scoped { .. } => (
            Some(text(value, "change_id", s)?),
            Some(text(value, "customer", s)?),
            Some(text(value, "source", s)?),
        ),
    };
    let prior = current.setup.grant_revision;
    if !scoped {
        let schema = equal(schema, "ledger-billing-permissions/1", s)?;
        let expected = revision(expected, false, s)?;
        let same = compare(expected, prior, s)?.is_eq();
        checked(false, "STALE_REVISION", s, || schema && same)?;
        rights(permissions, reason, initial, false, s)?;
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
        let schema = equal(schema, "ledger-billing-permissions/2", s)?;
        let c = equal(body_customer.unwrap(), customer, s)?;
        let r = equal(body_source.unwrap(), source, s)?;
        let expected = revision(expected, true, s)?;
        let same = compare(expected, prior, s)?.is_eq();
        let length = compare(change_id.unwrap().len(), 128usize, s)?;
        checked(true, "BILLING_PERMISSIONS", s, || {
            schema && c && r && same && !length.is_gt()
        })?;
        rights(permissions, reason, initial, true, s)?;
    }
    let slots = arithmetic(permissions.len(), mem::size_of::<String>(), Op::Multiply, s)?;
    let mut payload = arithmetic(slots, mem::size_of::<OwnedPermissionChange>(), Op::Add, s)?;
    for permission in permissions {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let permission = permission.as_str().expect("closed permission string");
        payload = arithmetic(payload, permission.len(), Op::Add, s)?;
        s.semantic_work()
            .scan_bytes(permission.len())
            .map_err(core_error)?;
    }
    if let Some(change_id) = change_id {
        payload = arithmetic(payload, change_id.len(), Op::Add, s)?;
        s.semantic_work()
            .scan_bytes(change_id.len())
            .map_err(core_error)?;
    }
    let storage = s.reserve_external_payload(payload).map_err(core_error)?;
    s.semantic_work().write_bytes(payload).map_err(core_error)?;
    let mut retained = Vec::new();
    retained
        .try_reserve_exact(permissions.len())
        .map_err(|_| crate::ServiceError::Unavailable)?;
    let capacity = compare(retained.capacity(), permissions.len(), s)?;
    checked(scoped, "BILLING_PERMISSIONS", s, || !capacity.is_gt())?;
    for permission in permissions {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        retained.push(
            permission
                .as_str()
                .expect("closed permission string")
                .to_owned(),
        );
    }
    let change_id = change_id.map(str::to_owned);
    Ok(OwnedPermissionChange {
        permissions: retained,
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
