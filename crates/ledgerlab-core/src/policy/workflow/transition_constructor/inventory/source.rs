// Closed operation names transcribed from pinned agreement-ready.schema.json SHA256 1f5764768505214a2b5c143b87a198d56671eef1363f9ed574b4b15c71c4943b
pub(in crate::policy::workflow::transition_constructor) const OPERATIONS: &[&str] = &[
    "installation.initialize",
    "installation.upgrade",
    "evidence.retain",
    "policy.publish",
    "agreement.accept",
    "agreement.activate",
    "agreement.revoke",
    "authority.change",
    "invocation.authorize",
    "invocation.close",
    "work.accept",
    "link.assert",
    "outcome.submit",
    "outcome.correct",
    "outcome.close",
    "work.reverse",
    "term.set",
    "cumulative_agreement.set",
    "activity.submit",
    "quantity.correct",
    "recurrence.set",
    "recurrence.cancel",
    "occurrence.accept",
    "fiscal.set",
    "period.resolve",
    "period.close",
    "statement.adjustments.issue",
    "settlement.close",
    "cost.finalize",
    "cost.correct",
    "connector.configure",
    "connector.pause",
    "connector.reconcile",
    "connector.resume",
    "dispatch.hold",
    "dispatch.resume",
    "settlement.dispatch",
    "delivery.enqueue",
    "backup.create",
    "backup.retrieve",
    "backup.verify",
    "backup.prune",
    "restore.stage",
    "restore.verify",
    "restore.activate",
    "restore.resume",
    "meter.ingest",
    "meter.finalize",
    "meter.correct",
    "service.rate.accept",
    "deployment.apply",
    "deployment.pause",
    "deployment.resume",
    "deployment.export",
    "deployment.offboard",
    "credential.issue",
    "credential.rotate",
    "credential.revoke",
    "producer.enroll",
    "producer.revoke",
    "provider.observation.record",
    "lifecycle.target.close",
    "recovery.bundle.import",
    "settlement.credit.allocate",
];
use super::super::record_body::RecordBody;
use super::super::{
    body, data::*, meter, records::Records, representation::InvocationToken, shape, CoreMeter,
};
use super::{check::*, evidence};
use crate::Result;
use serde_json::{json, Value};
fn ordered_unique(m: &mut CoreMeter, values: &[Value]) -> Result<()> {
    super::binding::ordered(m, values)?;
    // Strict canonical byte order already proves pairwise distinctness on
    // these selected JSON values. Repeating all pairs cannot add a valid
    // rejection: an equal pair cannot occur anywhere in a strictly increasing
    // sequence. Preserve the original v1 paid schedule at its old entry.
    if m.resource_vector_usage().is_none() {
        set(m, values)?;
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn current_header(
    m: &mut CoreMeter,
    c: &DraftContextData,
    state: &Value,
) -> Result<()> {
    invoke!(key, m, data_field!(m, state, "operation_key"), c)?;
    invoke!(name, m, state, "actor")
}
pub(in crate::policy::workflow::transition_constructor) fn current_sets(
    m: &mut CoreMeter,
    grant: &Value,
) -> Result<()> {
    for (field, minimum, maximum) in [
        ("customer_scope", 1, 64),
        ("operations", 1, 64),
        ("source_slots", 0, 32),
        ("permissions", 1, 8),
    ] {
        let values = invoke!(shape::array, m, grant, field)?;
        bounds(m, values.len(), minimum, maximum)?;
        ordered_unique(m, values)?;
    }
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) fn selector_subset(
    m: &mut CoreMeter,
    narrowed: &[Value],
    ceiling: &[Value],
    maximum: usize,
) -> Result<()> {
    bounds(m, narrowed.len(), 0, maximum)?;
    ordered_unique(m, narrowed)?;
    for selected in narrowed {
        m.visit("grant.narrowed_selector")?;
        // The selected original profile admits explicit name selectors. A
        // namespace cannot be introduced by narrowing those immutable names.
        shape::closed(m, selected, &["names"], &[])?;
        let names = invoke!(shape::array, m, selected, "names")?;
        bounds(m, names.len(), 1, 64)?;
        ordered_unique(m, names)?;
        for name in names {
            let value = name
                .as_str()
                .ok_or_else(|| shape::fail("grant selector name"))?;
            qualified(m, value)?;
            let mut covered = false;
            for original in ceiling {
                m.visit("grant.original_selector")?;
                shape::closed(m, original, &["names"], &[])?;
                if invoke!(
                    shape::member,
                    m,
                    invoke!(shape::array, m, original, "names")?,
                    name
                )? {
                    covered = true;
                }
            }
            if !covered {
                return Err(shape::fail("current selector widens immutable ceiling"));
            }
        }
    }
    Ok(())
}
const CEILING: &[&str] = &[
    "principal_id",
    "source",
    "canonical_economic_namespace",
    "customer_scope",
    "operations",
    "product_selectors",
    "activity_selectors",
    "link_selectors",
    "source_slots",
    "permissions",
    "starts_at",
    "ends_at",
];

#[cfg(test)]
mod ordered_source_tests {
    use super::*;
    use crate::{canonical::audit_arena::AuditArena, validation_work::OriginalAuditSession};
    #[test]
    fn strict_order_preserves_selector_rules_without_repeating_all_pairs() {
        // Exact legal64-name ceiling/regrouping from the existing schema.
        // This proves only this changed selector operation, not a complete
        // grant/source/financial envelope or a future-admission certificate.
        let names: Vec<Value> = (0..64).map(|i| json!(format!("test:item{i:02}"))).collect();
        let ceiling = vec![json!({"names":names})];
        let narrowed: Vec<Value> = names.iter().map(|name| json!({"names":[name]})).collect();
        let mut old = CoreMeter::new(65536).unwrap();
        assert_eq!(
            selector_subset(&mut old, &narrowed, &ceiling, 64)
                .unwrap_err()
                .code,
            "PROGRAM_BUDGET_EXCEEDED"
        );
        for case in 0..3 {
            let arena = AuditArena::new().unwrap();
            let mut session = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let mut m = session.meter_mut().original_economic_meter().unwrap();
            m.enter_draft().unwrap();
            let mut actual = narrowed.clone();
            if case == 1 {
                actual[63] = actual[62].clone();
            }
            if case == 2 {
                actual[63] = json!({"names":["test:outside"]});
            }
            let result = selector_subset(&mut m, &actual, &ceiling, 64);
            if case == 0 {
                result.unwrap();
                session.meter_mut().finish_original_economic(m).unwrap();
            } else {
                assert!(result.is_err());
            }
        }
    }
}
/// Validate the exact initial birth; later current heads use this immutable
/// original independently, never substitute their narrowed grant as the birth.
pub(in crate::policy::workflow::transition_constructor) fn initial(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    enrollment: &Value,
    state: &Value,
    grant: &Value,
    customer: &Value,
) -> Result<RecordBody> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::source::initial", |m| {
            initial_data(
                m,
                r,
                c,
                o,
                enrollment,
                state,
                grant,
                customer,
                &c.authorities[0].enrollment_ref,
                &c.authorities[0].current_state_ref,
                &c.authorities[0].current_grant_ref,
                false,
            )
        });
    }
    #[cfg(not(test))]
    initial_data(
        m,
        r,
        c,
        o,
        enrollment,
        state,
        grant,
        customer,
        &c.authorities[0].enrollment_ref,
        &c.authorities[0].current_state_ref,
        &c.authorities[0].current_grant_ref,
        false,
    )
}
/// Exact initial birth operands remain separate from a later current source
/// head. All supplied records still require completed origin auditing in host.
pub(in crate::policy::workflow::transition_constructor) fn birth(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    enrollment: &Value,
    state: &Value,
    grant: &Value,
    customer: &Value,
    enrollment_ref: &ExactRef,
    state_ref: &ExactRef,
    grant_ref: &ExactRef,
) -> Result<RecordBody> {
    let checked = r.source_birth_facts(
        m,
        c,
        enrollment,
        state,
        grant,
        customer,
        enrollment_ref,
        state_ref,
        grant_ref,
    )?;
    if let Some(registered) = checked.registered {
        return Ok(registered);
    }
    let registered = initial_data(
        m,
        r,
        c,
        o,
        enrollment,
        state,
        grant,
        customer,
        enrollment_ref,
        state_ref,
        grant_ref,
        checked.outputs,
    )?;
    r.remember_source_birth(m, c, &checked, &registered)?;
    Ok(registered)
}
fn initial_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    enrollment: &Value,
    state: &Value,
    grant: &Value,
    customer: &Value,
    enrollment_ref: &ExactRef,
    state_ref: &ExactRef,
    grant_ref: &ExactRef,
    produced: bool,
) -> Result<RecordBody> {
    if !produced {
        invoke!(
            shape::closed,
            m,
            enrollment,
            &[
                "schema",
                "scope",
                "enrollment_id",
                "ceiling",
                "customer_refs",
                "evidence_ref",
                "authority_ref",
                "installation_mode",
                "enrolled_by",
                "enrolled_at"
            ],
            &[]
        )?;
        let mut grant_fields = CEILING.to_vec();
        grant_fields.extend(["schema", "grant_id", "revision", "original_ceiling_ref"]);
        shape::closed(m, grant, &grant_fields, &[])?;
    }
    let ceiling = invoke!(shape::field, m, enrollment, "ceiling")?;
    invoke!(shape::closed, m, ceiling, CEILING, &[])?;
    if !produced {
        for k in CEILING {
            invoke!(copy, m, grant, k, ceiling, k)?;
        }
    }
    invoke!(opaque, m, ceiling, "principal_id", 128)?;
    for k in ["source", "canonical_economic_namespace"] {
        invoke!(opaque, m, ceiling, k, 256)?;
    }
    for k in [
        "customer_scope",
        "operations",
        "source_slots",
        "permissions",
    ] {
        let a = invoke!(shape::array, m, ceiling, k)?;
        let (min, max) = match k {
            "customer_scope" | "operations" => (1, 64),
            "source_slots" => (0, 32),
            _ => (1, 8),
        };
        invoke!(bounds, m, a.len(), min, max)?;
        ordered_unique(m, a)?;
        for v in a {
            m.visit("inventory.set_value")?;
            let text = v.as_str().ok_or_else(|| shape::fail("source set string"))?;
            match k {
                "source_slots" => invoke!(qualified, m, text)?,
                "permissions" => invoke!(
                    one_of,
                    m,
                    text,
                    &[
                        "read",
                        "submit",
                        "correct",
                        "link",
                        "admin",
                        "invocation",
                        "close",
                        "operate"
                    ]
                )?,
                "operations" => invoke!(one_of, m, text, OPERATIONS)?,
                _ => invoke!(opaque, m, &json!({"x":text}), "x", 128)?,
            }
        }
    }
    for k in ["product_selectors", "activity_selectors", "link_selectors"] {
        let a = invoke!(shape::array, m, ceiling, k)?;
        invoke!(
            bounds,
            m,
            a.len(),
            0,
            if k == "link_selectors" { 32 } else { 64 }
        )?;
        ordered_unique(m, a)?;
        for selector in a {
            m.visit("inventory.selector")?;
            if selector.get("namespace").is_some() {
                return Err(crate::Error::new(
                    "UNSUPPORTED_COMPOSITION",
                    "namespace selector DATA relation",
                ));
            }
            invoke!(shape::closed, m, selector, &["names"], &[])?;
            let names = invoke!(shape::array, m, selector, "names")?;
            invoke!(bounds, m, names.len(), 1, 64)?;
            ordered_unique(m, names)?;
            for name in names {
                m.visit("inventory.selector_name")?;
                let name = name
                    .as_str()
                    .ok_or_else(|| shape::fail("selector string"))?;
                invoke!(qualified, m, name)?;
            }
        }
    }
    let start = invoke!(time_owned, m, o, ceiling, "starts_at")?;
    let end = invoke!(time_owned, m, o, ceiling, "ends_at")?;
    if !invoke!(meter::cmp, m, &start, &end)?.is_lt() {
        return Err(shape::fail("ceiling interval"));
    }
    if !produced {
        invoke!(
            shape::closed,
            m,
            state,
            &[
                "schema",
                "enrollment_ref",
                "revision",
                "status",
                "current_grant_ref",
                "operation_key",
                "actor",
                "accepted_at"
            ],
            &[]
        )?;
        invoke!(eq, m, state, "revision", &json!("1"))?;
        invoke!(eq, m, state, "status", &json!("active"))?;
        invoke!(eq, m, state, "enrollment_ref", &json!(enrollment_ref))?;
        invoke!(eq, m, state, "current_grant_ref", &json!(grant_ref))?;
        invoke!(eq, m, grant, "original_ceiling_ref", &json!(enrollment_ref))?;
        invoke!(eq, m, grant, "revision", &json!("1"))?;
        invoke!(copy, m, grant, "grant_id", enrollment, "enrollment_id")?;
    }
    invoke!(
        before_owned,
        m,
        o,
        state,
        "accepted_at",
        &json!({"accepted_at":c.accepted_at}),
        "accepted_at"
    )?;
    invoke!(key, m, invoke!(shape::field, m, state, "operation_key")?, c)?;
    if !produced {
        invoke!(copy, m, state, "actor", enrollment, "enrolled_by")?;
    }
    invoke!(copy, m, ceiling, "principal_id", enrollment, "enrolled_by")?;
    if !produced {
        invoke!(copy, m, state, "accepted_at", enrollment, "enrolled_at")?;
    }
    invoke!(
        eq,
        m,
        enrollment,
        "installation_mode",
        &json!(c.installation_mode)
    )?;
    invoke!(eq, m, enrollment, "scope", &json!(c.key.scope))?;
    invoke!(name, m, enrollment, "enrollment_id")?;
    let customers = invoke!(shape::array, m, enrollment, "customer_refs")?;
    invoke!(count, m, customers, 1)?;
    let registered = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        &customers[0],
        "workflow-customer",
        "ledger-workflow-customer-registration/1"
    )?;
    invoke!(
        shape::closed,
        m,
        &registered,
        &[
            "schema",
            "scope",
            "customer",
            "acceptor",
            "authority_ref",
            "evidence_ref",
            "installation_mode",
            "registered_by",
            "registered_at"
        ],
        &[]
    )?;
    invoke!(eq, m, &registered, "customer", customer)?;
    invoke!(eq, m, &registered, "scope", &json!(c.key.scope))?;
    invoke!(
        eq,
        m,
        &registered,
        "installation_mode",
        &json!(c.installation_mode)
    )?;
    invoke!(name, m, &registered, "acceptor")?;
    invoke!(
        before_owned,
        m,
        o,
        &registered,
        "registered_at",
        enrollment,
        "enrolled_at"
    )?;
    let evidence = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        invoke!(shape::field, m, &registered, "evidence_ref")?,
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1"
    )?;
    invoke!(
        evidence::retained,
        m,
        &mut *r,
        c,
        o,
        &evidence,
        "customer_registration",
        &json!({"customer":customer}),
        &json!({"acceptor":data_field!(m,&registered,"acceptor")}),
        &registered,
        "registered_at"
    )?;

    let source_evidence = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        invoke!(shape::field, m, enrollment, "evidence_ref")?,
        "workflow-evidence",
        "ledger-workflow-retained-evidence/1"
    )?;
    invoke!(
        copy,
        m,
        &source_evidence,
        "retained_by",
        enrollment,
        "enrolled_by"
    )?;
    invoke!(
        evidence::retained,
        m,
        &mut *r,
        c,
        o,
        &source_evidence,
        "source_enrollment",
        &json!({"customer":customer}),
        &json!({"principal_id":data_field!(m,&ceiling,"principal_id"),"source":data_field!(m,&ceiling,"source")}),
        enrollment,
        "enrolled_at"
    )?;

    invoke!(
        evidence::root,
        m,
        &mut *r,
        c,
        o,
        data_field!(m, &registered, "authority_ref"),
        &registered,
        "registered_at",
        data_field!(m, &registered, "registered_by"),
        None
    )?;
    invoke!(
        evidence::root,
        m,
        &mut *r,
        c,
        o,
        data_field!(m, enrollment, "authority_ref"),
        enrollment,
        "enrolled_at",
        data_field!(m, enrollment, "enrolled_by"),
        None
    )?;
    if !produced {
        let expected = invoke!(
            body::semantic_id,
            m,
            "wsource_",
            "workflow-source-state",
            &json!([
                c.key.logical_store_id,
                c.key.scope,
                data_field!(m, &enrollment, "enrollment_id"),
                "1"
            ])
        )?;
        invoke!(eq, m, &json!({"id":state_ref.id}), "id", &json!(expected))?;
    }
    Ok(registered)
}
