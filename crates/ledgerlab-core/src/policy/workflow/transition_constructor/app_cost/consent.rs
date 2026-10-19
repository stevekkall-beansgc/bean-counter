//! Exact original fullterms/customer/platform associations and independently
//! held current platform authorization heads. Origin custody is not inferred
//! from a verification dictionary or a matching hash.
use super::super::super::selected_program::SelectedProgram;
use super::super::{
    cost_postings, data::*, inventory, record_body::RecordBody, records::Records,
    representation::InvocationToken, shape, working_value::owning_reference, CoreMeter,
};
use crate::{domain::Timestamp, validation_work as w, Result};
use serde_json::Value;
#[derive(Clone, Copy)]
struct Recipe {
    assent: &'static str,
    evidence: &'static str,
    local_management_authority: bool,
}
fn recipe(program: &SelectedProgram) -> Recipe {
    if program.uses_commitment_first_consent_recipe() {
        Recipe {
            assent: "ledger-workflow-party-assent/2",
            evidence: "ledger-workflow-party-evidence/2",
            local_management_authority: program.uses_local_execution_recipe(),
        }
    } else {
        Recipe {
            assent: "ledger-workflow-party-assent/1",
            evidence: "ledger-workflow-party-evidence/1",
            local_management_authority: false,
        }
    }
}
fn eq(m: &mut CoreMeter, a: &Value, b: &Value) -> Result<()> {
    shape::equal(m, a, b)
}
fn literal(m: &mut CoreMeter, v: &Value, k: &str, s: &str) -> Result<()> {
    let a = shape::text(m, v, k)?;
    if !shape::same_text(m, a, s)? {
        return Err(shape::fail("app fullterms consent literal"));
    }
    Ok(())
}
fn load(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    v: &Value,
    kind: &str,
    schema: &str,
) -> Result<(ExactRef, RecordBody)> {
    let reference = shape::reference(m, v)?;
    let body = r.get(m, &c.key, &reference, kind, schema)?;
    Ok((reference, body))
}
fn time(m: &mut CoreMeter, v: &Value, k: &str) -> Result<i64> {
    let raw = shape::text(m, v, k)?;
    Ok(Timestamp::parse_with_work(raw, m)?.micros())
}
fn roles(m: &mut CoreMeter, v: &Value) -> Result<()> {
    let rows = shape::array(m, v, "roles")?;
    if w::compare(m, rows.len(), 1usize)?.is_lt() || w::compare(m, rows.len(), 3usize)?.is_gt() {
        return Err(shape::fail("party role count"));
    }
    inventory::binding::ordered(m, rows)?;
    for value in rows {
        let name = value
            .as_str()
            .ok_or_else(|| shape::fail("party role string"))?;
        if !shape::same_text(m, name, "payer")?
            && !shape::same_text(m, name, "bearer")?
            && !shape::same_text(m, name, "provider")?
        {
            return Err(shape::fail("closed party role"));
        }
    }
    Ok(())
}
fn authorization(m: &mut CoreMeter, v: &Value, b: &Value, at: i64) -> Result<()> {
    shape::closed(
        m,
        v,
        &[
            "schema",
            "authorization",
            "revision",
            "status",
            "evidence_ref",
            "operator_attestation_ref",
            "recorded_by",
            "recorded_at",
            "installation_mode",
            "authority_ref",
        ],
        &["prior_ref"],
    )?;
    literal(m, v, "schema", "ledger-workflow-party-authorization/1")?;
    literal(m, v, "status", "active")?;
    let spec = &v["authorization"];
    shape::closed(
        m,
        spec,
        &[
            "authorization_id",
            "scope",
            "customer",
            "agreement_id",
            "party",
            "acceptor",
            "roles",
            "starts_at",
            "ends_at",
        ],
        &[],
    )?;
    for field in ["customer", "agreement_id"] {
        eq(m, &spec[field], &b[field])?;
    }
    roles(m, spec)?;
    let revision = shape::text(m, v, "revision")?;
    let revision = crate::policy::workflow::facts::counter_with_work(revision, m)?;
    if !w::compare(m, revision, 0u64)?.is_gt() {
        return Err(shape::fail("positive party authorization revision"));
    }
    if w::compare(m, revision, 1u64)?.is_eq() {
        if v.get("prior_ref").is_some() {
            return Err(shape::fail("initial party mapping predecessor"));
        }
    } else {
        shape::reference(m, &v["prior_ref"])?;
    }
    let start = time(m, spec, "starts_at")?;
    let end = time(m, spec, "ends_at")?;
    let recorded = time(m, v, "recorded_at")?;
    if w::compare(m, start, end)?.is_ge()
        || w::compare(m, at, start)?.is_lt()
        || !w::compare(m, at, end)?.is_lt()
        || w::compare(m, recorded, at)?.is_gt()
    {
        return Err(shape::fail("party authorization trusted interval"));
    }
    Ok(())
}
/// Local PartyEvidence retains the actual management authority. Its nested
/// local_admin_ref, rather than the management Ref itself, is the snapshot.
fn party_management_root(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    evidence: &Value,
) -> Result<()> {
    let (_, authority) = load(
        m,
        r,
        c,
        &evidence["authority_ref"],
        "workflow-authority",
        "ledger-workflow-party-management-authority/1",
    )?;
    shape::closed(
        m,
        &authority,
        &[
            "schema",
            "operation",
            "actor",
            "scope",
            "customer",
            "agreement_id",
            "local_admin_ref",
            "evidence_refs",
            "observed_at",
            "installation_mode",
        ],
        &[],
    )?;
    literal(
        m,
        &authority,
        "schema",
        "ledger-workflow-party-management-authority/1",
    )?;
    literal(m, &authority, "operation", "evidence.retain")?;
    for (field, evidence_field) in [
        ("actor", "retained_by"),
        ("scope", "scope"),
        ("observed_at", "retained_at"),
        ("installation_mode", "installation_mode"),
    ] {
        eq(m, &authority[field], &evidence[evidence_field])?;
    }
    for field in ["customer", "agreement_id"] {
        eq(m, &authority[field], &evidence["subject"][field])?;
    }
    inventory::binding::id(m, &authority["actor"], 128)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning party management authority"))?;
    let metadata = shape::field(m, evidence, "metadata")?;
    let purpose = shape::text(m, metadata, "purpose")?;
    let expected = if shape::same_text(m, purpose, "party_consent")? {
        arena.array_from_values(&[], m)?
    } else if shape::same_text(m, purpose, "party_operator_attestation")? {
        let target = shape::field(m, metadata, "target_evidence_ref")?;
        shape::reference(m, target)?;
        arena.array_from_values(&[target], m)?
    } else {
        return Err(shape::fail("party management consent evidence purpose"));
    };
    eq(m, &authority["evidence_refs"], expected.value())?;
    let owner = InvocationToken::new();
    inventory::evidence::root(
        m,
        r,
        c,
        &owner,
        &authority["local_admin_ref"],
        &authority,
        "observed_at",
        &authority["actor"],
        None,
    )?;
    Ok(())
}
fn party_material(
    recipe: Recipe,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    reference: &Value,
    subject: &Value,
    metadata: &Value,
    before: &Value,
    key: &str,
) -> Result<()> {
    let (_, evidence) = load(m, r, c, reference, "workflow-evidence", recipe.evidence)?;
    shape::closed(
        m,
        &evidence,
        &[
            "schema",
            "subject",
            "media_type",
            "content_base64",
            "content_sha256",
            "metadata",
            "scope",
            "retained_by",
            "retained_at",
            "installation_mode",
            "authority_ref",
        ],
        &[],
    )?;
    eq(m, &evidence["subject"], subject)?;
    eq(m, &evidence["metadata"], metadata)?;
    literal(m, &evidence, "installation_mode", &c.installation_mode)?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning party evidence material"))?;
    let material = arena.object_from_fields(
        &[
            ("media_type", &evidence["media_type"]),
            ("content_base64", &evidence["content_base64"]),
            ("content_sha256", &evidence["content_sha256"]),
        ],
        m,
    )?;
    super::evidence::material(m, material.value())?;
    let key_value = cost_postings::operation_key(m, &arena, c)?;
    eq(m, &evidence["scope"], &key_value.value()["scope"])?;
    let retained = time(m, &evidence, "retained_at")?;
    let limit = time(m, before, key)?;
    if w::compare(m, retained, limit)?.is_gt() {
        return Err(shape::fail("party evidence after original consent"));
    }
    if recipe.local_management_authority {
        party_management_root(m, r, c, &evidence)?;
    } else {
        let owner = InvocationToken::new();
        inventory::evidence::root(
            m,
            r,
            c,
            &owner,
            &evidence["authority_ref"],
            &evidence,
            "retained_at",
            &evidence["retained_by"],
            None,
        )?;
    }
    Ok(())
}
fn party(
    recipe: Recipe,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    b: &Value,
    accepted: &Value,
    terms_ref: &ExactRef,
    assent: &Value,
) -> Result<RecordBody> {
    shape::closed(
        m,
        assent,
        &[
            "schema",
            "consent",
            "mode",
            "evidence_ref",
            "operator",
            "operator_attestation_ref",
        ],
        &[],
    )?;
    literal(m, assent, "schema", recipe.assent)?;
    let consent = &assent["consent"];
    shape::closed(
        m,
        consent,
        &[
            "scope",
            "customer",
            "agreement_id",
            "party",
            "acceptor",
            "roles",
            "authorization_ref",
            "basis",
            "accepted_at",
        ],
        &[],
    )?;
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning original platform consent"))?;
    let key = cost_postings::operation_key(m, &arena, c)?;
    eq(m, &consent["scope"], &key.value()["scope"])?;
    for field in ["customer", "agreement_id"] {
        eq(m, &consent[field], &b[field])?;
    }
    roles(m, consent)?;
    shape::closed(
        m,
        &consent["basis"],
        &["kind", "terms_ref", "terms_digest"],
        &[],
    )?;
    literal(m, &consent["basis"], "kind", "acceptance_terms")?;
    let terms = owning_reference(m, &arena, terms_ref)?;
    eq(m, &consent["basis"]["terms_ref"], terms.value())?;
    eq(
        m,
        &consent["basis"]["terms_digest"],
        &accepted["terms_digest"],
    )?;
    let (auth_ref, auth) = load(
        m,
        r,
        c,
        &consent["authorization_ref"],
        "workflow-authority",
        "ledger-workflow-party-authorization/1",
    )?;
    let at = time(m, consent, "accepted_at")?;
    let binding_at = time(m, b, "accepted_at")?;
    if w::compare(m, at, binding_at)?.is_gt() {
        return Err(shape::fail("platform assent after original agreement"));
    }
    authorization(m, &auth, b, at)?;
    eq(m, &auth["authorization"]["scope"], &consent["scope"])?;
    for field in ["party", "acceptor"] {
        eq(m, &auth["authorization"][field], &consent[field])?;
    }
    let nominated = shape::array(m, b, "authority_refs")?;
    if !shape::member(m, nominated, &consent["authorization_ref"])? {
        return Err(shape::fail(
            "platform mapping absent original accepted authority",
        ));
    }
    let allowed = shape::array(m, &auth["authorization"], "roles")?;
    for role in shape::array(m, consent, "roles")? {
        if !shape::member(m, allowed, role)? {
            return Err(shape::fail("party assent role outside authorization"));
        }
    }
    literal(
        m,
        assent,
        "mode",
        if c.installation_mode == "isolated_synthetic" {
            "synthetic"
        } else {
            "real"
        },
    )?;
    let metadata = arena.object_from_fields(
        &[
            ("purpose", arena.string_value("party_consent", m)?.value()),
            ("consent", consent),
        ],
        m,
    )?;
    let subject = arena.object_from_fields(
        &[
            ("customer", &consent["customer"]),
            ("agreement_id", &consent["agreement_id"]),
            ("party", &consent["party"]),
            ("acceptor", &consent["acceptor"]),
        ],
        m,
    )?;
    party_material(
        recipe,
        m,
        r,
        c,
        &assent["evidence_ref"],
        subject.value(),
        metadata.value(),
        b,
        "accepted_at",
    )?;
    let statement = arena.string_value("operator_verified_party_exact_consent", m)?;
    let purpose = arena.string_value("party_operator_attestation", m)?;
    let metadata = arena.object_from_fields(
        &[
            ("purpose", purpose.value()),
            ("target_evidence_ref", &assent["evidence_ref"]),
            ("statement", statement.value()),
        ],
        m,
    )?;
    party_material(
        recipe,
        m,
        r,
        c,
        &assent["operator_attestation_ref"],
        subject.value(),
        metadata.value(),
        b,
        "accepted_at",
    )?;
    let _ = auth_ref;
    Ok(auth)
}
pub(super) fn original(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    b: &Value,
    accepted: &Value,
    terms_ref: &ExactRef,
    registered: &Value,
) -> Result<()> {
    let recipe = recipe(program);
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning app original assents"))?;
    let owner = InvocationToken::new();
    let assents = shape::array(m, b, "assent_refs")?;
    if w::compare(m, assents.len(), 2usize)?.is_lt()
        || w::compare(m, assents.len(), 16usize)?.is_gt()
    {
        return Err(shape::fail(
            "app requires customer and platform fullterms assent",
        ));
    }
    inventory::binding::ordered(m, assents)?;
    let mut customer_count = 0usize;
    let mut platform_count = 0usize;
    for value in assents {
        let reference = shape::reference(m, value)?;
        let assent = r.get_schemas(
            m,
            &c.key,
            &reference,
            "workflow-assent",
            &["ledger-workflow-assent/1", recipe.assent],
        )?;
        let schema = shape::text(m, &assent, "schema")?;
        if shape::same_text(m, schema, "ledger-workflow-assent/1")? {
            for field in ["customer", "agreement_id"] {
                eq(m, &assent[field], &b[field])?;
            }
            eq(m, &assent["terms_digest"], &accepted["terms_digest"])?;
            for (field, purpose) in [
                ("evidence_ref", "assent"),
                ("operator_attestation_ref", "operator_attestation"),
            ] {
                let (_, evidence) = load(
                    m,
                    r,
                    c,
                    &assent[field],
                    "workflow-evidence",
                    "ledger-workflow-retained-evidence/1",
                )?;
                inventory::evidence::assent(
                    m, r, c, &owner, &assent, &evidence, purpose, registered, b,
                )?;
            }
            customer_count = w::usize_add(m, customer_count, 1)?;
        } else {
            let auth = party(recipe, m, r, c, b, accepted, terms_ref, &assent)?;
            let consent = &assent["consent"];
            let payer = arena.string_value("payer", m)?;
            let bearer = arena.string_value("bearer", m)?;
            let roles = shape::array(m, consent, "roles")?;
            if !shape::member(m, roles, payer.value())? || !shape::member(m, roles, bearer.value())?
            {
                return Err(shape::fail(
                    "app resource payer and bearer consent required",
                ));
            }
            let mut used = false;
            for leg in shape::array(m, b, "funding_legs")? {
                let mode = shape::text(m, leg, "mode")?;
                if shape::same_text(m, mode, "platform")?
                    && shape::same(m, &leg["payer"], &consent["party"])?
                    && shape::same(m, &leg["resource_owner"], &consent["party"])?
                {
                    used = true;
                }
            }
            if !used {
                return Err(shape::fail("unrelated platform fullterms assent"));
            }
            let _ = auth;
            platform_count = w::usize_add(m, platform_count, 1)?;
        }
    }
    if !w::compare(m, customer_count, 1usize)?.is_eq()
        || !w::compare(m, platform_count, 1usize)?.is_eq()
    {
        return Err(shape::fail(
            "first app capability requires exact customer/platform assent",
        ));
    }
    Ok(())
}
/// Called by complete construction with actual locked current-head DATA.
/// Head absence, scope ownership and original-origin completion stay host
/// obligations; equality with historical consent fields never supplies them.
pub(in crate::policy::workflow::transition_constructor) fn current(
    program: &SelectedProgram,
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    b: &Value,
    accepted: &Value,
    state: &EconomicStateData,
) -> Result<()> {
    let recipe = recipe(program);
    let arena = m
        .representation_arena()
        .ok_or_else(|| shape::fail("owning current app party authority"))?;
    let key = cost_postings::operation_key(m, &arena, c)?;
    let now = Timestamp::parse_with_work(&c.accepted_at, m)?.micros();
    for value in shape::array(m, b, "assent_refs")? {
        let reference = shape::reference(m, value)?;
        let assent = r.get_schemas(
            m,
            &c.key,
            &reference,
            "workflow-assent",
            &["ledger-workflow-assent/1", recipe.assent],
        )?;
        let schema = shape::text(m, &assent, "schema")?;
        if shape::same_text(m, schema, "ledger-workflow-assent/1")? {
            continue;
        }
        let consent = &assent["consent"];
        let (_, original) = load(
            m,
            r,
            c,
            &consent["authorization_ref"],
            "workflow-authority",
            "ledger-workflow-party-authorization/1",
        )?;
        let original_id = shape::text(m, &original["authorization"], "authorization_id")?;
        let mut found = None;
        for head in state.current_party_authorizations {
            m.visit("app.held_party_head")?;
            if shape::same_text(m, &head.authorization_id, original_id)? {
                if found.is_some() {
                    return Err(shape::fail("duplicate held app party authorization head"));
                }
                found = Some(head);
            }
        }
        let head = found.ok_or_else(|| shape::fail("current app party authorization missing"))?;
        let current = r.get(
            m,
            &c.key,
            &head.reference,
            "workflow-authority",
            "ledger-workflow-party-authorization/1",
        )?;
        authorization(m, &current, b, now)?;
        literal(m, &current, "revision", &head.revision)?;
        literal(
            m,
            &current["authorization"],
            "authorization_id",
            original_id,
        )?;
        eq(m, &current["authorization"]["scope"], &key.value()["scope"])?;
        for field in ["customer", "agreement_id", "party", "acceptor"] {
            eq(m, &current["authorization"][field], &consent[field])?;
        }
        let allowed = shape::array(m, &current["authorization"], "roles")?;
        for role in shape::array(m, consent, "roles")? {
            if !shape::member(m, allowed, role)? {
                return Err(shape::fail(
                    "current mapping no longer permits accepted app role",
                ));
            }
        }
        eq(
            m,
            &consent["basis"]["terms_digest"],
            &accepted["terms_digest"],
        )?;
        // Intrinsic/custody of evidence/attestation was paid by this exact current
        // mapping's completed original recipe. Exact contextual associations remain
        // paid here, without reinterpreting retained issuer bytes as new permission.
        for field in ["evidence_ref", "operator_attestation_ref", "authority_ref"] {
            let reference = shape::reference(m, &current[field])?;
            r.provenance(m, &c.key, &reference)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod management_tests {
    use super::*;
    use crate::canonical::{audit_arena::AuditArena, CanonicalBytes};
    use crate::validation_work::OriginalAuditSession;
    use serde_json::json;
    fn record(body: &Value) -> RecordInput {
        let digest =
            crate::policy::workflow::document_digest(body["schema"].as_str().unwrap(), body)
                .unwrap();
        RecordInput {
            reference: ExactRef {
                kind: "workflow-authority".into(),
                id: format!("wdoc_{}", &digest[7..]),
                digest,
            },
            canonical_body: CanonicalBytes::from_value(body).unwrap().into_vec(),
        }
    }
    fn check(case: u8) -> Result<()> {
        let (_, c, _, inputs) = super::super::super::first_work_tests::test_inputs();
        let root = inputs
            .into_iter()
            .find(|r| {
                serde_json::from_slice::<Value>(&r.canonical_body).unwrap()["schema"]
                    == "ledger-workflow-local-authority-snapshot/1"
            })
            .unwrap();
        let root_body: Value = serde_json::from_slice(&root.canonical_body).unwrap();
        let actor = root_body["basis"]["principal_id"].clone();
        let at = root_body["verified_at"].clone();
        let target = json!({"kind":"workflow-evidence","id":format!("wdoc_{}","a".repeat(64)),"digest":format!("sha256:{}","a".repeat(64))});
        let attestation = case == 1 || case == 4;
        let metadata = if attestation {
            json!({"purpose":"party_operator_attestation","target_evidence_ref":target})
        } else {
            json!({"purpose":"party_consent"})
        };
        let mut authority = json!({"schema":"ledger-workflow-party-management-authority/1","operation":"evidence.retain","actor":actor,"scope":c.key.scope,"customer":"customer","agreement_id":"agreement","local_admin_ref":root.reference,"evidence_refs":if attestation{json!([target])}else{json!([])},"observed_at":at,"installation_mode":c.installation_mode});
        match case {
            2 => authority["actor"] = json!("other-operator"),
            3 => authority["scope"] = json!(["other", "scope"]),
            4 => authority["evidence_refs"] = json!([]),
            5 => authority["operation"] = json!("authority.change"),
            _ => {}
        }
        let authority = record(&authority);
        let evidence = json!({"authority_ref":if case==6{&root.reference}else{&authority.reference},"retained_by":actor,"retained_at":at,"scope":c.key.scope,"installation_mode":c.installation_mode,"subject":{"customer":"customer","agreement_id":"agreement"},"metadata":metadata});
        let rows = [root, authority];
        let views: Vec<_> = rows
            .iter()
            .map(|r| EconomicRecordView {
                reference: &r.reference,
                canonical_body: &r.canonical_body,
            })
            .collect();
        let arena = AuditArena::new()?;
        let mut s = OriginalAuditSession::for_resource_profile_v2(&arena)?;
        let mut child = s.meter_mut().local_economic_meter()?;
        let result = {
            let mut r = Records::borrowed_app(&mut child, &arena, &c.key, &views, &[])?;
            if case == 7 {
                let owner = InvocationToken::new();
                inventory::evidence::root(
                    &mut child,
                    &mut r,
                    &c,
                    &owner,
                    &json!(rows[0].reference),
                    &evidence,
                    "retained_at",
                    &evidence["retained_by"],
                    None,
                )
                .map(|_| ())
            } else {
                party_management_root(&mut child, &mut r, &c, &evidence)
            }
        };
        if result.is_ok() {
            s.meter_mut().finish_local_economic(child)?;
        }
        result
    }
    #[test]
    fn local_party_management_authority_joins_nested_snapshot_without_widening_it() {
        assert!(check(0).is_ok());
        assert!(check(1).is_ok());
        for case in 2..=6 {
            assert!(check(case).is_err(), "case{case}");
        }
        assert!(check(7).is_ok());
    }
}
