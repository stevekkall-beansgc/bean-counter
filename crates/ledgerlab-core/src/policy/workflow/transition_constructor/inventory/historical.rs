use super::super::super::selected_program::SelectedProgram;
use super::super::{
    data::*, meter, records::Records, representation::InvocationToken, shape, CoreMeter,
};
use super::{check::*, evidence, source};
use crate::{Error, Result};
use serde_json::{json, Value};
fn selector(m: &mut CoreMeter, g: &Value, k: &str, want: &Value) -> Result<()> {
    let mut found = false;
    for s in invoke!(shape::array, m, g, k)? {
        m.visit("inventory.historical_selector")?;
        invoke!(shape::closed, m, s, &["names"], &[])?;
        if invoke!(
            shape::member,
            m,
            invoke!(shape::array, m, s, "names")?,
            want
        )? {
            found = true;
        }
    }
    if !found {
        return Err(shape::fail("historical bound selector"));
    }
    Ok(())
}
/// Captured initial source DATA is evaluated at the original operation's time,
/// independent of the current work caller's grant-time checks. Wider lineages
/// remain explicit unsupported branches, never a Ref-equality substitution.
pub(super) fn source_and_root(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    program: &SelectedProgram,
    b: &Value,
    selected_operands: (&Value, &Value, &Value),
) -> Result<()> {
    let (auth, key, at) = selected_operands;
    #[cfg(test)]
    {
        m.diagnostic_scope("inventory::historical::source_and_root", |m| {
            source_and_root_data(m, r, c, o, program, b, (auth, key, at))
        })
    }
    #[cfg(not(test))]
    source_and_root_data(m, r, c, o, program, b, (auth, key, at))
}
fn source_and_root_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    program: &SelectedProgram,
    b: &Value,
    selected_operands: (&Value, &Value, &Value),
) -> Result<()> {
    let (auth, key, at) = selected_operands;
    let sr = invoke!(shape::array, m, auth, "source_state_refs")?;
    invoke!(count, m, sr, 1)?;
    let state = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        &sr[0],
        "workflow-source-state",
        "ledger-workflow-source-state/1"
    )?;
    invoke!(copy, m, auth, "source_revision", &state, "revision")?;
    if !invoke!(
        shape::same,
        m,
        data_field!(m, &state, "revision"),
        &json!("1")
    )? {
        return Err(Error::new(
            "UNSUPPORTED_COMPOSITION",
            "historical successor source lineage DATA",
        ));
    }
    let enrollment = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        data_field!(m, &state, "enrollment_ref"),
        "workflow-source-enrollment",
        "ledger-workflow-source-enrollment/1"
    )?;
    invoke!(
        eq,
        m,
        &state,
        "enrollment_ref",
        &json!(c.authorities[0].enrollment_ref)
    )?;
    invoke!(copy, m, auth, "source_revision", &state, "revision")?;
    let gr = invoke!(shape::array, m, auth, "source_grant_refs")?;
    invoke!(count, m, gr, 1)?;
    invoke!(eq, m, b, "authority_refs", &json!(gr))?;
    invoke!(
        shape::equal,
        m,
        &json!(gr),
        &json!(c.authorities[0].original_grant_refs)
    )?;
    invoke!(eq, m, &state, "current_grant_ref", &gr[0])?;
    let grant = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        &gr[0],
        "workflow-source-grant",
        "ledger-workflow-source-grant/1"
    )?;
    let mut captured = c.clone();
    captured.accepted_at = at
        .as_str()
        .ok_or_else(|| shape::fail("captured authority time"))?
        .to_owned();
    captured.authorities[0].current_state_ref = invoke!(shape::reference, m, &sr[0])?;
    captured.authorities[0].current_grant_ref = invoke!(shape::reference, m, &gr[0])?;
    invoke!(
        source::initial,
        m,
        &mut *r,
        &captured,
        o,
        &enrollment,
        &state,
        (&*grant, data_field!(m, b, "customer"))
    )?;
    let captured_time = invoke!(time, m, &json!({"at":at}), "at")?;
    let start = invoke!(time, m, &grant, "starts_at")?;
    let end = invoke!(time, m, &grant, "ends_at")?;
    if invoke!(meter::cmp, m, &captured_time, &start)?.is_lt()
        || !invoke!(meter::cmp, m, &captured_time, &end)?.is_lt()
    {
        return Err(shape::fail("historical grant interval"));
    }
    invoke!(
        shape::contains,
        m,
        invoke!(shape::array, m, &grant, "customer_scope")?,
        data_field!(m, b, "customer")
    )?;
    let mut product = None;
    for d in program.source_documents() {
        m.visit("inventory.original_product")?;
        if invoke!(
            shape::same,
            m,
            &json!(d.reference()),
            data_field!(m, b, "product_ref")
        )? {
            product = Some(d.canonical_value());
        }
    }
    let product = product.ok_or_else(|| shape::fail("historical original product"))?;
    invoke!(
        selector,
        m,
        &grant,
        "product_selectors",
        data_field!(m, product, "product_id")
    )?;
    for binding in invoke!(shape::array, m, b, "bindings")? {
        m.visit("inventory.original_binding_slot")?;
        let terms_ref =
            super::super::representation::ParsedRef::parse(m, o, &binding["binding_ref"])?;
        let terms = r.get_parsed_schemas(
            m,
            o,
            &c.key,
            &terms_ref,
            "workflow-slot-terms",
            &[
                "ledger-workflow-slot-binding-terms/2",
                "ledger-workflow-slot-binding-terms/3",
            ],
        )?;
        invoke!(eq, m, &terms, "source_grant_refs", &json!(gr))?;
        for activity_name in invoke!(shape::array, m, &terms, "allowed_activities")? {
            m.visit("inventory.bound_activity")?;
            invoke!(selector, m, &grant, "activity_selectors", activity_name)?;
            let activity = invoke!(
                shape::named,
                m,
                invoke!(shape::array, m, product, "activities")?,
                "name",
                activity_name
                    .as_str()
                    .ok_or_else(|| shape::fail("activity name"))?
            )?;
            invoke!(
                shape::contains,
                m,
                invoke!(shape::array, m, &grant, "source_slots")?,
                data_field!(m, activity, "source_slot")
            )?;
            let primitive = data_field!(m, activity, "primitive");
            let (operations, rights): (&[&str], &[&str]) =
                if invoke!(shape::same, m, primitive, &json!("work"))? {
                    (
                        &["work.accept", "quantity.correct", "work.reverse"],
                        &["read", "submit", "correct"],
                    )
                } else if invoke!(shape::same, m, primitive, &json!("outcome"))? {
                    (
                        &["outcome.submit", "outcome.correct", "outcome.close"],
                        &["read", "submit", "correct", "close"],
                    )
                } else {
                    return Err(Error::new(
                        "UNSUPPORTED_COMPOSITION",
                        "historical bound primitive",
                    ));
                };
            for op in operations {
                invoke!(
                    shape::contains,
                    m,
                    invoke!(shape::array, m, &grant, "operations")?,
                    &json!(op)
                )?;
            }
            for right in rights {
                invoke!(
                    shape::contains,
                    m,
                    invoke!(shape::array, m, &grant, "permissions")?,
                    &json!(right)
                )?;
            }
        }
    }
    let root = invoke!(
        evidence::root,
        m,
        &mut *r,
        c,
        o,
        data_field!(m, auth, "local_admin_ref"),
        &json!({"at":at}),
        (
            "at",
            data_field!(m, auth, "actor"),
            Some(data_field!(m, key, "source"))
        )
    )?;
    invoke!(
        copy,
        m,
        data_field!(m, &root, "basis"),
        "canonical_economic_namespace",
        b,
        "source_namespace"
    )?;
    Ok(())
}
