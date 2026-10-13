//! Local generic publication coordinator. Publications create no economic debt.
mod local;
mod provenance;
mod publication;
mod records;
mod request;
mod agreement;
use crate::ServiceError;
pub use local::LocalWorkflowLedger;
use serde_json::Value;
type Result<T> = std::result::Result<T, ServiceError>;
fn reject(code: &str) -> ServiceError {
    ServiceError::Rejection(code.into())
}
fn require(ok: bool, code: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(reject(code))
    }
}

// Publication boundary §Descriptor/evidence/reads/errors defines the public
// code families; registry-contract.json is the frozen exact closed vocabulary.
// Core implementation diagnostics do not extend that vocabulary.
fn core_error(error: ledgerlab_core::Error) -> ServiceError {
    match error.code {
        // Exact public meaning already named by the reviewed registry.
        "AUTHORITY_REQUIRED"
        | "BASIS_NOT_TOTAL"
        | "CORRECTION_CAPACITY_EXCEEDED"
        | "DESCRIPTOR_DEPENDENCY_MISSING"
        | "DESCRIPTOR_SCOPE_MISMATCH"
        | "DOCUMENT_DIGEST_MISMATCH"
        | "IDENTITY_CONFLICT"
        | "INTEGRITY_FAILURE"
        | "INVALID_REFERENCE"
        | "INVALID_SCALAR"
        | "INVALID_SHAPE"
        | "PLAN_DIGEST_MISMATCH"
        | "PREDICATE_ANALYSIS_LIMIT"
        | "PROGRAM_BUDGET_EXCEEDED"
        | "REQUEST_EVIDENCE_MISMATCH"
        | "REVERSAL_DEPENDENTS_REQUIRED"
        | "SEMANTIC_CONFLICT"
        | "SLOT_SEMANTICS_CHANGED"
        | "STALE_REVISION"
        | "UNSUPPORTED_COMPOSITION"
        | "UNSUPPORTED_SCHEMA_PROFILE" => reject(error.code),
        // A syntactically shaped definition's supplied content-address differs.
        "DEFINITION_DIGEST_MISMATCH" => reject("DOCUMENT_DIGEST_MISMATCH"),
        // DefinitionRef identifies an unsupported document family, not support.
        "DEFINITION_KIND" => reject("INVALID_REFERENCE"),
        // Typed JSON DTO parsing failed (including unknown nested fields).
        "JSON" => reject("INVALID_SHAPE"),
        // Typed literal/quantity units disagree or a scalar violates its type.
        "UNIT_MISMATCH" | "FACT_PRECISION" | "FACT_RANGE" | "FACT_TYPE" => reject("INVALID_SCALAR"),
        // Other internal diagnostics require a reviewed semantic mapping before
        // exposure. They cannot mint a new public refusal code or receipt.
        _ => ServiceError::Unavailable,
    }
}

#[cfg(test)]
mod error_tests {
    use super::*;
    #[test]
    fn workflow_core_diagnostics_do_not_extend_closed_public_error_vocabulary() {
        let registry: Value = serde_json::from_str(include_str!(
            "../../../../../contracts/candidates/full-suite-v1/core/registry-contract.json"
        ))
        .unwrap();
        for (core, public) in [
            ("DEFINITION_DIGEST_MISMATCH", "DOCUMENT_DIGEST_MISMATCH"),
            ("DEFINITION_KIND", "INVALID_REFERENCE"),
            ("JSON", "INVALID_SHAPE"),
            ("UNIT_MISMATCH", "INVALID_SCALAR"),
            ("FACT_RANGE", "INVALID_SCALAR"),
            ("PLAN_DIGEST_MISMATCH", "PLAN_DIGEST_MISMATCH"),
        ] {
            assert!(
                registry["errors"]
                    .as_array()
                    .unwrap()
                    .contains(&Value::String(public.into()))
            );
            assert_eq!(
                core_error(ledgerlab_core::Error {
                    code: core,
                    detail: "bounded mapping fixture".into()
                }),
                reject(public)
            );
        }
        assert_eq!(
            core_error(ledgerlab_core::Error {
                code: "FUTURE_INTERNAL_FAILURE",
                detail: "no public semantic mapping".into()
            }),
            ServiceError::Unavailable
        );
    }
}
