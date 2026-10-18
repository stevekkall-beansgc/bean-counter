use super::super::{CoreMeter, data::*, meter, shape};
use super::{check::*, publication::digest};
use crate::Result;
use serde_json::{Value, json};
/// Closed observation/verifier DATA, not authentication of the asserted prefix.
pub(in crate::policy::workflow::transition_constructor) fn validate(
    m: &mut CoreMeter,
    c: &DraftContextData,
    state: &Value,
    ob: &Value,
    verifier: &Value,
    root: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::observation::validate", |m| {
            validate_data(m, c, state, ob, verifier, root, false)
        });
    }
    #[cfg(not(test))]
    validate_data(m, c, state, ob, verifier, root, false)
}
fn validate_data(
    m: &mut CoreMeter,
    c: &DraftContextData,
    state: &Value,
    ob: &Value,
    verifier: &Value,
    root: &Value,
    app_catalog:bool,
) -> Result<()> {
    invoke!(
        shape::closed,
        m,
        ob,
        &[
            "schema",
            "logical_store_id",
            "scope",
            "installation_mode",
            "root_ref",
            "enrollment_ref",
            "current_state_ref",
            "current_grant_ref",
            "original_grant_refs",
            "head_revision",
            "verified_through_sequence",
            "verification_profile",
            "verifier_ref",
            "prefix_digest",
            "origin_count",
            "document_count",
            "audited_retained_bytes",
            "observed_at"
        ],
        &[]
    )?;
    invoke!(
        eq,
        m,
        ob,
        "verification_profile",
        &json!("ledger-authority-origin-audit/1")
    )?;
    invoke!(copy, m, ob, "head_revision", state, "revision")?;
    invoke!(
        one_of,
        m,
        invoke!(shape::text, m, ob, "installation_mode")?,
        &["real_local", "isolated_synthetic"]
    )?;
    for k in [
        "head_revision",
        "verified_through_sequence",
        "origin_count",
        "document_count",
        "audited_retained_bytes",
    ] {
        let n = invoke!(number, m, ob, k, true)?;
        if invoke!(meter::cmp, m, &n, &u64::MAX.into())?.is_gt() {
            return Err(shape::fail("observation counter"));
        }
    }
    invoke!(digest, m, data_field!(m, ob, "prefix_digest"))?;
    let basis = data_field!(m, root, "basis");
    for (field, want) in [
        ("logical_store_id", json!(c.key.logical_store_id)),
        ("scope", json!(c.key.scope)),
        ("installation_mode", json!(c.installation_mode)),
        ("observed_at", json!(c.accepted_at)),
    ] {
        eq(m, ob, field, &want)?;
    }
    let a = &c.authorities[0];
    for (field, want) in [
        ("enrollment_ref", json!(a.enrollment_ref)),
        ("current_state_ref", json!(a.current_state_ref)),
        ("current_grant_ref", json!(a.current_grant_ref)),
        ("original_grant_refs", json!(a.original_grant_refs)),
    ] {
        eq(m, ob, field, &want)?;
    }
    for field in [
        "root_ref",
        "enrollment_ref",
        "current_state_ref",
        "current_grant_ref",
        "verifier_ref",
    ] {
        invoke!(shape::reference, m, data_field!(m, ob, field))?;
    }
    invoke!(
        super::binding::refs,
        m,
        data_field!(m, ob, "original_grant_refs"),
        1,
        32
    )?;
    invoke!(eq, m, basis, "principal_id", &json!(c.actor))?;
    invoke!(eq, m, basis, "source", &json!(c.key.source))?;
    invoke!(
        eq,
        m,
        basis,
        "canonical_economic_namespace",
        &json!(c.canonical_economic_namespace)
    )?;
    if app_catalog{return super::super::audit_metadata::check_source(m,verifier);}
    invoke!(
        shape::closed,
        m,
        verifier,
        &[
            "schema",
            "profile",
            "algorithm",
            "resources",
            "origin_recipes",
            "artifacts"
        ],
        &[]
    )?;
    for (k, v) in [
        ("profile", "ledger-authority-origin-audit/1"),
        ("algorithm", "verified-introduction-fixed-point/1"),
        ("resources", "workflow-profile-b-origin-audit/1"),
    ] {
        invoke!(eq, m, verifier, k, &json!(v))?;
    }
    let recipes = invoke!(shape::array, m, verifier, "origin_recipes")?;
    invoke!(count, m, recipes, 3)?;
    invoke!(set, m, recipes)?;
    for (kind, variant, schema) in [
        (
            "authority.change",
            "customer.register",
            "ledger-workflow-customer-register/1",
        ),
        (
            "authority.change",
            "source.enroll",
            "ledger-workflow-source-enroll/1",
        ),
        (
            "evidence.retain",
            "all",
            "ledger-workflow-evidence-retain/1",
        ),
    ] {
        let recipe = invoke!(shape::named, m, recipes, "variant", variant)?;
        invoke!(
            shape::closed,
            m,
            recipe,
            &[
                "operation_kind",
                "variant",
                "payload_schema",
                "registry_digest",
                "normalization_profile",
                "closure_profile"
            ],
            &[]
        )?;
        for (k, v) in [
            ("operation_kind", kind),
            ("payload_schema", schema),
            ("normalization_profile", "ledger-operation-normalization/2"),
            ("closure_profile", "ledger-workflow-agreement-manifest/1"),
        ] {
            invoke!(eq, m, recipe, k, &json!(v))?;
        }
        invoke!(digest, m, data_field!(m, recipe, "registry_digest"))?;
        eq(
            m,
            recipe,
            "registry_digest",
            &json!("sha256:e023e7bf17aeed815f30b6fdce35fc0bf68cc9efcc0612a019b0b109494ac169"),
        )?;
    }
    let artifacts = invoke!(shape::array, m, verifier, "artifacts")?;
    invoke!(count, m, artifacts, 7)?;
    invoke!(set, m, artifacts)?;
    super::binding::ordered(m, artifacts)?;
    super::binding::ordered(m, recipes)?;
    for artifact in artifacts {
        m.visit("inventory.verifier_artifact")?;
        invoke!(shape::closed, m, artifact, &["path", "sha256"], &[])?;
        invoke!(opaque, m, artifact, "path", 256)?;
        let hash = invoke!(shape::text, m, artifact, "sha256")?;
        if !invoke!(meter::cmp, m, &hash.len().into(), &64usize.into())?.is_eq() {
            return Err(shape::fail("artifact raw hash"));
        }
        for b in hash.bytes() {
            m.scan_bytes(1)?;
            if !m.check("inventory.artifact_hex", || {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            })? {
                return Err(shape::fail("artifact raw hash"));
            }
        }
    }
    // These are the exact supported metadata pins, not an authentication of
    // physical/code-owned origin or a fixture-count shortcut.
    for (path, sha) in SUPPORTED_ARTIFACTS {
        let artifact = shape::named(m, artifacts, "path", path)?;
        eq(m, artifact, "sha256", &json!(sha))?;
    }
    // The literal seven artifact paths/digests and prefix contents are source DATA;
    // authenticating their physical/code-owned origin belongs to the host auditor.
    Ok(())
}
pub(in crate::policy::workflow::transition_constructor) const SUPPORTED_ARTIFACTS: &[(&str, &str)] = &[
    (
        "AUTHORITY-AUDIT-03.md",
        "760f97f7dc7281bc01c6ca462f0c4c55f9460fca85a8d7a6bb08b4448acd5087",
    ),
    (
        "authority-audit-03.schema.json",
        "c2d50718592e7d4ac824a62f0f9635b9472b0dbf0b4b3d9c658ee7333a1b213b",
    ),
    (
        "authority-audit-catalog.json",
        "4b529b10cd20b75179f18086cb16eb76531c805682975320096f55881236a6e5",
    ),
    (
        "agreement-ready.schema.json",
        "1f5764768505214a2b5c143b87a198d56671eef1363f9ed574b4b15c71c4943b",
    ),
    (
        "agreement-ready-registry.json",
        "c1a0225d0fa737dfbeb4fc674a01d560200080e0b7063ba138bd30005cc3a872",
    ),
    (
        "AGREEMENT-READY-AMENDMENT.md",
        "2e99c1dc27e597c4c6a5c416d9e448b714827d463a43bfd323ea06b769373aa6",
    ),
    (
        "AUTHORITY-AUDIT-04-REPAIR.md",
        "ee113e7761e130002162c162845600a7de561e86618d1e5eb8d4b62f9743895b",
    ),
];

pub(in crate::policy::workflow::transition_constructor) fn validate_app_catalog(m:&mut CoreMeter,c:&DraftContextData,state:&Value,ob:&Value,verifier:&Value,root:&Value)->Result<()>{
    validate_data(m,c,state,ob,verifier,root,true)
}
