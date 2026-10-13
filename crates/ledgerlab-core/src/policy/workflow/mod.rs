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
    compile_publication, normalize_publication_sources, CompilerDescriptor, DefinitionDocument,
    DefinitionRef, NormalizedPublicationSources, PreparedPublication, VerifiedCompilerSupport,
};

fn document_digest(schema: &str, body: &serde_json::Value) -> crate::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    hash.update(b"bean-counter/workflow-document/1\0");
    hash.update(
        crate::canonical::CanonicalBytes::from_value(&serde_json::json!([schema, body]))?
            .as_slice(),
    );
    Ok(format!(
        "sha256:{}",
        crate::canonical::hex(&hash.finalize())
    ))
}

mod slots;
pub use slots::{RoleAssignment, SlotCatalog, SlotInstantiation};
mod ceiling;
pub use ceiling::SourceCeiling;
mod evidence;
pub use evidence::EvidenceInput;
