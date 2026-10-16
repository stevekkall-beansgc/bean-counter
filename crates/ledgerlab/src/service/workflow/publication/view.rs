//! Borrowed compiler output at the publication boundary. This view neither
//! constructs source definitions nor confers executable compiler support.
use super::*;

#[derive(Clone, Copy)]
pub(in crate::service::workflow) struct SourceDocument<'a> {
    reference: &'a core::DefinitionRef,
    value: &'a Value,
    bytes: &'a [u8],
}
impl<'a> SourceDocument<'a> {
    pub(super) fn new(
        reference: &'a core::DefinitionRef,
        value: &'a Value,
        bytes: &'a [u8],
    ) -> Self {
        Self {
            reference,
            value,
            bytes,
        }
    }
    pub(super) fn reference(&self) -> &'a core::DefinitionRef {
        self.reference
    }
    pub(super) fn canonical_value(&self) -> &'a Value {
        self.value
    }
    pub(super) fn canonical_bytes(&self) -> &'a [u8] {
        self.bytes
    }
    pub(super) fn kind(&self) -> &'a str {
        self.reference.kind()
    }
    pub(super) fn id(&self) -> &'a str {
        self.reference.id()
    }
}
impl<'a> From<&'a core::DefinitionDocument> for SourceDocument<'a> {
    fn from(document: &'a core::DefinitionDocument) -> Self {
        Self::new(
            document.reference(),
            document.canonical_value(),
            document.canonical_bytes(),
        )
    }
}

pub(in crate::service::workflow) trait SourceView {
    fn documents(&self) -> impl Iterator<Item = SourceDocument<'_>>;
    fn root(&self) -> &core::DefinitionRef;
    fn compiler(&self) -> &core::DefinitionRef;
    /// Only an actual code-owned selected compiler may supply this identity.
    /// Legacy source/plan data and a public payload never select latest support.
    fn execution_registry(&self) -> Option<&'static str> {
        None
    }
    fn descriptors(&self) -> Result<Vec<core::DefinitionRef>> {
        let mut refs: Vec<_> = self
            .documents()
            .filter(|d| {
                matches!(
                    d.kind(),
                    "workflow-fact-schema" | "workflow-compiler-descriptor"
                )
            })
            .map(|d| d.reference().clone())
            .collect();
        ledgerlab_core::canonical::sort_set(&mut refs).map_err(core_error)?;
        Ok(refs)
    }
}
pub(in crate::service::workflow) trait CompiledView:
    SourceView
{
    fn plan(&self) -> SourceDocument<'_>;
}
impl SourceView for core::NormalizedPublicationSources {
    fn documents(&self) -> impl Iterator<Item = SourceDocument<'_>> {
        self.source_documents().iter().map(SourceDocument::from)
    }
    fn root(&self) -> &core::DefinitionRef {
        self.root_policy_ref()
    }
    fn compiler(&self) -> &core::DefinitionRef {
        self.compiler_descriptor().reference()
    }
}
impl SourceView for core::PreparedPublication {
    fn documents(&self) -> impl Iterator<Item = SourceDocument<'_>> {
        self.source_documents().iter().map(SourceDocument::from)
    }
    fn root(&self) -> &core::DefinitionRef {
        self.root_policy_ref()
    }
    fn compiler(&self) -> &core::DefinitionRef {
        self.compiler_descriptor().reference()
    }
}
impl CompiledView for core::PreparedPublication {
    fn plan(&self) -> SourceDocument<'_> {
        self.compiled_plan().into()
    }
}

impl SourceView for core::selected_program::SelectedProgram {
    fn documents(&self) -> impl Iterator<Item = SourceDocument<'_>> {
        self.source_documents().iter().map(|document| {
            SourceDocument::new(
                document.reference(),
                document.canonical_value(),
                document.canonical_bytes(),
            )
        })
    }
    fn root(&self) -> &core::DefinitionRef {
        self.root_policy_ref()
    }
    fn compiler(&self) -> &core::DefinitionRef {
        self.compiler_descriptor_ref()
    }
    fn execution_registry(&self) -> Option<&'static str> {
        self.execution_registry()
    }
}
impl CompiledView for core::selected_program::SelectedProgram {
    fn plan(&self) -> SourceDocument<'_> {
        let plan = self.compiled_plan();
        SourceDocument::new(
            plan.reference(),
            plan.canonical_value(),
            plan.canonical_bytes(),
        )
    }
}
