//! Actual same-snapshot replacement for the legacy all-resident metadata
//! snapshot. No row hit or returned Setup establishes local-owner authority.
use super::*;
use crate::service::billing::setup_owned::{self, OwnedSetup, PermissionRecipe};
use crate::store::sqlite::workflow::financial::{ManagementRow, ManagementTable};

fn checked(s: &mut OriginalAuditSession, condition: impl FnOnce() -> bool) -> Result<()> {
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    if condition() {
        Ok(())
    } else {
        Err(ServiceError::IntegrityFailure)
    }
}
fn same(a: &str, b: &str, s: &mut OriginalAuditSession) -> Result<bool> {
    s.semantic_work()
        .visit(Visit::MemberCompare)
        .map_err(core_error)?;
    s.semantic_work().scan_bytes(a.len()).map_err(core_error)?;
    s.semantic_work().scan_bytes(b.len()).map_err(core_error)?;
    Ok(a == b)
}
fn compare(a: i64, b: i64, s: &mut OriginalAuditSession) -> Result<std::cmp::Ordering> {
    s.semantic_work()
        .arithmetic(Op::Compare, &[&a.into(), &b.into()])
        .map_err(core_error)?;
    Ok(a.cmp(&b))
}
fn text<'a>(row: &'a ManagementRow, index: usize, s: &mut OriginalAuditSession) -> Result<&'a str> {
    let raw = row.text[index].bytes();
    s.semantic_work()
        .scan_bytes(raw.len())
        .map_err(core_error)?;
    s.semantic_work()
        .visit(Visit::Constraint)
        .map_err(core_error)?;
    std::str::from_utf8(raw).map_err(|_| ServiceError::IntegrityFailure)
}
pub(in crate::service::workflow) async fn effective(
    tx: &mut SqliteTx,
    s: &mut OriginalAuditSession,
) -> Result<OwnedSetup> {
    tx.billing_management_guards(s)
        .await
        .map_err(prefix::read_error)?;
    let raw = tx
        .workflow_management_setup_bytes(s)
        .await
        .map_err(prefix::read_error)?;
    let counts = tx
        .workflow_management_counts(s)
        .await
        .map_err(prefix::read_error)?;
    // Preserve the old metadata reader's complete physical row decoding. Each
    // row is dropped before the next; nothing unrelated is mislabeled an I or
    // a required workflow-origin dependency by these incidental reads.
    for (table, count) in [
        (ManagementTable::LegacyPermissions, counts[3]),
        (ManagementTable::ScopedPermissions, counts[4]),
        (ManagementTable::Customers, counts[0]),
        (ManagementTable::Agreements, counts[1]),
        (ManagementTable::Controls, counts[2]),
    ] {
        let mut after = None;
        for _ in 0..count {
            s.semantic_work()
                .visit(Visit::Element)
                .map_err(core_error)?;
            let row = tx
                .workflow_management_row(table, after, s)
                .await
                .map_err(prefix::read_error)?
                .ok_or(ServiceError::IntegrityFailure)?;
            after = Some(row.row_id);
        }
    }
    let legacy = setup_owned::parse(raw.bytes(), s)?;
    let customer = &legacy.setup().customer;
    let source = &legacy.setup().source;
    let mut first = tx
        .workflow_management_initial_terms(customer, source, s)
        .await
        .map_err(prefix::read_error)?
        .ok_or_else(|| reject("BILLING_SCOPE"))?;
    let first = first.bodies[0]
        .take()
        .ok_or(ServiceError::IntegrityFailure)?;
    let initial = setup_owned::parse(first.bytes(), s)?;
    // Own separate current fields, preserving the old effective clone. This
    // extra normalizer pass is paid here and included in the caller envelope.
    let mut current = setup_owned::parse(first.bytes(), s)?;
    let mut after = None;
    for _ in 0..counts[3] {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let row = tx
            .workflow_management_row(ManagementTable::LegacyPermissions, after, s)
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        after = Some(row.row_id);
        let body = row.bodies[0]
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        let change = setup_owned::parse_permission(
            body.bytes(),
            PermissionRecipe::Legacy,
            &initial,
            &current,
            s,
        )?;
        current.apply_permissions(change, s)?;
    }
    let mut after = None;
    let mut selected = 0i64;
    loop {
        s.semantic_work()
            .visit(Visit::Element)
            .map_err(core_error)?;
        let row = tx
            .workflow_management_scoped_permission(customer, source, after, s)
            .await
            .map_err(prefix::read_error)?;
        let Some(row) = row else { break };
        s.semantic_work()
            .arithmetic(Op::Add, &[&selected.into(), &1i64.into()])
            .map_err(core_error)?;
        selected = selected
            .checked_add(1)
            .ok_or(ServiceError::IntegrityFailure)?;
        let count = compare(selected, counts[4], s)?;
        checked(s, || !count.is_gt())?;
        after = Some(row.numbers[0]);
        let body = row.bodies[0]
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        let change = setup_owned::parse_permission(
            body.bytes(),
            PermissionRecipe::Scoped { customer, source },
            &initial,
            &current,
            s,
        )?;
        s.semantic_work()
            .arithmetic(
                Op::Compare,
                &[&row.numbers[0].into(), &change.revision().into()],
            )
            .map_err(core_error)?;
        checked(s, || {
            u64::try_from(row.numbers[0]).ok() == Some(change.revision())
        })?;
        let control = tx
            .workflow_management_permission_control(
                customer,
                source,
                change.change_id().ok_or(ServiceError::IntegrityFailure)?,
                s,
            )
            .await
            .map_err(prefix::read_error)?
            .ok_or(ServiceError::IntegrityFailure)?;
        let operation = same(text(&control, 3, s)?, "permissions", s)?;
        checked(s, || operation)?;
        let request = control.bodies[0]
            .as_ref()
            .ok_or(ServiceError::IntegrityFailure)?;
        s.semantic_work()
            .visit(Visit::MemberCompare)
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(request.bytes().len())
            .map_err(core_error)?;
        s.semantic_work()
            .scan_bytes(change.canonical_bytes().len())
            .map_err(core_error)?;
        checked(s, || request.bytes() == change.canonical_bytes())?;
        let time = compare(control.numbers[0], row.numbers[1], s)?;
        checked(s, || time.is_eq())?;
        current.apply_permissions(change, s)?;
    }
    Ok(current)
}
