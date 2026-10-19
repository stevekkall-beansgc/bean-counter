use super::super::{arithmetic, data::*, meter, records::Records, shape, CoreMeter};
use super::{binding, check::*};
use crate::{domain::Timestamp, Error, Result};
use num_bigint::BigInt;
use serde_json::{json, Value};
pub(in crate::policy::workflow::transition_constructor) fn validate(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    state: &DraftStateData,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::entry::validate", |m| {
            validate_data(m, c, p, state)
        });
    }
    #[cfg(not(test))]
    validate_data(m, c, p, state)
}
fn validate_data(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    state: &DraftStateData,
) -> Result<()> {
    context(m, c)?;
    validate_body(m, c, p, state)
}
fn validate_body(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    state: &DraftStateData,
) -> Result<()> {
    for name in ["activity"] {
        invoke!(qualified, m, invoke!(shape::text, m, p, name)?)?;
    }
    for field in [
        "product_ref",
        "policy_ref",
        "fact_schema_ref",
        "binding_ref",
    ] {
        invoke!(shape::reference, m, data_field!(m, p, field))?;
    }
    invoke!(binding::refs, m, data_field!(m, p, "evidence_refs"), 0, 32)?;
    let evidence = invoke!(shape::array, m, p, "evidence_refs")?;
    if !meter::cmp(m, &evidence.len().into(), &0usize.into())?.is_eq() {
        return Err(Error::new(
            "UNSUPPORTED_COMPOSITION",
            "A.setup supplemental work evidence",
        ));
    }
    // Explicit selected head/claim DATA is not complete-index custody. The host
    // must prove current-head selection and absence under its held snapshot.
    if let Some(head) = &state.chain_head {
        shape::reference(m, &json!(head))?;
        if !shape::same_text(m, &head.kind, "workflow-chain-state")? {
            return Err(shape::fail("selected chain head kind"));
        }
    }
    bounds(m, state.work_claims.len(), 0, 4096)?;
    for claim in &state.work_claims {
        shape::reference(m, &json!(claim))?;
        if !shape::same_text(m, &claim.kind, "workflow-work-claim")? {
            return Err(shape::fail("selected work claim kind"));
        }
    }
    for (field, empty) in [
        ("decisions", state.decision_heads.is_empty()),
        ("families", state.family_heads.is_empty()),
        ("outcome_claims", state.outcome_claims.is_empty()),
        ("links", state.links.is_empty()),
        ("verification", state.evidence_verifications.is_empty()),
    ] {
        m.visit("inventory.initial_index")?;
        if !empty {
            // Prior unrelated heads do not become this work's pricing basis.
            // The host selects the exact snapshot; impacted dependencies are
            // separately resolved by the complete-candidate capacity pass.
            match field {
                "decisions" => {
                    bounds(m, state.decision_heads.len(), 0, 4096)?;
                    for head in &state.decision_heads {
                        shape::reference(m, &json!(head.reference))?;
                        binding::id(m, &json!(head.decision_id), 128)?;
                        crate::policy::workflow::transition_constructor::arithmetic::integer(
                            m,
                            &head.revision,
                            false,
                        )?;
                    }
                }
                "families" => {
                    bounds(m, state.family_heads.len(), 0, 4096)?;
                    for head in &state.family_heads {
                        shape::reference(m, &json!(head.reference))?;
                        shape::reference(m, &json!(head.target_ref))?;
                        binding::id(m, &json!(head.family_slot), 128)?;
                        crate::policy::workflow::transition_constructor::arithmetic::integer(
                            m,
                            &head.revision,
                            false,
                        )?;
                    }
                }
                "outcome_claims" => binding::refs(m, &json!(state.outcome_claims), 0, 4096)?,
                "links" => binding::refs(m, &json!(state.links), 0, 4096)?,
                "verification" => binding::refs(m, &json!(state.evidence_verifications), 0, 4096)?,
                _ => return Err(shape::fail("unknown work index field")),
            }
        }
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn validate_selected(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    state: &DraftStateData,
    program: &crate::policy::workflow::selected_program::SelectedProgram,
) -> Result<()> {
    if !program.uses_local_execution_recipe() {
        return validate(m, c, p, state);
    }
    local_context(m, c)?;
    validate_body(m, c, p, state)
}
pub(in crate::policy::workflow::transition_constructor) fn context_selected(
    m: &mut CoreMeter,
    c: &DraftContextData,
    program: &crate::policy::workflow::selected_program::SelectedProgram,
) -> Result<()> {
    if program.uses_local_execution_recipe() {
        local_context(m, c)
    } else {
        context(m, c)
    }
}
pub(in crate::policy::workflow::transition_constructor) fn context(
    m: &mut CoreMeter,
    c: &DraftContextData,
) -> Result<()> {
    context_data(m, c, true)
}
pub(in crate::policy::workflow::transition_constructor) fn local_context(
    m: &mut CoreMeter,
    c: &DraftContextData,
) -> Result<()> {
    context_data(m, c, false)
}
fn context_data(m: &mut CoreMeter, c: &DraftContextData, require_observation: bool) -> Result<()> {
    for value in [&c.key.logical_store_id, &c.key.operation_id, &c.actor] {
        binding::id(m, &json!(value), 128)?;
    }
    for value in &c.key.scope {
        binding::id(m, &json!(value), 128)?;
    }
    for value in [&c.key.source, &c.canonical_economic_namespace] {
        binding::string(m, &json!(value), 256)?;
    }
    one_of(
        m,
        &c.installation_mode,
        &["real_local", "isolated_synthetic"],
    )?;
    bounds(
        m,
        c.authorities.len(),
        1,
        if require_observation { 1 } else { 32 },
    )?;
    for a in &c.authorities {
        qualified(m, &a.source_slot)?;
        for r in [
            &a.enrollment_ref,
            &a.current_state_ref,
            &a.current_grant_ref,
            &a.binding_ref,
            &a.accepted_agreement_ref,
            &a.activation_ref,
        ] {
            shape::reference(m, &json!(r))?;
        }
        if require_observation {
            shape::reference(m, &json!(a.required_observation_ref()?))?;
        } else if a.authority_observation_ref.is_some() {
            return Err(super::super::shape::fail(
                "local context cannot carry an experimental observation",
            ));
        }
        binding::refs(m, &json!(a.original_grant_refs), 1, 32)?;
    }
    bounds(
        m,
        c.source_observation_inputs.len(),
        0,
        if require_observation { 2 } else { 0 },
    )?;
    for input in &c.source_observation_inputs {
        one_of(
            m,
            &input.reference.kind,
            &[
                "workflow-authority-prefix-observation",
                "workflow-authority-audit-verifier",
            ],
        )?;
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn quantities(
    m: &mut CoreMeter,
    b: &Value,
    t: &Value,
    activity: &str,
    facts: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::entry::quantities", |m| {
            quantities_data(m, b, t, activity, facts)
        });
    }
    #[cfg(not(test))]
    quantities_data(m, b, t, activity, facts)
}
fn quantities_data(
    m: &mut CoreMeter,
    b: &Value,
    t: &Value,
    activity: &str,
    facts: &Value,
) -> Result<()> {
    for declarations in [
        invoke!(shape::array, m, b, "quantity_ceilings")?,
        invoke!(shape::array, m, t, "quantity_ceilings")?,
    ] {
        for q in declarations {
            m.visit("inventory.current_quantity")?;
            if !invoke!(
                shape::same_text,
                m,
                invoke!(shape::text, m, q, "activity")?,
                activity
            )? {
                continue;
            }
            let field = invoke!(shape::text, m, q, "field")?;
            let raw = invoke!(shape::text, m, facts, field)?;
            let actual = arithmetic::integer(m, raw, true)?;
            let maximum = decimal(m, q, "maximum")?;
            let actual = meter::mul(m, &actual, &maximum.denominator)?;
            if meter::cmp(m, &actual, &maximum.numerator)?.is_gt() {
                return Err(shape::fail("accepted quantity ceiling"));
            }
        }
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn exposure(
    m: &mut CoreMeter,
    b: &Value,
    t: &Value,
    atoms: &num_bigint::BigInt,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::entry::exposure", |m| {
            exposure_data(m, b, t, atoms)
        });
    }
    #[cfg(not(test))]
    exposure_data(m, b, t, atoms)
}
fn exposure_data(
    m: &mut CoreMeter,
    b: &Value,
    t: &Value,
    atoms: &num_bigint::BigInt,
) -> Result<()> {
    if meter::cmp(m, atoms, &0.into())?.is_lt() {
        return Err(shape::fail("aggregate discount exceeds original charge"));
    }
    let slot = invoke!(shape::text, m, t, "slot")?;
    let bound = invoke!(
        shape::named,
        m,
        invoke!(shape::array, m, b, "exposure_ceilings")?,
        "binding_slot",
        slot
    )?;
    for maximum in [
        data_field!(m, bound, "maximum"),
        data_field!(m, t, "exposure_ceiling"),
    ] {
        let max = amount(m, maximum)?;
        if meter::cmp(m, atoms, &max)?.is_gt() {
            return Err(shape::fail("accepted exposure ceiling"));
        }
    }
    Ok(())
}

/// Validate the explicit selected prior chain DATA, including semantic identity,
/// original binding copies and clock order. This is not a host origin audit or
/// proof that a mutable index selected the latest retained head.
pub(in crate::policy::workflow::transition_constructor) fn chain_head(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    reference: &ExactRef,
    old: &Value,
) -> Result<(BigInt, BigInt)> {
    chain_head_data(m, c, p, reference, old, None)
}
/// The exact current immutable link set is supplied as DATA and each body is
/// validated before deriving its root; snapshot completeness stays host-owned.
pub(in crate::policy::workflow::transition_constructor) fn chain_head_with_links(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    p: &Value,
    reference: &ExactRef,
    old: &Value,
    links: &[ExactRef],
) -> Result<(BigInt, BigInt)> {
    let mut current = vec![];
    for link_ref in links {
        m.visit("chain.current_immutable_link")?;
        let link = r.get(
            m,
            &c.key,
            link_ref,
            "workflow-link",
            "ledger-workflow-link/1",
        )?;
        shape::closed(
            m,
            &link,
            &[
                "schema",
                "scope",
                "child",
                "product_ref",
                "binding_ref",
                "slot",
                "predecessor",
                "classification",
                "authority_ref",
                "operation_key",
            ],
            &[],
        )?;
        eq(m, &link, "scope", &json!(c.key.scope))?;
        eq(m, &link, "binding_ref", &p["binding_ref"])?;
        shape::reference(m, &link["child"])?;
        shape::reference(m, &link["predecessor"])?;
        shape::reference(m, &link["authority_ref"])?;
        current.push(link_ref.clone());
    }
    let current = super::super::body::ref_set(m, current)?;
    if !meter::cmp(m, &current.len().into(), &links.len().into())?.is_eq() {
        return Err(shape::fail("duplicate current link identity"));
    }
    let links_root = super::super::records::hash(m, "workflow-link-set", &json!(current))?;
    chain_head_data(m, c, p, reference, old, Some(&links_root))
}
fn chain_head_data(
    m: &mut CoreMeter,
    c: &DraftContextData,
    p: &Value,
    reference: &ExactRef,
    old: &Value,
    actual_links_root: Option<&str>,
) -> Result<(BigInt, BigInt)> {
    shape::closed(
        m,
        old,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "customer",
            "chain",
            "binding_ref",
            "accepted_agreement_ref",
            "activation_ref",
            "revision",
            "admitted_events",
            "links_root",
            "operation_key",
            "accepted_at",
        ],
        &["prior_ref"],
    )?;
    for (key, want) in [
        ("schema", json!("ledger-workflow-chain-state/1")),
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("customer", p["customer"].clone()),
        ("chain", p["chain"].clone()),
        ("binding_ref", p["binding_ref"].clone()),
        (
            "accepted_agreement_ref",
            json!(c.authorities[0].accepted_agreement_ref),
        ),
        ("activation_ref", json!(c.authorities[0].activation_ref)),
    ] {
        eq(m, old, key, &want)?;
    }
    let revision = number(m, old, "revision", true)?;
    let admitted = number(m, old, "admitted_events", true)?;
    if meter::cmp(m, &admitted, &1000.into())?.is_gt() {
        return Err(shape::fail("chain event ceiling"));
    }
    let expected = super::super::body::semantic_id(
        m,
        "wchain_",
        "workflow-chain-state",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            p["customer"],
            p["chain"],
            revision.to_string()
        ]),
    )?;
    if !shape::same_text(m, &reference.id, &expected)? {
        return Err(shape::fail("prior chain semantic identity"));
    }
    if meter::cmp(m, &revision, &1.into())?.is_eq() {
        m.visit("chain.initial_prior_absence")?;
        if old.get("prior_ref").is_some() {
            return Err(shape::fail("initial chain prior"));
        }
    } else {
        let prior = shape::field(m, old, "prior_ref")?;
        shape::reference(m, prior)?;
        eq(m, prior, "kind", &json!("workflow-chain-state"))?;
    }
    let old_key = shape::field(m, old, "operation_key")?;
    key(m, old_key, c)?;
    let old_time = time(m, old, "accepted_at")?;
    let new_time = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    if meter::cmp(m, &old_time, &new_time.into())?.is_gt() {
        return Err(Error::new("CLOCK_REGRESSION", "chain accepted time"));
    }
    let legacy_empty;
    let links_root = match actual_links_root {
        Some(root) => root,
        None => {
            legacy_empty = super::super::records::hash(m, "workflow-link-set", &json!([]))?;
            &legacy_empty
        }
    };
    eq(m, old, "links_root", &json!(links_root))?;
    Ok((revision, admitted))
}

pub(in crate::policy::workflow::transition_constructor) fn work_claim(
    m: &mut CoreMeter,
    c: &DraftContextData,
    reference: &ExactRef,
    old: &Value,
) -> Result<()> {
    work_claim_namespace(m, c, reference, old, &c.canonical_economic_namespace)
}
pub(in crate::policy::workflow::transition_constructor) fn historical_work_claim(
    m: &mut CoreMeter,
    c: &DraftContextData,
    reference: &ExactRef,
    old: &Value,
    work: &Value,
) -> Result<()> {
    let namespace = shape::text(m, work, "source_namespace")?;
    work_claim_namespace(m, c, reference, old, namespace)
}
fn work_claim_namespace(
    m: &mut CoreMeter,
    c: &DraftContextData,
    reference: &ExactRef,
    old: &Value,
    namespace: &str,
) -> Result<()> {
    shape::closed(
        m,
        old,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "source_namespace",
            "business_operation_id",
            "completion",
            "work_ref",
            "facts_digest",
        ],
        &[],
    )?;
    for (key, want) in [
        ("schema", json!("ledger-workflow-work-claim/1")),
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("source_namespace", json!(namespace)),
        ("completion", json!("completion")),
    ] {
        eq(m, old, key, &want)?;
    }
    opaque(m, old, "business_operation_id", 128)?;
    let work = shape::field(m, old, "work_ref")?;
    shape::reference(m, work)?;
    eq(m, work, "kind", &json!("workflow-work"))?;
    let hash = shape::field(m, old, "facts_digest")?;
    super::publication::digest(m, hash)?;
    let expected = super::super::body::semantic_id(
        m,
        "wclaim_",
        "workflow-work-claim",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            namespace,
            old["business_operation_id"],
            "completion"
        ]),
    )?;
    if !shape::same_text(m, &reference.id, &expected)? {
        return Err(shape::fail("work claim semantic identity"));
    }
    Ok(())
}

#[cfg(test)]
mod chain_tests {
    use super::super::super::{body, data::AuthorityContextData};
    use super::*;
    fn reference(kind: &str, prefix: &str) -> ExactRef {
        ExactRef {
            kind: kind.into(),
            id: format!("{prefix}{}", "1".repeat(64)),
            digest: format!("sha256:{}", "1".repeat(64)),
        }
    }
    fn context() -> DraftContextData {
        let binding = reference("workflow-binding", "wdoc_");
        DraftContextData {
            key: OperationKey {
                logical_store_id: "test-store".into(),
                scope: ["fixture".into(), "sandbox".into()],
                source: "test-source".into(),
                operation_id: "next-work".into(),
            },
            actor: "operator".into(),
            canonical_economic_namespace: "test-source".into(),
            installation_mode: "isolated_synthetic".into(),
            received_at: "2026-10-13T01:00:01.000000Z".into(),
            accepted_at: "2026-10-13T01:00:01.000000Z".into(),
            source_observation_inputs: vec![],
            authorities: vec![AuthorityContextData {
                source_slot: "test:worker".into(),
                enrollment_ref: reference("workflow-source-enrollment", "wdoc_"),
                current_state_ref: reference("workflow-source-state", "wsource_"),
                current_grant_ref: reference("workflow-source-grant", "wdoc_"),
                original_grant_refs: vec![],
                binding_ref: binding,
                accepted_agreement_ref: reference("workflow-accepted-agreement", "waccepted_"),
                activation_ref: reference("workflow-activation", "wactivation_"),
                authority_observation_ref: Some(reference(
                    "workflow-authority-prefix-observation",
                    "wdoc_",
                )),
            }],
        }
    }
    #[test]
    fn prior_chain_semantic_binding_and_clock_checks_accept_only_exact_data() {
        let c = context();
        let p = json!({"customer":"customer", "chain":"chain", "binding_ref":c.authorities[0].binding_ref});
        for variant in 0..4 {
            let mut m = CoreMeter::new(65536).unwrap();
            let customer = if variant == 1 { "other" } else { "customer" };
            let old_time = if variant == 2 {
                "2026-10-13T01:00:02.000000Z"
            } else {
                "2026-10-13T01:00:00.000000Z"
            };
            let link_root = body::semantic_id(&mut m, "", "workflow-link-set", &json!([])).unwrap();
            let id = body::semantic_id(
                &mut m,
                "wchain_",
                "workflow-chain-state",
                &json!([c.key.logical_store_id, c.key.scope, customer, "chain", "1"]),
            )
            .unwrap();
            let mut prior_key = c.key.clone();
            prior_key.operation_id = "previous-work".into();
            let old = json!({"schema":"ledger-workflow-chain-state/1","logical_store_id":c.key.logical_store_id,"scope":c.key.scope,"customer":customer,"chain":"chain","binding_ref":c.authorities[0].binding_ref,"accepted_agreement_ref":c.authorities[0].accepted_agreement_ref,"activation_ref":c.authorities[0].activation_ref,"revision":"1","admitted_events":"1","links_root":format!("sha256:{link_root}"),"operation_key":prior_key,"accepted_at":old_time});
            // The negative is a correctly framed/hash-rebound synthetic record,
            // not an authenticated original financial operation.
            let mut envelope =
                body::record(&mut m, &c.key, "workflow-chain-state", id, old.clone()).unwrap();
            if variant == 3 {
                envelope.reference.id = format!("wchain_{}", "2".repeat(64));
            }
            let result = chain_head(&mut m, &c, &p, &envelope.reference, &old);
            assert_eq!(
                result.is_ok(),
                variant == 0,
                "variant {variant}: {result:?}"
            );
        }
    }
}
