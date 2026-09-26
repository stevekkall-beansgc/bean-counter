use super::*;
use crate::store::sqlite::BillingSnapshot;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Change {
    schema: String,
    expected_revision: String,
    permissions: Vec<String>,
    reason: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ChangeV2 {
    pub schema: String,
    pub customer: String,
    pub source: String,
    pub change_id: String,
    pub expected_revision: String,
    pub permissions: Vec<String>,
    pub reason: String,
}

pub(crate) struct ValidatedScopedPermissions {
    pub customer: String,
    pub source: String,
    pub revision: i64,
    pub change_id: String,
    pub recorded_at_us: i64,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
}

fn change(raw: &[u8], initial: &Setup, revision: u64) -> Result<Change> {
    let value = canonical::parse_bounded(raw, 65536).map_err(|_| reject("BILLING_PERMISSIONS"))?;
    let c: Change = serde_json::from_value(value).map_err(|_| reject("BILLING_PERMISSIONS"))?;
    require(
        c.schema == "ledger-billing-permissions/1" && c.expected_revision == revision.to_string(),
        "STALE_REVISION",
    )?;
    validate_rights(&c.permissions, initial, &c.reason)?;
    Ok(c)
}

fn validate_rights(permissions: &[String], initial: &Setup, reason: &str) -> Result<()> {
    require(
        !reason.trim().is_empty()
            && reason.len() <= 8192
            && permissions.len() <= 3
            && permissions.iter().all(|p| initial.permissions.contains(p))
            && permissions.iter().collect::<BTreeSet<_>>().len() == permissions.len(),
        "BILLING_PERMISSIONS",
    )
}

fn permission_history(
    snapshot: &BillingSnapshot,
    customer: &str,
    source: &str,
) -> Result<(Setup, Vec<Vec<String>>)> {
    let initial = control::first_terms(snapshot, customer, source)?;
    let legacy_pair = Setup::parse(&snapshot.setup)?;
    let mut rights_by_revision = vec![initial.permissions.clone()];
    let mut effective = initial.clone();

    // M1 revisions have no customer/source fields. They remain attached only
    // to the original setup pair and preserve their exact input bytes.
    if customer == legacy_pair.customer && source == legacy_pair.source {
        for raw in &snapshot.permissions {
            let c = change(raw, &initial, effective.grant_revision)?;
            b::check(bytes(&json!(c))? == *raw)?;
            effective.permissions = c.permissions;
            effective.grant_revision += 1;
            rights_by_revision.push(effective.permissions.clone());
        }
    }

    let mut scoped = snapshot
        .scoped_permissions
        .iter()
        .filter(|row| row.customer == customer && row.source == source)
        .collect::<Vec<_>>();
    scoped.sort_by_key(|row| row.revision);
    for row in scoped {
        let c: ChangeV2 = serde_json::from_value(
            canonical::parse_bounded(&row.canonical_bytes, 65536).map_err(|_| b::integrity())?,
        )
        .map_err(|_| b::integrity())?;
        let encoded = control::canonical_bytes(&c)?;
        let next_revision = effective.grant_revision + 1;
        b::check(
            encoded == row.canonical_bytes
                && row.revision == next_revision as i64
                && c.schema == "ledger-billing-permissions/2"
                && c.customer == customer
                && c.source == source
                && c.expected_revision == effective.grant_revision.to_string()
                && c.change_id.len() <= 128,
        )?;
        validate_rights(&c.permissions, &initial, &c.reason).map_err(|_| b::integrity())?;
        let control = snapshot
            .controls
            .iter()
            .find(|control| {
                control.customer == customer
                    && control.source == source
                    && control.change_id == c.change_id
            })
            .ok_or_else(b::integrity)?;
        b::check(
            control.operation == "permissions"
                && control.request == encoded
                && control.recorded_at_us == row.recorded_at_us,
        )?;
        effective.permissions = c.permissions;
        effective.grant_revision = next_revision;
        rights_by_revision.push(effective.permissions.clone());
    }
    Ok((effective, rights_by_revision))
}

pub(crate) fn effective(snapshot: &BillingSnapshot) -> Result<Setup> {
    let initial = Setup::parse(&snapshot.setup)?;
    effective_for(snapshot, &initial.customer, &initial.source)
}

pub(crate) fn effective_for(
    snapshot: &BillingSnapshot,
    customer: &str,
    source: &str,
) -> Result<Setup> {
    Ok(permission_history(snapshot, customer, source)?.0)
}

pub(crate) fn prepare_scoped(
    snapshot: &BillingSnapshot,
    customer: &str,
    source: &str,
    raw: &[u8],
    at: &Timestamp,
) -> Result<(Value, Option<ValidatedScopedPermissions>)> {
    let value = canonical::parse_bounded(raw, 65536).map_err(|_| reject("BILLING_PERMISSIONS"))?;
    if value.get("schema").and_then(Value::as_str) == Some("ledger-billing-permissions/1") {
        let initial = Setup::parse(&snapshot.setup)?;
        require(
            customer == initial.customer && source == initial.source,
            "BILLING_SCOPE",
        )?;
        let legacy: Change =
            serde_json::from_value(value).map_err(|_| reject("BILLING_PERMISSIONS"))?;
        let request = bytes(&json!(legacy))?;
        if let Some((index, _)) = snapshot
            .permissions
            .iter()
            .enumerate()
            .find(|(_, saved)| **saved == request)
        {
            let expected_revision = legacy
                .expected_revision
                .parse::<u64>()
                .map_err(|_| b::integrity())?;
            let revision = expected_revision.checked_add(1).ok_or_else(b::integrity)?;
            b::check(revision == index as u64 + 2)?;
            return Ok((
                json!({
                    "status":"permissions_updated",
                    "revision":revision.to_string(),
                    "permissions":legacy.permissions
                }),
                None,
            ));
        }
        return Err(reject("BILLING_LEGACY_RETRY_ONLY"));
    }
    let c: ChangeV2 = serde_json::from_value(value).map_err(|_| reject("BILLING_PERMISSIONS"))?;
    require(
        c.schema == "ledger-billing-permissions/2"
            && c.customer == customer
            && c.source == source
            && !c.change_id.is_empty()
            && c.change_id.len() <= 128
            && !c.change_id.chars().any(char::is_control),
        "BILLING_SCOPE",
    )?;
    control::first_terms(snapshot, customer, source)?;
    let request = control::canonical_bytes(&c)?;
    if let Some(old) = snapshot.controls.iter().find(|old| {
        old.customer == customer && old.source == source && old.change_id == c.change_id
    }) {
        require(
            old.operation == "permissions" && old.request == request,
            "IDENTITY_CONFLICT",
        )?;
        let result = canonical::parse_bounded(&old.response, 65_536).map_err(|_| b::integrity())?;
        b::check(control::canonical_bytes(&result)? == old.response)?;
        return Ok((result, None));
    }
    let (current, _) = permission_history(snapshot, customer, source)?;
    require(
        c.expected_revision == current.grant_revision.to_string(),
        "STALE_REVISION",
    )?;
    validate_rights(
        &c.permissions,
        &control::first_terms(snapshot, customer, source)?,
        &c.reason,
    )?;
    let initial = Setup::parse(&snapshot.setup)?;
    let audit = history::load(&initial, snapshot)?;
    control::ensure_new_ledger_time(snapshot, &audit, at)?;
    let revision = current.grant_revision + 1;
    let result = json!({
        "schema":"ledger-billing-permissions-result/2",
        "status":"permissions_updated",
        "customer":customer,
        "source":source,
        "change_id":c.change_id,
        "revision":revision.to_string(),
        "permissions":c.permissions
    });
    Ok((
        result.clone(),
        Some(ValidatedScopedPermissions {
            customer: customer.into(),
            source: source.into(),
            revision: revision as i64,
            change_id: c.change_id,
            recorded_at_us: at.micros(),
            request,
            response: control::canonical_bytes(&result)?,
        }),
    ))
}

pub(super) fn verify_grants(
    records: &Records,
    snapshot: &BillingSnapshot,
    customer: &str,
    source: &str,
) -> Result<()> {
    let (_, rights_by_revision) = permission_history(snapshot, customer, source)?;
    let initial = control::first_terms(snapshot, customer, source)?;
    for row in records
        .rows
        .iter()
        .filter(|r| r["kind"] == "evidence" && r["body"]["purpose"] == "grant")
    {
        let body =
            canonical::parse(text(&row["body"]["utf8"])?.as_bytes()).map_err(|_| b::integrity())?;
        let revision = text(&body["revision"])?
            .parse::<usize>()
            .map_err(|_| b::integrity())?;
        let rights = rights_by_revision
            .get(revision.saturating_sub(1))
            .ok_or_else(b::integrity)?;
        b::check(
            revision >= 1
                && body
                    == json!({"scope":initial.scope,"principal":initial.operator,"source":source,"permissions":rights,"revision":revision.to_string(),"active":true}),
        )?;
    }
    for authority in records
        .rows
        .iter()
        .filter(|r| r["kind"] == "authority-decision")
    {
        let a = &authority["body"];
        let grant = records
            .rows
            .iter()
            .find(|r| r["id"] == a["grant"])
            .ok_or_else(b::integrity)?;
        let body = canonical::parse(text(&grant["body"]["utf8"])?.as_bytes())
            .map_err(|_| b::integrity())?;
        let rights = array(&body["permissions"])?;
        b::check(
            a["grant_revision"] == body["revision"]
                && rights.contains(&json!("read"))
                && (a["may_submit"] != true || rights.contains(&json!("submit")))
                && (a["may_correct"] != true || rights.contains(&json!("correct"))),
        )?;
    }
    Ok(())
}
