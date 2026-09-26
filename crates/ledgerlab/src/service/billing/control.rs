//! Strict DTOs and deterministic validation for M2 customer/agreement controls.
use super::*;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Register {
    pub schema: String,
    pub customer: String,
    pub source: String,
    pub change_id: String,
    pub expected_revision: String,
    pub effective_at: Timestamp,
    pub setup: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Amend {
    pub schema: String,
    pub customer: String,
    pub source: String,
    pub change_id: String,
    pub expected_revision: String,
    pub effective_at: Timestamp,
    pub setup: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct End {
    pub schema: String,
    pub customer: String,
    pub source: String,
    pub agreement: String,
    pub change_id: String,
    pub expected_revision: String,
    pub effective_at: Timestamp,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub(crate) enum AgreementCommand {
    Register(Register, Setup),
    Amend(Amend, Setup),
    End(End),
}

pub(crate) struct ValidatedControl {
    pub customer: String,
    pub source: String,
    pub change_id: String,
    pub operation: String,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    pub recorded_at_us: i64,
    pub new_customer_scope: Option<(String, String)>,
    pub revision: i64,
    pub agreement_id: String,
    pub agreement_version: i64,
    pub transition: String,
    pub effective_at_us: i64,
    pub setup_bytes: Option<Vec<u8>>,
}

pub(crate) fn parse(raw: &[u8]) -> Result<AgreementCommand> {
    let value = canonical::parse_bounded(raw, 131_072).map_err(|_| reject("BILLING_CONTROL"))?;
    let schema = value
        .get("schema")
        .and_then(Value::as_str)
        .ok_or_else(|| reject("BILLING_CONTROL"))?
        .to_owned();
    let command = match schema.as_str() {
        "ledger-billing-registration/2" => {
            let request: Register =
                serde_json::from_value(value).map_err(|_| reject("BILLING_CONTROL"))?;
            require(request.schema == schema, "BILLING_CONTROL")?;
            validate_identity(&request.customer, &request.source, &request.change_id)?;
            Revision::parse(&request.expected_revision).map_err(|_| reject("STALE_REVISION"))?;
            let setup = parse_setup(&request.setup, &request.customer, &request.source)?;
            AgreementCommand::Register(request, setup)
        }
        "ledger-billing-amendment/2" => {
            let request: Amend =
                serde_json::from_value(value).map_err(|_| reject("BILLING_CONTROL"))?;
            require(request.schema == schema, "BILLING_CONTROL")?;
            validate_identity(&request.customer, &request.source, &request.change_id)?;
            Revision::parse(&request.expected_revision).map_err(|_| reject("STALE_REVISION"))?;
            let setup = parse_setup(&request.setup, &request.customer, &request.source)?;
            AgreementCommand::Amend(request, setup)
        }
        "ledger-billing-ending/2" => {
            let request: End =
                serde_json::from_value(value).map_err(|_| reject("BILLING_CONTROL"))?;
            require(request.schema == schema, "BILLING_CONTROL")?;
            validate_identity(&request.customer, &request.source, &request.change_id)?;
            validate_identity(&request.customer, &request.source, &request.agreement)?;
            Revision::parse(&request.expected_revision).map_err(|_| reject("STALE_REVISION"))?;
            require(
                !request.reason.trim().is_empty() && request.reason.len() <= 8192,
                "BILLING_CONTROL",
            )?;
            AgreementCommand::End(request)
        }
        _ => return Err(reject("BILLING_CONTROL")),
    };
    Ok(command)
}

fn parse_setup(value: &Value, customer: &str, source: &str) -> Result<Setup> {
    let raw = bytes(value)?;
    let setup = Setup::parse(&raw)?;
    require(
        setup.customer == customer && setup.source == source,
        "BILLING_SCOPE",
    )?;
    Ok(setup)
}

fn validate_identity(customer: &str, source: &str, change_id: &str) -> Result<()> {
    require(
        !customer.is_empty()
            && customer.len() <= 128
            && !customer.chars().any(char::is_control)
            && !source.is_empty()
            && source.len() <= 256
            && !source.chars().any(char::is_control)
            && !change_id.is_empty()
            && change_id.len() <= 128
            && !change_id.chars().any(char::is_control),
        "BILLING_IDENTIFIER",
    )?;
    Ok(())
}

pub(crate) fn canonical_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    bytes(&serde_json::to_value(value).map_err(|_| b::integrity())?)
}

/// A later control cannot alter the version resolved for work accepted earlier.
pub(crate) fn validate_transition_time(
    effective_at: &Timestamp,
    recorded_at: &Timestamp,
    previous_effective_at_us: Option<i64>,
    is_first_start: bool,
) -> Result<()> {
    require(
        is_first_start || effective_at.micros() >= recorded_at.micros(),
        "BILLING_EFFECTIVE_TIME",
    )?;
    if let Some(previous) = previous_effective_at_us {
        require(effective_at.micros() > previous, "BILLING_EFFECTIVE_TIME")?;
    }
    Ok(())
}

pub(super) fn ensure_new_ledger_time(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    audit: &history::Audit,
    at: &Timestamp,
) -> Result<()> {
    let mut maximum: Option<i64> = None;
    let mut observe = |value: i64| {
        maximum = Some(maximum.map_or(value, |prior| prior.max(value)));
    };
    for receipt in audit.receipts.values() {
        observe(b::time(&receipt["body"]["accepted_at"])?.micros());
    }
    for row in &snapshot.agreements {
        observe(row.recorded_at_us);
    }
    for row in &snapshot.controls {
        observe(row.recorded_at_us);
    }
    for row in &snapshot.scoped_permissions {
        observe(row.recorded_at_us);
    }
    if let Some(maximum) = maximum {
        require(at.micros() > maximum, "BILLING_CLOCK_NOT_ADVANCED")?;
    }
    Ok(())
}

pub(crate) fn customer_scope(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
) -> Result<Scope> {
    let row = snapshot
        .customers
        .iter()
        .find(|row| row.customer == customer)
        .ok_or_else(|| reject("BILLING_SCOPE"))?;
    Scope::new(&row.tenant, &row.environment).map_err(|_| b::integrity())
}

pub(crate) fn validate_customer_registry(
    snapshot: &crate::store::sqlite::BillingSnapshot,
) -> Result<()> {
    let initial = Setup::parse(&snapshot.setup)?;
    require(!snapshot.customers.is_empty(), "BILLING_SCOPE")?;
    b::check(canonical_bytes(&initial)? == snapshot.setup)?;
    let legacy_first = first_terms(snapshot, &initial.customer, &initial.source)?;
    b::check(canonical_bytes(&legacy_first)? == snapshot.setup)?;
    let mut scopes = BTreeSet::new();
    for customer in &snapshot.customers {
        let scope =
            Scope::new(&customer.tenant, &customer.environment).map_err(|_| b::integrity())?;
        let expected = if customer.customer == initial.customer {
            initial.scope.clone()
        } else {
            derived_scope(&initial.scope, &customer.customer)?
        };
        b::check(scope == expected)?;
        if customer.customer != initial.customer {
            b::check(scope != initial.scope)?;
        }
        b::check(scopes.insert((scope.tenant().to_owned(), scope.environment().to_owned())))?;
    }
    b::check(
        snapshot
            .customers
            .iter()
            .any(|customer| customer.customer == initial.customer),
    )?;
    for agreement in &snapshot.agreements {
        let scope = customer_scope(snapshot, &agreement.customer)?;
        let _ = Scope::new(scope.tenant(), scope.environment()).map_err(|_| b::integrity())?;
    }
    validate_timelines(snapshot, &initial)?;
    validate_control_history(snapshot, &initial)?;
    Ok(())
}

fn derived_scope(installation: &Scope, customer: &str) -> Result<Scope> {
    let input = json!([
        "bean-counter.customer-scope",
        1,
        [installation.tenant(), installation.environment()],
        customer
    ]);
    let encoded = bytes(&input)?;
    let digest = ledgerlab_core::canonical::hex(&Sha256::digest(encoded));
    Scope::new(installation.tenant(), &format!("bc-customer-{digest}"))
        .map_err(|_| reject("BILLING_SCOPE"))
}

pub(crate) fn scope_for_registration(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
) -> Result<Scope> {
    let initial = Setup::parse(&snapshot.setup)?;
    if let Some(existing) = snapshot
        .customers
        .iter()
        .find(|row| row.customer == customer)
    {
        let scope =
            Scope::new(&existing.tenant, &existing.environment).map_err(|_| b::integrity())?;
        return Ok(scope);
    }
    let candidate = derived_scope(&initial.scope, customer)?;
    require(candidate != initial.scope, "BILLING_SCOPE_COLLISION")?;
    require(
        snapshot.customers.iter().all(|row| {
            row.tenant != candidate.tenant() || row.environment != candidate.environment()
        }),
        "BILLING_SCOPE_COLLISION",
    )?;
    Ok(candidate)
}

pub(crate) fn selected_terms(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
    source: &str,
    at: &Timestamp,
) -> Result<Option<(Setup, i64)>> {
    let mut active: Option<(Setup, i64)> = None;
    let mut transitions = snapshot
        .agreements
        .iter()
        .filter(|row| row.customer == customer && row.source == source)
        .collect::<Vec<_>>();
    transitions.sort_by_key(|row| row.revision);
    for transition in transitions {
        if transition.effective_at_us > at.micros() {
            break;
        }
        match transition.transition.as_str() {
            "start" | "amend" => {
                let raw = transition.setup.as_deref().ok_or_else(b::integrity)?;
                let setup = Setup::parse(raw)?;
                let version = transition.agreement_version;
                b::check(
                    transition.agreement_id == setup.agreement
                        && setup.customer == customer
                        && setup.source == source
                        && setup.scope == customer_scope(snapshot, customer)?,
                )?;
                active = Some((setup, version));
            }
            "end" => active = None,
            _ => return Err(b::integrity()),
        }
    }
    Ok(active)
}

pub(crate) fn prepare(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    raw: &[u8],
    recorded_at: &Timestamp,
    expected_customer: &str,
    expected_source: &str,
) -> Result<(Value, Option<ValidatedControl>)> {
    validate_customer_registry(snapshot)?;
    let command = parse(raw)?;
    let (customer, source, change_id, operation, request, expected_revision, effective_at) =
        match &command {
            AgreementCommand::Register(request, _) => (
                request.customer.as_str(),
                request.source.as_str(),
                request.change_id.as_str(),
                "start",
                canonical_bytes(request)?,
                request.expected_revision.as_str(),
                request.effective_at.clone(),
            ),
            AgreementCommand::Amend(request, _) => (
                request.customer.as_str(),
                request.source.as_str(),
                request.change_id.as_str(),
                "amend",
                canonical_bytes(request)?,
                request.expected_revision.as_str(),
                request.effective_at.clone(),
            ),
            AgreementCommand::End(request) => (
                request.customer.as_str(),
                request.source.as_str(),
                request.change_id.as_str(),
                "end",
                canonical_bytes(request)?,
                request.expected_revision.as_str(),
                request.effective_at.clone(),
            ),
        };
    require(
        customer == expected_customer && source == expected_source,
        "BILLING_SCOPE",
    )?;

    let timelines = timeline(snapshot, customer, source);
    if operation != "start" {
        require(!timelines.is_empty(), "BILLING_SCOPE")?;
    }
    let scope = scope_for_registration(snapshot, customer)?;
    let control_row = snapshot
        .controls
        .iter()
        .find(|old| old.customer == customer && old.source == source && old.change_id == change_id);
    if let Some(old) = control_row {
        require(
            old.operation == operation && old.request == request,
            "IDENTITY_CONFLICT",
        )?;
        let result = canonical::parse_bounded(&old.response, 65_536).map_err(|_| b::integrity())?;
        b::check(canonical_bytes(&result)? == old.response)?;
        return Ok((result, None));
    }

    let latest = timelines.last().copied();
    let current_revision = latest.map_or(0, |row| row.revision);
    require(
        expected_revision == current_revision.to_string(),
        "STALE_REVISION",
    )?;
    if let Some(latest) = latest {
        match operation {
            "start" => require(latest.transition == "end", "BILLING_AGREEMENT_ACTIVE")?,
            "amend" | "end" => require(latest.transition != "end", "BILLING_AGREEMENT_ENDED")?,
            _ => return Err(b::integrity()),
        }
    } else {
        require(operation == "start", "BILLING_SCOPE")?;
    }
    if let AgreementCommand::End(request) = &command {
        require(
            latest.is_some_and(|row| row.agreement_id == request.agreement),
            "BILLING_AGREEMENT_ID",
        )?;
    }

    let initial = Setup::parse(&snapshot.setup)?;
    let audit = history::load(&initial, snapshot)?;
    ensure_new_ledger_time(snapshot, &audit, recorded_at)?;
    validate_transition_time(
        &effective_at,
        recorded_at,
        latest.map(|row| row.effective_at_us),
        operation == "start" && latest.is_none(),
    )?;

    let (agreement_id, agreement_version, setup_bytes, version_for_response, new_scope) =
        match &command {
            AgreementCommand::Register(_, setup) => {
                let mut setup = (*setup).clone();
                validate_business_identity(&initial, &setup, customer, source, &scope)?;
                if !timelines.is_empty() {
                    let first = first_terms(snapshot, customer, source)?;
                    require(
                        setup.permissions == first.permissions,
                        "BILLING_PERMISSION_CEILING",
                    )?;
                    require(
                        latest.is_some_and(|row| row.agreement_id != setup.agreement),
                        "BILLING_AGREEMENT_ID",
                    )?;
                }
                setup.scope = scope.clone();
                let bytes = canonical_bytes(&setup)?;
                let new_scope = if snapshot
                    .customers
                    .iter()
                    .any(|row| row.customer == customer)
                {
                    None
                } else {
                    Some((scope.tenant().to_owned(), scope.environment().to_owned()))
                };
                (setup.agreement, 1, Some(bytes), "1".to_owned(), new_scope)
            }
            AgreementCommand::Amend(_, setup) => {
                let mut setup = (*setup).clone();
                let latest = latest.ok_or_else(|| reject("BILLING_SCOPE"))?;
                let current = latest
                    .setup
                    .as_deref()
                    .ok_or_else(b::integrity)
                    .and_then(Setup::parse)?;
                let first = first_terms(snapshot, customer, source)?;
                validate_business_identity(&initial, &setup, customer, source, &scope)?;
                require(
                    setup.agreement == current.agreement && setup.permissions == first.permissions,
                    "BILLING_AGREEMENT_ID",
                )?;
                setup.scope = scope.clone();
                let version = latest.agreement_version + 1;
                let bytes = canonical_bytes(&setup)?;
                (
                    setup.agreement,
                    version,
                    Some(bytes),
                    version.to_string(),
                    None,
                )
            }
            AgreementCommand::End(_) => {
                let latest = latest.ok_or_else(|| reject("BILLING_SCOPE"))?;
                (
                    latest.agreement_id.clone(),
                    latest.agreement_version,
                    None,
                    "ended".to_owned(),
                    None,
                )
            }
        };

    let revision = current_revision + 1;
    let result = json!({
        "schema":"ledger-billing-control-result/2",
        "status": match operation {
            "start" => "agreement_started",
            "amend" => "agreement_amended",
            "end" => "agreement_ended",
            _ => "error",
        },
        "customer":customer,
        "source":source,
        "change_id":change_id,
        "revision":revision.to_string(),
        "agreement_id":agreement_id,
        "agreement_version":version_for_response,
        "effective_at":effective_at
    });
    let plan = ValidatedControl {
        customer: customer.into(),
        source: source.into(),
        change_id: change_id.into(),
        operation: operation.into(),
        request,
        response: canonical_bytes(&result)?,
        recorded_at_us: recorded_at.micros(),
        new_customer_scope: new_scope,
        revision,
        agreement_id,
        agreement_version,
        transition: operation.into(),
        effective_at_us: effective_at.micros(),
        setup_bytes,
    };
    Ok((result, Some(plan)))
}

fn timeline<'a>(
    snapshot: &'a crate::store::sqlite::BillingSnapshot,
    customer: &str,
    source: &str,
) -> Vec<&'a crate::store::sqlite::BillingAgreement> {
    let mut rows = snapshot
        .agreements
        .iter()
        .filter(|row| row.customer == customer && row.source == source)
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| row.revision);
    rows
}

fn validate_business_identity(
    installation: &Setup,
    proposed: &Setup,
    customer: &str,
    source: &str,
    customer_scope: &Scope,
) -> Result<()> {
    require(
        proposed.customer == customer
            && proposed.source == source
            && proposed.store_id == installation.store_id
            && proposed.operator == installation.operator
            && proposed.host == installation.host
            && (proposed.scope == installation.scope || proposed.scope == customer_scope.clone()),
        "BILLING_SCOPE",
    )?;
    Ok(())
}

pub(crate) fn terms_for_entry(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
    source: &str,
    agreement_id: &str,
    agreement_version: i64,
) -> Result<Setup> {
    let row = snapshot
        .agreements
        .iter()
        .find(|row| {
            row.customer == customer
                && row.source == source
                && row.agreement_id == agreement_id
                && row.agreement_version == agreement_version
                && row.setup.is_some()
        })
        .ok_or_else(b::integrity)?;
    let setup = Setup::parse(row.setup.as_deref().ok_or_else(b::integrity)?)?;
    b::check(
        setup.customer == customer
            && setup.source == source
            && setup.agreement == agreement_id
            && setup.scope == customer_scope(snapshot, customer)?,
    )?;
    Ok(setup)
}

fn validate_timelines(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    installation: &Setup,
) -> Result<()> {
    let mut pairs = BTreeSet::new();
    for row in &snapshot.agreements {
        pairs.insert((row.customer.clone(), row.source.clone()));
    }
    for (customer, source) in pairs {
        let mut rows = snapshot
            .agreements
            .iter()
            .filter(|row| row.customer == customer && row.source == source)
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.revision);
        let mut active: Option<(String, i64)> = None;
        let mut previous_time = None;
        for (index, row) in rows.into_iter().enumerate() {
            let expected_revision = index as i64 + 1;
            b::check(row.revision == expected_revision)?;
            if let Some(previous) = previous_time {
                b::check(row.effective_at_us > previous)?;
                b::check(row.effective_at_us >= row.recorded_at_us)?;
            }
            match row.transition.as_str() {
                "start" => {
                    b::check(active.is_none())?;
                    let setup = Setup::parse(row.setup.as_deref().ok_or_else(b::integrity)?)?;
                    let first_setup = if expected_revision > 1 {
                        Some(first_terms(snapshot, &customer, &source)?)
                    } else {
                        None
                    };
                    b::check(
                        row.agreement_version == 1
                            && row.agreement_id == setup.agreement
                            && setup.customer == customer
                            && setup.source == source
                            && setup.scope == customer_scope(snapshot, &customer)?
                            && setup.store_id == installation.store_id
                            && setup.operator == installation.operator
                            && setup.host == installation.host
                            && first_setup
                                .as_ref()
                                .is_none_or(|first| setup.permissions == first.permissions),
                    )?;
                    if let Some((old_agreement, _)) = active {
                        b::check(old_agreement != setup.agreement)?;
                    }
                    if expected_revision > 1 {
                        b::check(row.effective_at_us >= row.recorded_at_us)?;
                    }
                    active = Some((setup.agreement, 1));
                }
                "amend" => {
                    let (current_agreement, current_version) =
                        active.as_ref().ok_or_else(b::integrity)?;
                    let setup = Setup::parse(row.setup.as_deref().ok_or_else(b::integrity)?)?;
                    let first_setup = first_terms(snapshot, &customer, &source)?;
                    b::check(
                        setup.agreement == *current_agreement
                            && row.agreement_id == *current_agreement
                            && row.agreement_version == current_version + 1
                            && setup.customer == customer
                            && setup.source == source
                            && setup.scope == customer_scope(snapshot, &customer)?
                            && setup.store_id == installation.store_id
                            && setup.operator == installation.operator
                            && setup.host == installation.host
                            && setup.permissions == first_setup.permissions,
                    )?;
                    active = Some((setup.agreement, current_version + 1));
                }
                "end" => {
                    let (current_agreement, current_version) =
                        active.take().ok_or_else(b::integrity)?;
                    b::check(
                        row.setup.is_none()
                            && row.agreement_id == current_agreement
                            && row.agreement_version == current_version,
                    )?;
                }
                _ => return Err(b::integrity()),
            }
            previous_time = Some(row.effective_at_us);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_identity;

    #[test]
    fn source_identifiers_keep_the_legacy_256_byte_bound() {
        let source_256 = format!("urn:{}", "a".repeat(252));
        let source_257 = format!("urn:{}", "a".repeat(253));
        assert_eq!(source_256.len(), 256);
        assert_eq!(source_257.len(), 257);
        assert!(validate_identity("customer", &source_256, "change").is_ok());
        assert!(validate_identity("customer", &source_257, "change").is_err());
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PermissionRequest {
    schema: String,
    customer: String,
    source: String,
    change_id: String,
    expected_revision: String,
    permissions: Vec<String>,
    reason: String,
}

fn validate_control_history(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    installation: &Setup,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut agreement_revisions = BTreeSet::new();
    let mut permission_revisions = BTreeSet::new();
    let mut agreement_controls = 0usize;
    let mut permission_controls = 0usize;
    for control in &snapshot.controls {
        b::check(seen.insert((
            control.customer.clone(),
            control.source.clone(),
            control.change_id.clone(),
        )))?;
        let response =
            canonical::parse_bounded(&control.response, 65_536).map_err(|_| b::integrity())?;
        b::check(canonical_bytes(&response)? == control.response)?;
        if control.operation == "permissions" {
            permission_controls += 1;
            let request: PermissionRequest = serde_json::from_value(
                canonical::parse_bounded(&control.request, 65_536).map_err(|_| b::integrity())?,
            )
            .map_err(|_| b::integrity())?;
            let encoded = canonical_bytes(&request)?;
            let expected_revision = request
                .expected_revision
                .parse::<i64>()
                .map_err(|_| b::integrity())?;
            let revision = expected_revision.checked_add(1).ok_or_else(b::integrity)?;
            b::check(!request.reason.trim().is_empty() && request.reason.len() <= 8192)?;
            let initial = first_terms(snapshot, &control.customer, &control.source)?;
            b::check(
                request.permissions.len() <= 3
                    && request
                        .permissions
                        .iter()
                        .all(|permission| initial.permissions.contains(permission))
                    && request.permissions.iter().collect::<BTreeSet<_>>().len()
                        == request.permissions.len(),
            )?;
            let permission = snapshot
                .scoped_permissions
                .iter()
                .find(|row| {
                    row.customer == control.customer
                        && row.source == control.source
                        && row.revision == revision
                })
                .ok_or_else(b::integrity)?;
            b::check(
                request.schema == "ledger-billing-permissions/2"
                    && request.customer == control.customer
                    && request.source == control.source
                    && request.change_id == control.change_id
                    && encoded == control.request
                    && permission.canonical_bytes == encoded
                    && permission.recorded_at_us == control.recorded_at_us
                    && response
                        == json!({
                            "schema":"ledger-billing-permissions-result/2",
                            "status":"permissions_updated",
                            "customer":control.customer,
                            "source":control.source,
                            "change_id":control.change_id,
                            "revision":revision.to_string(),
                            "permissions":request.permissions
                        }),
            )?;
            b::check(permission_revisions.insert((
                control.customer.clone(),
                control.source.clone(),
                revision,
            )))?;
        } else {
            agreement_controls += 1;
            let command = parse(&control.request).map_err(|_| b::integrity())?;
            let (
                operation,
                customer,
                source,
                change_id,
                expected_revision,
                effective_at,
                agreement_id,
                agreement_version,
                request_serialized,
                mut expected_setup,
            ) = match &command {
                AgreementCommand::Register(request, setup) => (
                    "start",
                    request.customer.clone(),
                    request.source.clone(),
                    request.change_id.clone(),
                    request.expected_revision.clone(),
                    request.effective_at.clone(),
                    setup.agreement.clone(),
                    1,
                    canonical_bytes(request)?,
                    Some((*setup).clone()),
                ),
                AgreementCommand::Amend(request, setup) => {
                    let expected = request
                        .expected_revision
                        .parse::<i64>()
                        .map_err(|_| b::integrity())?;
                    let previous = timeline(snapshot, &request.customer, &request.source)
                        .into_iter()
                        .find(|row| row.revision == expected)
                        .ok_or_else(b::integrity)?;
                    (
                        "amend",
                        request.customer.clone(),
                        request.source.clone(),
                        request.change_id.clone(),
                        request.expected_revision.clone(),
                        request.effective_at.clone(),
                        setup.agreement.clone(),
                        previous.agreement_version + 1,
                        canonical_bytes(request)?,
                        Some((*setup).clone()),
                    )
                }
                AgreementCommand::End(request) => {
                    let expected = request
                        .expected_revision
                        .parse::<i64>()
                        .map_err(|_| b::integrity())?;
                    let previous = timeline(snapshot, &request.customer, &request.source)
                        .into_iter()
                        .find(|row| row.revision == expected)
                        .ok_or_else(b::integrity)?;
                    b::check(request.agreement == previous.agreement_id)?;
                    (
                        "end",
                        request.customer.clone(),
                        request.source.clone(),
                        request.change_id.clone(),
                        request.expected_revision.clone(),
                        request.effective_at.clone(),
                        previous.agreement_id.clone(),
                        previous.agreement_version,
                        canonical_bytes(request)?,
                        None,
                    )
                }
            };
            let expected_revision = expected_revision
                .parse::<i64>()
                .map_err(|_| b::integrity())?;
            let revision = expected_revision.checked_add(1).ok_or_else(b::integrity)?;
            let version_for_response = if operation == "end" {
                "ended".to_owned()
            } else {
                agreement_version.to_string()
            };
            let expected_status = match operation {
                "start" => "agreement_started",
                "amend" => "agreement_amended",
                "end" => "agreement_ended",
                _ => return Err(b::integrity()),
            };
            let transition = snapshot
                .agreements
                .iter()
                .find(|row| {
                    row.customer == customer && row.source == source && row.revision == revision
                })
                .ok_or_else(b::integrity)?;
            b::check(
                customer == control.customer
                    && source == control.source
                    && change_id == control.change_id
                    && control.operation == operation
                    && request_serialized == control.request
                    && transition.transition == operation
                    && transition.agreement_id == agreement_id
                    && transition.agreement_version == agreement_version
                    && transition.effective_at_us == effective_at.micros()
                    && transition.recorded_at_us == control.recorded_at_us
                    && response
                        == json!({
                            "schema":"ledger-billing-control-result/2",
                            "status":expected_status,
                            "customer":customer,
                            "source":source,
                            "change_id":change_id,
                            "revision":revision.to_string(),
                            "agreement_id":agreement_id,
                            "agreement_version":version_for_response,
                            "effective_at":effective_at
                        }),
            )?;
            b::check(agreement_revisions.insert((customer.clone(), source.clone(), revision)))?;
            if let Some(setup) = expected_setup.as_mut() {
                setup.scope = customer_scope(snapshot, &customer)?;
                let expected_setup = canonical_bytes(setup)?;
                b::check(transition.setup.as_deref() == Some(expected_setup.as_slice()))?;
            }
        }
    }
    let original_transitions = snapshot
        .agreements
        .iter()
        .filter(|row| {
            !(row.customer == installation.customer
                && row.source == installation.source
                && row.revision == 1)
        })
        .count();
    b::check(agreement_controls == original_transitions)?;
    b::check(permission_controls == snapshot.scoped_permissions.len())?;
    Ok(())
}

pub(crate) fn first_terms(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
    source: &str,
) -> Result<Setup> {
    let row = snapshot
        .agreements
        .iter()
        .find(|row| {
            row.customer == customer
                && row.source == source
                && row.revision == 1
                && row.transition == "start"
        })
        .ok_or_else(|| reject("BILLING_SCOPE"))?;
    Setup::parse(row.setup.as_deref().ok_or_else(b::integrity)?)
}
