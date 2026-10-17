//! One closed activation payload grammar shared by legacy and owning callers.
//! This is data selection only: no Binding/origin/current source is approved.
use super::*;
pub(in super::super) struct Shape {
    pub(in super::super) action: &'static str,
    pub(in super::super) fields: &'static [&'static str],
}
const ACTIVATE: &[&str] = &[
    "schema",
    "agreement_id",
    "expected_revision",
    "accepted_binding_ref",
    "evidence_refs",
    "policy_ref",
    "product_ref",
    "fact_schema_refs",
    "compiled_plan_ref",
    "effective",
];
const REPLACE: &[&str] = &[
    "schema",
    "agreement_id",
    "expected_revision",
    "accepted_binding_ref",
    "evidence_refs",
    "policy_ref",
    "product_ref",
    "fact_schema_refs",
    "compiled_plan_ref",
    "effective",
    "replaces_transition_ref",
];
const REVOKE: &[&str] = &[
    "schema",
    "agreement_id",
    "expected_revision",
    "accepted_binding_ref",
    "evidence_refs",
    "transition_ref",
    "reason",
];
pub(in super::super) fn select(
    kind: &str,
    schema: &str,
    mut same: impl FnMut(&str, &str) -> Result<bool>,
) -> Result<Shape> {
    for (expected_kind, expected_schema, action, fields) in [
        (
            "agreement.activate",
            "ledger-workflow-activation/1",
            "activate",
            ACTIVATE,
        ),
        (
            "agreement.activate",
            "ledger-workflow-activation-replace/1",
            "replace",
            REPLACE,
        ),
        (
            "agreement.revoke",
            "ledger-workflow-agreement-revoke/1",
            "revoke",
            REVOKE,
        ),
    ] {
        if same(kind, expected_kind)? && same(schema, expected_schema)? {
            return Ok(Shape { action, fields });
        }
    }
    Err(reject("UNSUPPORTED_SCHEMA_PROFILE"))
}
