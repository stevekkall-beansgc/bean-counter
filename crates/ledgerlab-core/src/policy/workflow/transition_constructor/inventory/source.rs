// Closed operation names transcribed from pinned agreement-ready.schema.json SHA256 1f5764768505214a2b5c143b87a198d56671eef1363f9ed574b4b15c71c4943b
const OPERATIONS: &[&str] = &[
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
use super::super::{
    CoreMeter, body, data::*, meter, records::Records, representation::InvocationToken, shape,
};
use super::{check::*, evidence};
use crate::Result;
use serde_json::{Value, json};
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
/// The selected initial state requires exact original ceiling, not a successor
/// guessed from a hash. Later source revisions remain unsupported here.
pub(in crate::policy::workflow::transition_constructor) fn initial(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    enrollment: &Value,
    state: &Value,
    grant: &Value,
    customer: &Value,
) -> Result<Value> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::source::initial", |m| {
            initial_data(m, r, c, o, enrollment, state, grant, customer)
        });
    }
    #[cfg(not(test))]
    initial_data(m, r, c, o, enrollment, state, grant, customer)
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
) -> Result<Value> {
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
    let ceiling = invoke!(shape::field, m, enrollment, "ceiling")?;
    invoke!(shape::closed, m, ceiling, CEILING, &[])?;
    for k in CEILING {
        invoke!(copy, m, grant, k, ceiling, k)?;
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
        super::binding::ordered(m, a)?;
        invoke!(set, m, a)?;
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
        super::binding::ordered(m, a)?;
        invoke!(set, m, a)?;
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
            super::binding::ordered(m, names)?;
            invoke!(set, m, names)?;
            for name in names {
                m.visit("inventory.selector_name")?;
                let name = name
                    .as_str()
                    .ok_or_else(|| shape::fail("selector string"))?;
                invoke!(qualified, m, name)?;
            }
        }
    }
    let start = invoke!(time, m, ceiling, "starts_at")?;
    let end = invoke!(time, m, ceiling, "ends_at")?;
    if !invoke!(meter::cmp, m, &start, &end)?.is_lt() {
        return Err(shape::fail("ceiling interval"));
    }
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
    invoke!(
        eq,
        m,
        state,
        "enrollment_ref",
        &json!(c.authorities[0].enrollment_ref)
    )?;
    invoke!(
        eq,
        m,
        state,
        "current_grant_ref",
        &json!(c.authorities[0].current_grant_ref)
    )?;
    invoke!(
        eq,
        m,
        grant,
        "original_ceiling_ref",
        &json!(c.authorities[0].enrollment_ref)
    )?;
    invoke!(eq, m, grant, "revision", &json!("1"))?;
    invoke!(copy, m, grant, "grant_id", enrollment, "enrollment_id")?;
    invoke!(
        before,
        m,
        state,
        "accepted_at",
        &json!({"accepted_at":c.accepted_at}),
        "accepted_at"
    )?;
    invoke!(key, m, invoke!(shape::field, m, state, "operation_key")?, c)?;
    invoke!(copy, m, state, "actor", enrollment, "enrolled_by")?;
    invoke!(copy, m, ceiling, "principal_id", enrollment, "enrolled_by")?;
    invoke!(copy, m, state, "accepted_at", enrollment, "enrolled_at")?;
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
        before,
        m,
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
    invoke!(
        eq,
        m,
        &json!({"id":c.authorities[0].current_state_ref.id}),
        "id",
        &json!(expected)
    )?;
    Ok(registered)
}
