//! Exact local recipe document DATA. This producer grants no execution or
//! financial preparation capability and accepts no caller-selected catalog.
use super::{compiler::finish_owned_document, SelectedDocument};
use crate::policy::workflow::transition_constructor::{codec, shape};
use crate::{
    validation_work::{OriginalAuditSession, RepresentationKind},
    Result,
};
use sha2::{Digest, Sha256};
#[path = "local_assets.rs"]
mod assets;
#[path = "local_operator_assets.rs"]
mod operator_assets;
#[path = "local_parent_assets.rs"]
mod parent_assets;
pub(super) const REGISTRY: &str =
    "sha256:d0a21becb08150a781770cfef80b5521eeb93c3b45aa9ebdab23c800ef44ee4a";
pub(super) const DESCRIPTOR:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-compiler6-data.json");
pub(super) const PARENT_DESCRIPTOR:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-compiler7-data.json");
pub(super) const OPERATOR_DESCRIPTOR:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-compiler8-data.json");
const OPERATOR_MANIFEST:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-contract-profile3.json");
const MANIFEST:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-contract-profile.json");
const PARENT_MANIFEST:&[u8]=include_bytes!("../../../../../../contracts/candidates/full-suite-v1/core/runtime-support/local-contract-profile2.json");

pub fn local_compiler_descriptor_with_session(
    s: &mut OriginalAuditSession,
) -> Result<SelectedDocument> {
    if s.resource_limits().is_some() {
        compiler_descriptor(
            s,
            OPERATOR_DESCRIPTOR,
            OPERATOR_MANIFEST,
            operator_assets::ARTIFACTS,
        )
    } else {
        compiler_descriptor(
            s,
            PARENT_DESCRIPTOR,
            PARENT_MANIFEST,
            parent_assets::ARTIFACTS,
        )
    }
}
#[cfg(test)]
pub(super) fn legacy_descriptor_for_test(s: &mut OriginalAuditSession) -> Result<SelectedDocument> {
    compiler_descriptor(s, DESCRIPTOR, MANIFEST, assets::ARTIFACTS)
}
fn compiler_descriptor(
    s: &mut OriginalAuditSession,
    descriptor: &[u8],
    manifest_bytes: &[u8],
    artifacts: &[(&str, &[u8])],
) -> Result<SelectedDocument> {
    let result = (|| {
        let a = s.arena().shared_handle();
        let m = s.meter_mut();
        let manifest =
            m.parse_owned_representation(&a, manifest_bytes, RepresentationKind::SourceDefinition)?;
        codec::validate_selected_types(m, manifest.value())?;
        let bytes = m.canonical_owned_representation(
            &a,
            manifest.value(),
            RepresentationKind::SourceDefinition,
        )?;
        if !super::support::same_bytes(m, bytes.bytes(), manifest_bytes)? {
            return Err(shape::fail("local defining manifest canonicality"));
        }
        let rows = shape::array(m, manifest.value(), "encoding_files")?;
        if !crate::validation_work::compare(m, rows.len(), artifacts.len())?.is_eq() {
            return Err(shape::fail("local defining asset membership"));
        }
        for (row, (path, raw)) in rows.iter().zip(artifacts) {
            m.visit("local.descriptor_defining_asset")?;
            let actual_path = shape::text(m, row, "path")?;
            if !shape::same_text(m, actual_path, path)? {
                return Err(shape::fail("local defining artifact path"));
            }
            m.hash_bytes(raw.len())?;
            let digest = format!(
                "sha256:{}",
                crate::canonical::hex_with_work(&Sha256::digest(raw), m)?
            );
            let actual_hash = shape::text(m, row, "sha256")?;
            if !shape::same_text(m, actual_hash, &digest)? {
                return Err(shape::fail("local defining artifact bytes"));
            }
        }
        let body =
            m.parse_owned_representation(&a, descriptor, RepresentationKind::SourceDefinition)?;
        codec::validate_selected_types(m, body.value())?;
        m.hash_bytes(bytes.bytes().len())?;
        let digest = format!(
            "sha256:{}",
            crate::canonical::hex_with_work(&Sha256::digest(bytes.bytes()), m)?
        );
        let actual_manifest = shape::text(m, body.value(), "contract_manifest_digest")?;
        if !shape::same_text(m, actual_manifest, &digest)? {
            return Err(shape::fail("local descriptor defining manifest"));
        }
        finish_owned_document(m, body, false)
    })();
    if result.is_err() {
        s.invalidate();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        canonical::{self, audit_arena::AuditArena},
        policy::workflow::{document_digest, selected_program::*, DefinitionRef},
    };
    use serde_json::{json, Value};
    #[test]
    fn actual_first_policy_local_compile_load_and_incoming_intent() {
        for version in [6, 7, 8] {
            let arena = if version == 8 {
                AuditArena::with_resource_limits(crate::validation_work::ResourceLimits::unlimited()).unwrap()
            } else {
                AuditArena::new().unwrap()
            };
            let mut s = OriginalAuditSession::for_resource_profile_v2(&arena).unwrap();
            let support = if version == 8 {
                SelectedCompilerSupport::local_execution_with_session(&mut s).unwrap()
            } else if version == 7 {
                SelectedCompilerSupport::local_execution_with_session(&mut s).unwrap()
            } else {
                SelectedCompilerSupport::legacy_local_for_test(&mut s).unwrap()
            };
            let data: Value =
                serde_json::from_slice(include_bytes!("fixtures/local-first-policy-input.json"))
                    .unwrap();
            let mut docs = data["documents"].as_array().unwrap().clone();
            let policy = docs
                .iter_mut()
                .find(|v| v["schema"] == "ledger-workflow-policy/4")
                .unwrap();
            policy["compiler_ref"] = serde_json::to_value(support.reference()).unwrap();
            let digest = document_digest("ledger-workflow-policy/4", policy).unwrap();
            let root = DefinitionRef::new(
                "workflow-policy",
                &format!("wdoc_{}", &digest[7..]),
                &digest,
            )
            .unwrap();
            docs.push(support.canonical_value().clone());
            let input = || SelectedSourceInput {
                documents: docs
                    .iter()
                    .map(|d| canonical::CanonicalBytes::from_value(d).unwrap().into_vec())
                    .collect(),
                root_policy_ref: root.clone(),
            };
            let program = compile_selected_publication(input(), &support, &mut s).unwrap();
            if version == 8 {
                let mut plain =
                    crate::validation_work::economic_meter::Meter::for_compile(1048576).unwrap();
                let plain_program =
                    compile_selected_program(input(), &support, &mut plain).unwrap();
                assert_eq!(
                    plain_program.compiled_plan().canonical_bytes(),
                    program.compiled_plan().canonical_bytes()
                );
            }
            assert!(program.uses_local_execution_recipe());
            assert_eq!(program.uses_local_parent_budget_recipe(), version >= 7);
            assert_eq!(
                program.compiled_plan().canonical_value()["schema"],
                format!("ledger-workflow-compiled-plan/{version}")
            );
            assert_eq!(
                program.compiled_plan().canonical_value()["worst_case"]
                    .get("runtime_work")
                    .is_none(),
                version >= 7
            );
            if version == 6 {
                assert_eq!(
                    support.reference().id(),
                    "wdoc_23136c6a9ab71bb61978b7b972e6afed28ecd0612d525ac2e8700a96da3cc9fa"
                );
            } else if version == 7 {
                assert_eq!(
                    support.reference().id(),
                    "wdoc_f18db526e992b55be1b95b8283aac73040aaa33888c2c986e51a7bcbffab967e"
                );
            }
            if version == 8 {
                for field in ["compile_work", "loaded_refs", "bytes"] {
                    assert!(program.compiled_plan().canonical_value()["worst_case"]
                        .get(field)
                        .is_none());
                }
            }
            let plan = program.compiled_plan().canonical_bytes().to_vec();
            let plan_ref = program.compiled_plan().reference().clone();
            let source = SelectedSourceInput {
                documents: program
                    .source_documents()
                    .iter()
                    .map(|d| d.canonical_bytes().to_vec())
                    .collect(),
                root_policy_ref: program.root_policy_ref().clone(),
            };
            let read_arena = AuditArena::with_resource_limits(
                crate::validation_work::ResourceLimits::unlimited(),
            )
            .unwrap();
            let mut read_session =
                OriginalAuditSession::for_resource_profile_v2(&read_arena).unwrap();
            let loaded =
                load_local_selected_execution(source, &plan, &plan_ref, &mut read_session).unwrap();
            assert_eq!(loaded.uses_local_parent_budget_recipe(), version >= 7);
            assert_eq!(
                loaded.compiled_plan().canonical_bytes(),
                program.compiled_plan().canonical_bytes()
            );
            assert_eq!(loaded.static_analysis().compile_work, None);
            let normalized =
                normalize_local_publication_input(input(), &loaded, &mut read_session).unwrap();
            assert_eq!(normalized.root_policy_ref(), program.root_policy_ref());
            assert_eq!(
                normalized.source_documents().len(),
                program.source_documents().len()
            );
            for actual in normalized.source_documents() {
                assert!(program
                    .source_documents()
                    .iter()
                    .any(|p| p.reference() == actual.reference()
                        && p.canonical_bytes() == actual.canonical_bytes()));
            }
            let schema = loaded
                .source_documents()
                .iter()
                .find(|d| d.canonical_value()["schema_id"] == "support:work-facts")
                .unwrap();
            let original = json!({"schema":"ledger-workflow-work/1","fact_schema_ref":schema.reference(),"facts":{"support:quantity":"20"},"fact_proofs":[]});
            let mut incoming = original.clone();
            incoming["facts"]["support:quantity"] = json!("00020");
            let original = canonical::CanonicalBytes::from_value(&original).unwrap();
            let incoming = canonical::CanonicalBytes::from_value(&incoming).unwrap();
            let normalized =
                crate::policy::workflow::transition_constructor::normalize_local_saved_intent(
                    &loaded,
                    original.as_slice(),
                    incoming.as_slice(),
                    &mut read_session,
                )
                .unwrap();
            assert_eq!(normalized.bytes(), original.as_slice());
            if version < 8 {
                let reject_arena = AuditArena::with_resource_limits(
                    crate::validation_work::ResourceLimits::unlimited(),
                )
                .unwrap();
                let mut reject_session =
                    OriginalAuditSession::for_resource_profile_v2(&reject_arena).unwrap();
                assert_eq!(
                    compile_selected_publication(input(), &support, &mut reject_session)
                        .err()
                        .unwrap()
                        .detail,
                    "operator resources require exact local compiler8"
                );
            }
            let mut changed = input();
            let mut value: Value = canonical::parse(&changed.documents[5]).unwrap();
            value["version"] = json!("2");
            changed.documents[5] = canonical::CanonicalBytes::from_value(&value)
                .unwrap()
                .into_vec();
            assert!(
                normalize_local_publication_input(changed, &loaded, &mut read_session).is_err()
            );
        }
    }
}
