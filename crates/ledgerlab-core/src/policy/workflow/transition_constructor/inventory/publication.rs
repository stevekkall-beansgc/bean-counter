use super::super::super::selected_program::SelectedProgram;
use super::super::{
    CoreMeter, body,
    data::*,
    records::{self, Records},
    representation::InvocationToken,
    shape,
};
use super::check::*;
use crate::Result;
use serde_json::{Value, json};
/// Exact original program/source/context DATA; no current support substitution.
pub(in crate::policy::workflow::transition_constructor) fn validate(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    program: &SelectedProgram,
    b: &Value,
    a: &Value,
    terms: &Value,
) -> Result<()> {
    #[cfg(test)]
    {
        return m.diagnostic_scope("inventory::publication::validate", |m| {
            validate_data(m, r, c, o, program, b, a, terms)
        });
    }
    #[cfg(not(test))]
    validate_data(m, r, c, o, program, b, a, terms)
}
fn validate_data(
    m: &mut CoreMeter,
    r: &mut Records,
    c: &DraftContextData,
    o: &InvocationToken,
    program: &SelectedProgram,
    b: &Value,
    a: &Value,
    terms: &Value,
) -> Result<()> {
    let publication_ref = data_field!(m, a, "publication_ref");
    let publication = invoke!(
        load,
        m,
        &mut *r,
        c,
        o,
        publication_ref,
        "workflow-publication",
        "ledger-workflow-publication/1"
    )?;
    invoke!(
        shape::closed,
        m,
        &publication,
        &[
            "schema",
            "root_policy_ref",
            "source_refs",
            "compiler_ref",
            "compiled_plan_ref",
            "provenance"
        ],
        &[]
    )?;
    invoke!(copy, m, terms, "publication_ref", a, "publication_ref")?;
    invoke!(copy, m, &publication, "root_policy_ref", b, "policy_ref")?;
    invoke!(
        copy,
        m,
        &publication,
        "compiled_plan_ref",
        b,
        "compiled_plan_ref"
    )?;
    let plan = program.compiled_plan();
    invoke!(eq, m, b, "compiled_plan_ref", &json!(plan.reference()))?;
    invoke!(
        eq,
        m,
        &publication,
        "compiler_ref",
        data_field!(m, plan.canonical_value(), "compiler_ref")
    )?;
    let pid = invoke!(
        body::semantic_id,
        m,
        "wpub_",
        "workflow-publication",
        &json!([
            c.key.logical_store_id,
            c.key.scope,
            data_field!(m, b, "policy_ref")
        ])
    )?;
    invoke!(eq, m, publication_ref, "id", &json!(pid))?;
    let sources: Vec<ExactRef> = program
        .source_documents()
        .iter()
        .map(|d| ExactRef {
            kind: d.reference().kind().into(),
            id: d.reference().id().into(),
            digest: d.reference().digest().into(),
        })
        .collect();
    let ordered = invoke!(body::ref_set, m, sources)?;
    invoke!(eq, m, &publication, "source_refs", &json!(ordered))?;
    let mut policy = None;
    let mut product = None;
    for d in program.source_documents() {
        m.visit("inventory.original_definition")?;
        if invoke!(
            shape::same,
            m,
            &json!(d.reference()),
            data_field!(m, b, "policy_ref")
        )? {
            policy = Some(d.canonical_value());
        }
        if invoke!(
            shape::same,
            m,
            &json!(d.reference()),
            data_field!(m, b, "product_ref")
        )? {
            product = Some(d.canonical_value());
        }
    }
    let policy = policy.ok_or_else(|| shape::fail("publication original policy"))?;
    let product = product.ok_or_else(|| shape::fail("publication original product"))?;
    invoke!(copy, m, policy, "product_ref", b, "product_ref")?;
    invoke!(copy, m, product, "fact_schemas", b, "fact_schema_refs")?;
    invoke!(
        eq,
        m,
        b,
        "evaluator",
        data_field!(m, plan.canonical_value(), "evaluator")
    )?;
    let context = data_field!(m, b, "accepted_context");
    invoke!(
        shape::closed,
        m,
        context,
        &["fact_schema_ref", "facts"],
        &[]
    )?;
    invoke!(
        copy,
        m,
        context,
        "fact_schema_ref",
        policy,
        "context_schema_ref"
    )?;
    let mut schema = None;
    for (reference, s) in program.fact_schemas() {
        m.visit("inventory.context_schema")?;
        if invoke!(
            shape::same,
            m,
            &json!(reference),
            data_field!(m, context, "fact_schema_ref")
        )? {
            schema = Some(s);
        }
    }
    let schema = schema.ok_or_else(|| shape::fail("original context schema"))?;
    m.scoped(
        "FactSchema::validate_context_schema_with_work.accepted_context",
        |m| schema.validate_context_schema_with_work(m),
    )?;
    let facts = data_field!(m, context, "facts");
    if let Some(arena)=m.representation_arena(){
        let normalized=super::super::owned_facts::normalize(&arena,m,schema,facts)?;
        invoke!(eq,m,context,"facts",normalized.facts().canonical_value())?;
    }else{
        let normalized = m.scoped(
            "FactSchema::normalize_value_with_work.accepted_context",
            |m| schema.normalize_value_with_work(facts, m),
        )?;
        invoke!(eq, m, context, "facts", normalized.canonical_value())?;
    }
    let provenance = data_field!(m, &publication, "provenance");
    invoke!(
        shape::closed,
        m,
        provenance,
        &["source_sha", "binary_sha256", "build_manifest_digest"],
        &[]
    )?;
    invoke!(
        raw_hex,
        m,
        invoke!(shape::text, m, provenance, "source_sha")?,
        40
    )?;
    invoke!(
        raw_hex,
        m,
        invoke!(shape::text, m, provenance, "binary_sha256")?,
        64
    )?;
    // Build body raw digest is a locator followed by the same canonical/document
    // validation. It never identifies the currently executing binary or support.
    let build_digest = invoke!(shape::text, m, provenance, "build_manifest_digest")?;
    let (_, manifest) = r.by_raw_sha(
        m,
        &c.key,
        "workflow-build-manifest",
        build_digest,
        "ledger-workflow-build-manifest/1",
    )?;
    invoke!(
        shape::closed,
        m,
        &manifest,
        &["schema", "build_inputs", "executable"],
        &[]
    )?;
    let inputs = data_field!(m, &manifest, "build_inputs");
    invoke!(
        shape::closed,
        m,
        inputs,
        &["schema", "source", "recipe"],
        &[]
    )?;
    invoke!(
        eq,
        m,
        inputs,
        "schema",
        &json!("ledger-workflow-build-inputs/1")
    )?;
    let source = data_field!(m, inputs, "source");
    invoke!(
        shape::closed,
        m,
        source,
        &[
            "base_commit",
            "state",
            "inventory_digest",
            "file_count",
            "total_bytes"
        ],
        &[]
    )?;
    invoke!(copy, m, source, "base_commit", provenance, "source_sha")?;
    invoke!(
        one_of,
        m,
        invoke!(shape::text, m, source, "state")?,
        &["clean_commit", "worktree_snapshot"]
    )?;
    invoke!(digest, m, data_field!(m, source, "inventory_digest"))?;
    let file_count = data_field!(m, source, "file_count")
        .as_u64()
        .ok_or_else(|| shape::fail("build file count"))?;
    if !invoke!(
        super::super::meter::cmp,
        m,
        &file_count.into(),
        &0u64.into()
    )?
    .is_gt()
    {
        return Err(shape::fail("build file count"));
    }
    invoke!(number, m, source, "total_bytes", true)?;
    let recipe = data_field!(m, inputs, "recipe");
    invoke!(
        shape::closed,
        m,
        recipe,
        &[
            "build_profile",
            "cargo_lock_digest",
            "cargo_version",
            "compiler_ref",
            "features",
            "package_name",
            "package_version",
            "rustc_version",
            "rustflags",
            "target"
        ],
        &[]
    )?;
    invoke!(
        copy,
        m,
        recipe,
        "compiler_ref",
        &publication,
        "compiler_ref"
    )?;
    invoke!(digest, m, data_field!(m, recipe, "cargo_lock_digest"))?;
    for k in [
        "build_profile",
        "cargo_version",
        "package_name",
        "package_version",
        "rustc_version",
        "target",
    ] {
        invoke!(opaque, m, recipe, k, 256)?;
    }
    for k in ["features", "rustflags"] {
        let items = invoke!(shape::array, m, recipe, k)?;
        invoke!(bounds, m, items.len(), 0, 64)?;
        for item in items {
            m.visit("inventory.build_argument")?;
            invoke!(opaque, m, &json!({"x":item}), "x", 256)?;
        }
    }
    let executable = data_field!(m, &manifest, "executable");
    invoke!(
        shape::closed,
        m,
        executable,
        &["bytes", "observation", "sha256"],
        &[]
    )?;
    invoke!(number, m, executable, "bytes", true)?;
    invoke!(
        eq,
        m,
        executable,
        "observation",
        &json!("stable-current-executable-file/1")
    )?;
    invoke!(copy, m, executable, "sha256", provenance, "binary_sha256")?;
    Ok(())
}
fn raw_hex(m: &mut CoreMeter, text: &str, length: usize) -> Result<()> {
    if !invoke!(
        super::super::meter::cmp,
        m,
        &text.len().into(),
        &length.into()
    )?
    .is_eq()
    {
        return Err(shape::fail("raw hash length"));
    }
    for b in text.bytes() {
        m.scan_bytes(1)?;
        if !m.check("inventory.hash_digit", || {
            b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
        })? {
            return Err(shape::fail("raw hash digit"));
        }
    }
    Ok(())
}
pub(super) fn digest(m: &mut CoreMeter, v: &Value) -> Result<()> {
    let s = v.as_str().ok_or_else(|| shape::fail("digest string"))?;
    if !invoke!(shape::same_text, m, s.get(..7).unwrap_or(""), "sha256:")? {
        return Err(shape::fail("digest tag"));
    }
    invoke!(raw_hex, m, &s[7..], 64)
}
