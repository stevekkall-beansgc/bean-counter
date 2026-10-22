//! Binary-owned, non-admitting support. No request/retained descriptor becomes
//! executable support. All catalog membership and raw artifact pins are checked.
use super::super::DefinitionRef;
use crate::{canonical, Error, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
#[derive(Clone, Debug)]
pub struct SelectedCompilerSupport {
    document: SupportDocument,
    // Set only by an exact code-owned capability/saved-verifier, never by
    // parsing a caller or retained Descriptor DATA dictionary.
    execution_registry: Option<&'static str>,
}
#[derive(Clone, Debug)]
enum SupportDocument {
    Legacy {
        body: Value,
        reference: DefinitionRef,
        bytes: Vec<u8>,
    },
    Owned(super::compiler::SelectedDocument),
}
fn raw(bytes: &[u8]) -> String {
    format!("sha256:{}", canonical::hex(&Sha256::digest(bytes)))
}
impl SelectedCompilerSupport {
    /// Exact binary-owned local executor selection. This profile preserves
    /// finite guards and financial rights; it makes no universal future
    /// resource or whole-history audit promise. Caller descriptors cannot
    /// select or widen this capability.
    pub fn local_execution_with_session(
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<Self> {
        let document = super::local::local_compiler_descriptor_with_session(session)?;
        Ok(Self {
            document: SupportDocument::Owned(document),
            execution_registry: Some(super::local::REGISTRY),
        })
    }
    /// Exact additive conditional support; no caller descriptor grants this
    /// capability or widens the existing operator factory.
    pub fn local_conditional_execution_with_session(
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<Self> {
        let document = super::local::local_conditional_compiler_descriptor_with_session(session)?;
        Ok(Self {
            document: SupportDocument::Owned(document),
            execution_registry: Some(super::local::REGISTRY),
        })
    }
    #[cfg(test)]
    pub(super) fn legacy_local_for_test(
        session: &mut crate::validation_work::OriginalAuditSession,
    ) -> Result<Self> {
        let document = super::local::legacy_descriptor_for_test(session)?;
        Ok(Self {
            document: SupportDocument::Owned(document),
            execution_registry: Some(super::local::REGISTRY),
        })
    }
    pub fn builtin() -> Result<Self> {
        let limits = canonical::parse(include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/limits.json"
        ))?;
        let mode = canonical::parse(include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/mode-matrix.json"
        ))?;
        let contract = canonical::parse(include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/contract-profile.json"
        ))?;
        let files: [(&str, &[u8]); 11] = [
            (
                "AUTHORITY-AUDIT-03.md",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/AUTHORITY-AUDIT-03.md"
                ),
            ),
            (
                "CAUSAL-REVERSAL-02-REPAIR.md",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/CAUSAL-REVERSAL-02-REPAIR.md"
                ),
            ),
            (
                "ECONOMIC-BOUNDARY.md",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/ECONOMIC-BOUNDARY.md"
                ),
            ),
            (
                "authority-audit-03.schema.json",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/authority-audit-03.schema.json"
                ),
            ),
            (
                "causal-reversal-02-registry.json",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/causal-reversal-02-registry.json"
                ),
            ),
            (
                "causal-reversal-02.schema.json",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/causal-reversal-02.schema.json"
                ),
            ),
            (
                "economic-boundary.schema.json",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/economic-boundary.schema.json"
                ),
            ),
            (
                "normative-amendment.md",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/normative-amendment.md"
                ),
            ),
            (
                "publication-boundary.md",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/publication-boundary.md"
                ),
            ),
            (
                "r2-foundation/CONTRACT.md",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/r2-foundation/CONTRACT.md"
                ),
            ),
            (
                "workflow.schema.json",
                include_bytes!(
                    "../../../../../../contracts/candidates/full-suite-v1/core/workflow.schema.json"
                ),
            ),
        ];
        let implemented_mode = json!({
          "schema": "ledger-workflow-builtin-mode-matrix/2",
          "profile": "retail-selected-draft/1",
          "scope": "compile-and-draft-only",
          "policy_modes": ["component_workflow"],
          "books": ["retail"],
          "primitives": ["discount/1", "fixed/1", "outcome/1", "unit/1"],
          "fact_types": ["enum/1", "integer/1", "timestamp/1"],
          "predicate_ops": ["true/1"],
          "formula_kinds": ["discount/1", "fixed/1", "unit/1"],
          "discount_modes": ["additive"],
          "activity_primitives": ["outcome", "work"],
          "unit_normalization": "terminal-only",
          "family_basis": "frozen-original-selected-root-net",
          "family_effects": ["discount", "none", "premium"],
          "window_anchors": ["work_accepted_at"],
          "monetary_paths": {"min_hops": 1, "max_hops": 4, "max_per_slot": 1},
          "admission_enabled": false,
          "future_capacity_qualified": false,
          "original_auditor_qualified": false,
          "excluded": ["caps", "delayed-cost", "predicate-expressions", "shares", "stages", "supplier-book", "unit-conversion", "verified-predecessor-window-anchor"]
        }
        );
        if mode != implemented_mode {
            return Err(Error::new(
                "UNSUPPORTED_COMPILER_DESCRIPTOR",
                "selected code mode mismatch",
            ));
        }
        let expected_contract = json!({"schema":"ledger-workflow-builtin-contract-profile/2","profile":"retail-selected-draft/1","scope":"compile-and-draft-only","encoding_files":files.iter().map(|(path,bytes)|json!({"path":path,"sha256":raw(bytes)})).collect::<Vec<_>>()});
        if contract != expected_contract
            || mode["profile"] != "retail-selected-draft/1"
            || mode["scope"] != "compile-and-draft-only"
            || mode["admission_enabled"] != false
            || mode["future_capacity_qualified"] != false
            || mode["original_auditor_qualified"] != false
        {
            return Err(Error::new(
                "UNSUPPORTED_COMPILER_DESCRIPTOR",
                "selected embedded artifact membership",
            ));
        }
        let body = json!({"schema":"ledger-workflow-compiler-descriptor/1","compiler_semantics":"ledger-policy-compiler/1","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","facts_normalization":"ledger-facts-normalization/1","operation_normalization":"ledger-operation-normalization/2","limits_profile":"workflow-limits/1","primitive_versions":mode["primitives"],"fact_types":mode["fact_types"],"predicate_ops":mode["predicate_ops"],"formula_kinds":mode["formula_kinds"],"plan_schema":"ledger-workflow-compiled-plan/1","limits_digest":raw(canonical::CanonicalBytes::from_value(&limits)?.as_slice()),"mode_matrix_digest":raw(canonical::CanonicalBytes::from_value(&mode)?.as_slice()),"contract_manifest_digest":raw(canonical::CanonicalBytes::from_value(&contract)?.as_slice())});
        let digest = super::super::document_digest("ledger-workflow-compiler-descriptor/1", &body)?;
        let reference = DefinitionRef::new(
            "workflow-compiler-descriptor",
            &format!("wdoc_{}", &digest[7..]),
            &digest,
        )?;
        let bytes = canonical::CanonicalBytes::from_value(&body)?.into_vec();
        Ok(Self {
            document: SupportDocument::Legacy {
                body,
                reference,
                bytes,
            },
            execution_registry: None,
        })
    }
    /// Exact historical binary-owned recipe selection. A retained arbitrary
    /// descriptor cannot become executable support, and current builtin support
    /// is never substituted for this saved identity.
    pub(super) fn restore_saved(
        saved: &[u8],
        m: &mut super::super::transition_constructor::CoreMeter,
    ) -> Result<Self> {
        use super::super::transition_constructor::codec;
        const DIGEST: &str =
            "sha256:f232fc8619efbde13546b3cf6f514ac98e495f0f4de77d1ffc70dd1f79fd057b";
        const ID: &str = "wdoc_f232fc8619efbde13546b3cf6f514ac98e495f0f4de77d1ffc70dd1f79fd057b";
        if let Some(arena) = m.representation_arena() {
            let body = m.parse_owned_representation(
                &arena,
                saved,
                crate::validation_work::RepresentationKind::SourceDefinition,
            )?;
            codec::validate_selected_types(m, body.value())?;
            // Preserve canonicality-before-recipe refusal even for a saved
            // unknown schema. Both actual owning emission calls are charged;
            // no codec credit is inferred from this first equality check.
            let canonical = m.canonical_owned_representation(
                &arena,
                body.value(),
                crate::validation_work::RepresentationKind::SourceDefinition,
            )?;
            if !same_bytes(m, saved, canonical.bytes())? {
                return Err(Error::new(
                    "INTEGRITY_FAILURE",
                    "noncanonical saved compiler descriptor",
                ));
            }
            drop(canonical);
            let schema = body
                .value()
                .get("schema")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !super::compiler::same_literal(m, schema, "ledger-workflow-compiler-descriptor/1")? {
                return Err(Error::new(
                    "ORIGINAL_VERIFIER_UNAVAILABLE",
                    "saved selected compiler recipe unavailable",
                ));
            }
            let document = super::compiler::finish_owned_document(m, body, false)?;
            if !same_bytes(
                m,
                document.reference().digest().as_bytes(),
                DIGEST.as_bytes(),
            )? || !same_bytes(m, document.reference().id().as_bytes(), ID.as_bytes())?
            {
                return Err(Error::new(
                    "ORIGINAL_VERIFIER_UNAVAILABLE",
                    "saved selected compiler recipe unavailable",
                ));
            }
            return Ok(Self {
                document: SupportDocument::Owned(document),
                execution_registry: None,
            });
        }
        let body = codec::parse(m, saved, 262144)?;
        let bytes = codec::encode(m, &body, 262144)?;
        if !same_bytes(m, saved, &bytes)? {
            return Err(Error::new(
                "INTEGRITY_FAILURE",
                "noncanonical saved compiler descriptor",
            ));
        }
        let schema = codec::encode(m, &json!("ledger-workflow-compiler-descriptor/1"), 262144)?;
        let mut h = Sha256::new();
        let prefix = b"bean-counter/workflow-document/1\0";
        m.hash_bytes(prefix.len())?;
        h.update(prefix);
        for (index, part) in [
            b"[".as_slice(),
            schema.as_slice(),
            b",".as_slice(),
            bytes.as_slice(),
            b"]".as_slice(),
        ]
        .into_iter()
        .enumerate()
        {
            m.visit("support.descriptor_hash_part")?;
            if matches!(index, 0 | 2 | 4) {
                m.write_bytes(part.len())?;
            }
            m.hash_bytes(part.len())?;
            h.update(part);
        }
        let digest = format!("sha256:{}", canonical::hex_with_work(&h.finalize(), m)?);
        if !same_bytes(m, digest.as_bytes(), DIGEST.as_bytes())? {
            return Err(Error::new(
                "ORIGINAL_VERIFIER_UNAVAILABLE",
                "saved selected compiler recipe unavailable",
            ));
        }
        let reference = DefinitionRef::new("workflow-compiler-descriptor", ID, DIGEST)?;
        Ok(Self {
            document: SupportDocument::Legacy {
                body,
                reference,
                bytes,
            },
            execution_registry: None,
        })
    }
    pub fn reference(&self) -> &DefinitionRef {
        match &self.document {
            SupportDocument::Legacy { reference, .. } => reference,
            SupportDocument::Owned(doc) => doc.reference(),
        }
    }
    pub(super) fn execution_registry(&self) -> Option<&'static str> {
        self.execution_registry
    }
    pub fn canonical_value(&self) -> &Value {
        match &self.document {
            SupportDocument::Legacy { body, .. } => body,
            SupportDocument::Owned(doc) => doc.canonical_value(),
        }
    }
    pub fn canonical_bytes(&self) -> &[u8] {
        match &self.document {
            SupportDocument::Legacy { bytes, .. } => bytes,
            SupportDocument::Owned(doc) => doc.canonical_bytes(),
        }
    }
}

pub(super) fn same_bytes(
    m: &mut super::super::transition_constructor::CoreMeter,
    a: &[u8],
    b: &[u8],
) -> Result<bool> {
    use crate::validation_work;
    m.visit("support.exact_identity")?;
    if !validation_work::compare(m, a.len(), b.len())?.is_eq() {
        return Ok(false);
    }
    for (a, b) in a.iter().zip(b) {
        m.scan_bytes(2)?;
        if a != b {
            return Ok(false);
        }
    }
    Ok(true)
}
