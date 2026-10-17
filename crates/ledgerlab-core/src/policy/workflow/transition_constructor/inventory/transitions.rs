use super::super::record_body::RecordBody;
use super::super::{
    CoreMeter, body, data::*, meter, records::Records, representation::InvocationToken, shape,
};
use super::check::*;
use crate::{Error, Result};
use serde_json::{Value, json};
pub(super) fn initial(m: &mut CoreMeter, h: &Value, kind: &str) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        h,
        &[
            "schema",
            "scope",
            "customer",
            "agreement_id",
            "operation_key",
            "actor",
            "head_kind",
            "old_revision",
            "new_revision",
            "accepted_binding_ref",
            "resolved_effective_time",
            "accepted_at",
            "commit_sequence"
        ],
        &[]
    )?;
    invoke!(eq, m, h, "head_kind", &json!(kind))?;
    invoke!(eq, m, h, "old_revision", &json!("0"))?;
    invoke!(eq, m, h, "new_revision", &json!("1"))?;
    invoke!(number, m, h, "commit_sequence", true)?;
    Ok(())
}
pub(super) fn operation(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    k: &Value,
    kind: &str,
) -> Result<RecordBody> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::transitions::operation", |m| {
            operation_data(m, r, c, k, kind, false)
        });
    }
    #[cfg(not(test))]
    operation_data(m, r, c, k, kind, false)
}
fn operation_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    k: &Value,
    kind: &str,
    family_close_acceptance: bool,
) -> Result<RecordBody> {
    invoke!(key, m, k, c)?;
    let id = invoke!(body::semantic_id, m, "wop_", "workflow-operation", k)?;
    let (_, op) = r.by_identity(
        m,
        &c.key,
        "workflow-operation",
        &id,
        "ledger-workflow-operation-record/1",
    )?;
    invoke!(
        shape::closed,
        m,
        &op,
        &[
            "schema",
            "key",
            "kind",
            "contract",
            "request_evidence",
            "authority_snapshot",
            "accepted_at",
            "commit_sequence",
            "result_refs"
        ],
        &[]
    )?;
    invoke!(eq, m, &op, "key", k)?;
    invoke!(eq, m, &op, "kind", &json!(kind))?;
    invoke!(eq, m, &op, "contract", &json!("workflow/1"))?;
    invoke!(number, m, &op, "commit_sequence", true)?;
    let evidence = invoke!(shape::field, m, &op, "request_evidence")?;
    invoke!(
        shape::closed,
        m,
        evidence,
        &[
            "schema",
            "normalized_request",
            "descriptor_resolution",
            "normalization_profile",
            "original_request_bytes_base64",
            "original_request_bytes_sha256",
            "request_hash",
            "schema_bundle_digest"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        evidence,
        "schema",
        &json!("ledger-operation-request-evidence/2")
    )?;
    invoke!(
        eq,
        m,
        evidence,
        "normalization_profile",
        &json!("ledger-operation-normalization/2")
    )?;
    let envelope = invoke!(shape::field, m, evidence, "normalized_request")?;
    invoke!(
        shape::closed,
        m,
        envelope,
        &["schema", "key", "kind", "contract", "payload"],
        &[]
    )?;
    invoke!(eq, m, envelope, "schema", &json!("ledger-operation/1"))?;
    for key in ["key", "kind", "contract"] {
        invoke!(copy, m, envelope, key, &op, key)?;
    }
    let payload_schema = invoke!(
        shape::text,
        m,
        data_field!(m, envelope, "payload"),
        "schema"
    )?;
    if family_close_acceptance {
        super::request::validate_family_close_acceptance(m,evidence,kind,payload_schema)?;
    } else {super::request::validate(m,evidence,kind,payload_schema)?;}

    invoke!(set, m, invoke!(shape::array, m, &op, "result_refs")?)?;
    Ok(op)
}
pub(super) fn admission(
    m: &mut CoreMeter,
    auth: &Value,
    key: &Value,
    b: &Value,
    at: &Value,
    actor: &Value,
    kind: &str,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::transitions::admission", |m| {
            admission_data(m, auth, key, b, at, actor, kind)
        });
    }
    #[cfg(not(test))]
    admission_data(m, auth, key, b, at, actor, kind)
}
fn admission_data(
    m: &mut CoreMeter,
    auth: &Value,
    key: &Value,
    b: &Value,
    at: &Value,
    actor: &Value,
    kind: &str,
) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        auth,
        &[
            "schema",
            "purpose",
            "operation",
            "actor",
            "source",
            "scope",
            "customer",
            "agreement_id",
            "local_admin_ref",
            "canonical_economic_namespace",
            "source_revision",
            "source_state_refs",
            "source_grant_refs",
            "assent_refs",
            "observed_at"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        auth,
        "schema",
        &json!("ledger-workflow-agreement-authority/2")
    )?;
    invoke!(eq, m, auth, "purpose", &json!("admit"))?;
    invoke!(eq, m, auth, "operation", &json!(kind))?;
    invoke!(eq, m, auth, "actor", actor)?;
    invoke!(eq, m, auth, "observed_at", at)?;
    invoke!(copy, m, auth, "source", key, "source")?;
    invoke!(copy, m, auth, "scope", key, "scope")?;
    for k in ["customer", "agreement_id", "assent_refs"] {
        invoke!(copy, m, auth, k, b, k)?;
    }
    invoke!(
        copy,
        m,
        auth,
        "canonical_economic_namespace",
        b,
        "source_namespace"
    )?;
    invoke!(number, m, auth, "source_revision", true)?;
    invoke!(
        count,
        m,
        invoke!(shape::array, m, auth, "source_state_refs")?,
        1
    )?;
    invoke!(
        count,
        m,
        invoke!(shape::array, m, auth, "source_grant_refs")?,
        1
    )?;
    Ok(())
}
fn head_id(m: &mut CoreMeter, c: &DraftContextData, h: &Value, reference: &Value) -> Result<()> {
    let id = invoke!(
        body::semantic_id,
        m,
        "whead_",
        "workflow-head-transition",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            data_field!(m, &h, "customer"),
            data_field!(m, &h, "agreement_id"),
            data_field!(m, &h, "head_kind"),
            data_field!(m, &h, "new_revision")
        ])
    )?;
    invoke!(eq, m, reference, "id", &json!(id))
}
/// Closed initial birth DATA. Broader schedule and authenticated origin audit
/// remain separate; an alias operation cannot substitute the deterministic birth.
pub(in crate::policy::workflow::transition_constructor) fn validate(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    program: &super::super::super::selected_program::SelectedProgram,
    b: &Value,
    a: &Value,
    t: &Value,
    h: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::transitions::validate", |m| {
            validate_data(m, r, c, o, program, b, a, t, h)
        });
    }
    #[cfg(not(test))]
    validate_data(m, r, c, o, program, b, a, t, h)
}
fn validate_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    program: &super::super::super::selected_program::SelectedProgram,
    b: &Value,
    a: &Value,
    t: &Value,
    h: &Value,
) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        a,
        &[
            "schema",
            "scope",
            "customer",
            "agreement_id",
            "version",
            "publication_ref",
            "binding_ref",
            "terms_ref",
            "terms_digest",
            "authority_observation_ref",
            "operation_key",
            "accepted_at",
            "commit_sequence"
        ],
        &[]
    )?;
    invoke!(key, m, &data_field!(m, &a, "operation_key"), c)?;
    invoke!(number, m, a, "version", true)?;
    invoke!(eq, m, a, "version", &json!("1"))?;
    invoke!(copy, m, a, "version", b, "version")?;
    invoke!(number, m, a, "commit_sequence", true)?;
    invoke!(copy, m, a, "accepted_at", b, "accepted_at")?;
    let aid = invoke!(
        body::semantic_id,
        m,
        "waccepted_",
        "workflow-agreement-acceptance",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            data_field!(m, &a, "customer"),
            data_field!(m, &a, "agreement_id"),
            data_field!(m, &a, "version")
        ])
    )?;
    invoke!(
        eq,
        m,
        &json!(c.authorities[0].accepted_agreement_ref),
        "id",
        &json!(aid)
    )?;
    invoke!(
        shape::closed,
        m,
        t,
        &[
            "schema",
            "action",
            "head_transition_ref",
            "binding_ref",
            "effective_at",
            "commit_sequence",
            "accepted_at"
        ],
        &[]
    )?;
    invoke!(initial, m, h, "agreement_activation")?;
    invoke!(eq, m, t, "action", &json!("activate"))?;
    invoke!(key, m, &data_field!(m, &h, "operation_key"), c)?;
    invoke!(head_id, m, c, h, &data_field!(m, &t, "head_transition_ref"))?;
    for k in ["customer", "agreement_id"] {
        invoke!(copy, m, h, k, b, k)?;
    }
    invoke!(eq, m, h, "scope", &json!(c.key.scope))?;
    invoke!(copy, m, h, "accepted_binding_ref", a, "binding_ref")?;
    invoke!(copy, m, h, "resolved_effective_time", t, "effective_at")?;
    for k in ["accepted_at", "commit_sequence"] {
        invoke!(copy, m, h, k, t, k)?;
    }
    let tid = invoke!(
        body::semantic_id,
        m,
        "wactivation_",
        "workflow-activation-transition",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            data_field!(m, &b, "customer"),
            data_field!(m, &b, "agreement_id"),
            data_field!(m, &h, "new_revision")
        ])
    )?;
    invoke!(
        eq,
        m,
        &json!(c.authorities[0].activation_ref),
        "id",
        &json!(tid)
    )?;
    let aseq = invoke!(number, m, a, "commit_sequence", true)?;
    let tseq = invoke!(number, m, t, "commit_sequence", true)?;
    if !invoke!(meter::cmp, m, &aseq, &tseq)?.is_lt() {
        return Err(shape::fail("acceptance before activation sequence"));
    }
    invoke!(before, m, a, "accepted_at", t, "accepted_at")?;
    invoke!(
        before,
        m,
        t,
        "accepted_at",
        &json!({"at":c.accepted_at}),
        "at"
    )?;
    invoke!(
        before,
        m,
        t,
        "effective_at",
        &json!({"at":c.accepted_at}),
        "at"
    )?;
    let activate = invoke!(
        operation,
        m,
        &mut *r,
        c,
        &data_field!(m, &h, "operation_key"),
        "agreement.activate"
    )?;
    for k in ["accepted_at", "commit_sequence"] {
        invoke!(copy, m, &activate, k, t, k)?;
    }
    let mut intent_ref = None;
    for rr in invoke!(shape::array, m, &activate, "result_refs")? {
        if invoke!(
            shape::same,
            m,
            &data_field!(m, &rr, "kind"),
            &json!("workflow-agreement-intent")
        )? {
            if intent_ref.is_some() {
                return Err(shape::fail("multiple activation intents"));
            }
            intent_ref = Some(rr.clone());
        }
    }
    invoke!(
        count,
        m,
        invoke!(shape::array, m, &activate, "result_refs")?,
        3
    )?;
    invoke!(
        shape::contains,
        m,
        invoke!(shape::array, m, &activate, "result_refs")?,
        &data_field!(m, &t, "head_transition_ref")
    )?;
    invoke!(
        shape::contains,
        m,
        invoke!(shape::array, m, &activate, "result_refs")?,
        &json!(c.authorities[0].activation_ref)
    )?;
    let intent = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        &intent_ref.ok_or_else(|| shape::fail("activation intent"))?,
        "workflow-agreement-intent",
        "ledger-workflow-agreement-intent/1"
    )?;
    invoke!(
        shape::closed,
        m,
        &intent,
        &["schema", "kind", "payload"],
        &[]
    )?;
    invoke!(eq, m, &intent, "kind", &json!("agreement.activate"))?;
    let payload = &data_field!(
        m,
        &activate,
        "request_evidence",
        "normalized_request",
        "payload"
    );
    invoke!(
        copy,
        m,
        &intent,
        "payload",
        &json!({"payload":payload}),
        "payload"
    )?;
    invoke!(
        shape::closed,
        m,
        payload,
        &[
            "schema",
            "agreement_id",
            "accepted_binding_ref",
            "policy_ref",
            "product_ref",
            "fact_schema_refs",
            "compiled_plan_ref",
            "expected_revision",
            "effective",
            "evidence_refs"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        payload,
        "schema",
        &json!("ledger-workflow-activation/1")
    )?;
    for k in [
        "agreement_id",
        "policy_ref",
        "product_ref",
        "fact_schema_refs",
        "compiled_plan_ref",
    ] {
        invoke!(copy, m, payload, k, b, k)?;
    }
    invoke!(copy, m, payload, "accepted_binding_ref", a, "binding_ref")?;
    invoke!(copy, m, payload, "expected_revision", h, "old_revision")?;
    // Nonempty supplemental evidence requires its own selected metadata recipe.
    let supplemental = invoke!(shape::array, m, payload, "evidence_refs")?;
    if !meter::cmp(m, &supplemental.len().into(), &0usize.into())?.is_eq() {
        return Err(Error::new(
            "UNSUPPORTED_COMPOSITION",
            "activation supplemental evidence DATA recipe",
        ));
    }
    let effective = &data_field!(m, &payload, "effective");
    invoke!(shape::closed, m, effective, &["kind"], &["not_before"])?;
    if invoke!(
        shape::same,
        m,
        &data_field!(m, &effective, "kind"),
        &json!("at_commit")
    )? {
        invoke!(shape::closed, m, effective, &["kind"], &[])?;
        invoke!(copy, m, t, "effective_at", &activate, "accepted_at")?;
    } else if invoke!(
        shape::same,
        m,
        &data_field!(m, &effective, "kind"),
        &json!("not_before")
    )? {
        invoke!(shape::closed, m, effective, &["kind", "not_before"], &[])?;
        invoke!(copy, m, t, "effective_at", effective, "not_before")?;
        invoke!(before, m, &activate, "accepted_at", effective, "not_before")?;
    } else {
        return Err(shape::fail("activation effective variant"));
    }
    invoke!(
        admission,
        m,
        &data_field!(m, &activate, "authority_snapshot"),
        &data_field!(m, &h, "operation_key"),
        b,
        &data_field!(m, &activate, "accepted_at"),
        &data_field!(m, &h, "actor"),
        "agreement.activate"
    )?;
    invoke!(
        super::historical::source_and_root,
        m,
        &mut *r,
        c,
        o,
        program,
        b,
        data_field!(m, &activate, "authority_snapshot"),
        data_field!(m, h, "operation_key"),
        data_field!(m, &activate, "accepted_at")
    )?;
    // Acceptance birth is independently located, not inferred from activation.
    let accept=if program.uses_family_close_recipe() {
        let key=data_field!(m,a,"operation_key");
        operation_data(m,r,c,key,"agreement.accept",true)?
    } else {
        let accept = invoke!(
        operation,
        m,
        &mut *r,
        c,
        &data_field!(m, &a, "operation_key"),
        "agreement.accept"
    )?;
        accept
    };
    for k in ["accepted_at", "commit_sequence"] {
        invoke!(copy, m, &accept, k, a, k)?;
    }
    let accepted_auth = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        &data_field!(m, &a, "authority_observation_ref"),
        "workflow-authority",
        "ledger-workflow-agreement-authority/2"
    )?;
    invoke!(eq, m, &accept, "authority_snapshot", &accepted_auth)?;
    invoke!(
        admission,
        m,
        &accepted_auth,
        &data_field!(m, &a, "operation_key"),
        b,
        &data_field!(m, &a, "accepted_at"),
        &data_field!(m, &b, "accepted_by"),
        "agreement.accept"
    )?;
    invoke!(
        super::historical::source_and_root,
        m,
        &mut *r,
        c,
        o,
        program,
        b,
        &accepted_auth,
        data_field!(m, a, "operation_key"),
        data_field!(m, a, "accepted_at")
    )?;
    let results = invoke!(shape::array, m, &accept, "result_refs")?;
    invoke!(count, m, results, 3)?;
    invoke!(
        shape::contains,
        m,
        results,
        &json!(c.authorities[0].accepted_agreement_ref)
    )?;
    let mut intent_ref = None;
    let mut head_ref = None;
    for result in results {
        m.visit("inventory.accept_result")?;
        if invoke!(
            shape::same,
            m,
            data_field!(m, result, "kind"),
            &json!("workflow-agreement-intent")
        )? {
            intent_ref = Some(result);
        }
        if invoke!(
            shape::same,
            m,
            data_field!(m, result, "kind"),
            &json!("workflow-head-transition")
        )? {
            head_ref = Some(result);
        }
    }
    let intent = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        intent_ref.ok_or_else(|| shape::fail("acceptance intent"))?,
        "workflow-agreement-intent",
        "ledger-workflow-agreement-intent/1"
    )?;
    invoke!(
        shape::closed,
        m,
        &intent,
        &["schema", "kind", "payload"],
        &[]
    )?;
    invoke!(eq, m, &intent, "kind", &json!("agreement.accept"))?;
    let payload = data_field!(
        m,
        &accept,
        "request_evidence",
        "normalized_request",
        "payload"
    );
    invoke!(eq, m, &intent, "payload", payload)?;
    invoke!(
        shape::closed,
        m,
        payload,
        &[
            "schema",
            "agreement_id",
            "publication_ref",
            "slot_catalog_ref",
            "slot_instantiation_ref",
            "binding_inputs",
            "assent_refs",
            "authority_refs",
            "expected_revision",
            "documents"
        ],
        &[]
    )?;
    let payload_schema = shape::text(m, payload, "schema")?;
    let exposure = shape::same_text(m, payload_schema, "ledger-workflow-agreement-accept/4")?;
    if !exposure {
        eq(
            m,
            payload,
            "schema",
            &json!("ledger-workflow-agreement-accept/3"),
        )?;
    }

    invoke!(eq, m, payload, "expected_revision", &json!("0"))?;
    for key in [
        "agreement_id",
        "slot_catalog_ref",
        "slot_instantiation_ref",
        "assent_refs",
        "authority_refs",
    ] {
        invoke!(copy, m, payload, key, b, key)?;
    }
    invoke!(copy, m, payload, "publication_ref", a, "publication_ref")?;
    let projection = data_field!(m, payload, "binding_inputs");
    super::binding::inputs(m, projection, false)?;
    let documents = invoke!(shape::array, m, payload, "documents")?;
    bounds(m, documents.len(), 0, 64)?;
    super::binding::ordered(m, documents)?;
    for document in documents {
        m.visit("inventory.private_document_transport")?;
        let schema = invoke!(shape::text, m, document, "schema")?;
        let mut kind = None;
        if shape::same_text(m, schema, "ledger-workflow-slot-binding-terms/3")? {
            if !exposure {
                return Err(shape::fail("terms3 requires exact accept4 recipe"));
            }
            eq(m, document, "exposure_scope", &json!("original_work"))?;
            eq(m, document, "book", &json!("retail"))?;
            kind = Some("workflow-slot-terms");
        }
        for (supported, record_kind) in [
            ("ledger-workflow-role-assignment/1", "workflow-roles"),
            ("ledger-workflow-slot-catalog/1", "workflow-slot-catalog"),
            (
                "ledger-workflow-slot-instantiation/1",
                "workflow-slot-instantiation",
            ),
            (
                "ledger-workflow-slot-binding-terms/2",
                "workflow-slot-terms",
            ),
            (
                "ledger-workflow-acceptance-terms/1",
                "workflow-acceptance-terms",
            ),
            ("ledger-workflow-assent/1", "workflow-assent"),
        ] {
            if shape::same_text(m, schema, supported)? {
                kind = Some(record_kind);
                break;
            }
        }
        let kind = kind.ok_or_else(|| shape::fail("private document family"))?;
        let digest =
            super::super::records::hash(m, "workflow-document", &json!([schema, document]))?;
        let reference = json!({"kind":kind,"id":format!("wdoc_{}",&digest[7..]),"digest":digest});
        let retained = invoke!(load, m, &mut *r, c, o, &reference, kind, schema)?;
        shape::equal(m, document, &retained)?;
    }
    let ignored = [
        "schema",
        "policy_ref",
        "product_ref",
        "fact_schema_refs",
        "compiled_plan_ref",
        "evaluator",
        "accepted_by",
        "accepted_at",
    ];
    let map = b.as_object().ok_or_else(|| shape::fail("binding object"))?;
    for (key, value) in map {
        m.visit("inventory.binding_projection_field")?;
        let mut skip = false;
        for ignore in ignored {
            if invoke!(shape::same_text, m, key, ignore)? {
                skip = true;
                break;
            }
        }
        if !skip {
            invoke!(eq, m, projection, key, value)?;
        }
    }
    let href = head_ref.ok_or_else(|| shape::fail("acceptance head"))?;
    let accepted_head = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        href,
        "workflow-head-transition",
        "ledger-workflow-head-transition/1"
    )?;
    invoke!(
        shape::closed,
        m,
        &accepted_head,
        &[
            "schema",
            "scope",
            "customer",
            "agreement_id",
            "operation_key",
            "actor",
            "head_kind",
            "old_revision",
            "new_revision",
            "accepted_binding_ref",
            "accepted_at",
            "commit_sequence"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        &accepted_head,
        "head_kind",
        &json!("agreement_acceptance")
    )?;
    invoke!(eq, m, &accepted_head, "old_revision", &json!("0"))?;
    invoke!(eq, m, &accepted_head, "new_revision", &json!("1"))?;
    for key in [
        "scope",
        "customer",
        "agreement_id",
        "operation_key",
        "accepted_at",
        "commit_sequence",
    ] {
        invoke!(copy, m, &accepted_head, key, a, key)?;
    }
    invoke!(
        copy,
        m,
        &accepted_head,
        "accepted_binding_ref",
        a,
        "binding_ref"
    )?;
    invoke!(copy, m, &accepted_head, "actor", b, "accepted_by")?;
    invoke!(head_id, m, c, &accepted_head, href)?;
    Ok(())
}
