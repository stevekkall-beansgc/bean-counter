//! Pure generic workflow semantics. The current increment validates fact
//! schemas, facts and reusable templates; publication and economic acceptance
//! remain separate capabilities.
mod facts;
pub use facts::{FactSchema, FactValue, NormalizedFacts, SignedScalar};
mod template;
pub use template::{SlotTemplate, TemplateBook, TemplateComponent, TemplateFamily};
mod product;
pub use product::{
    Activity, BindingRelation, LinkDefinition, MissingDependency, Primitive, Product,
    UnitDefinition,
};
mod compiler;
pub use compiler::{
    candidate_dependency_refs, compile_publication, normalize_definition,
    normalize_publication_sources, normalize_retained_definition, CompilerDescriptor,
    DefinitionDocument, DefinitionRef, NormalizedDefinition, NormalizedPublicationSources,
    PreparedPublication, VerifiedCompilerSupport,
};

fn document_digest(schema: &str, body: &serde_json::Value) -> crate::Result<String> {
    document_digest_with_work(
        schema,
        body,
        &mut crate::validation_work::NoopValidationWork,
    )
}
fn document_digest_with_work(
    schema: &str,
    body: &serde_json::Value,
    work: &mut dyn crate::validation_work::ValidationWork,
) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let domain = b"bean-counter/workflow-document/1\0";
    let bytes = crate::canonical::CanonicalBytes::from_value_with_work(
        &serde_json::json!([schema, body]),
        work,
    )?;
    work.hash_bytes(domain.len())?;
    let mut hash = Sha256::new();
    hash.update(domain);
    work.scan_bytes(bytes.as_slice().len())?;
    work.hash_bytes(bytes.as_slice().len())?;
    hash.update(bytes.as_slice());
    work.write_bytes(32)?;
    let hash = hash.finalize();
    let hex = crate::canonical::hex_with_work(&hash, work)?;
    work.write_bytes(71)?;
    Ok(format!("sha256:{hex}"))
}

mod slots;
pub use slots::{RoleAssignment, SlotCatalog, SlotInstantiation};
mod ceiling;
pub use ceiling::SourceCeiling;
mod evidence;
pub use evidence::EvidenceInput;
mod agreement_ref;
pub use agreement_ref::AgreementRef;
mod grant;
pub use grant::SourceGrant;
mod enrollment;
pub use enrollment::SourceEnrollment;
mod source_state;
pub use source_state::SourceState;
mod transport;
pub use transport::{
    DefinitionFinalizeInput, DefinitionFragment, DefinitionFragmentInput, SourceRetentionInput,
};
mod terms;
pub use terms::{AcceptanceTerms, AssentRecord, SlotBindingTerms};
