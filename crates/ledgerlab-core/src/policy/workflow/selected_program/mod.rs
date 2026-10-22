//! Actual selected source compiler. Its plan/data cannot authorize persistence.
mod model;
mod support;
pub use support::SelectedCompilerSupport;

mod compiler;
pub use compiler::{
    compile_selected_program, compile_selected_publication, load_local_selected_execution,
    normalize_local_publication_input, rehydrate_selected_execution, restore_selected_program,
    NormalizedLocalSourceInput, SelectedDocument, SelectedExecutionReloadKey, SelectedProgram,
    SelectedSourceInput, StaticAnalysis,
};

#[cfg(test)]
mod tests;

mod validate;

mod normalize;
// Optional AP projection implementation remains retained in supplier_projection.rs;
// the current app compilation/publication dependency does not execute AP support.

mod app_declarations;
mod owning_plan;
mod owning_policy;

mod policy_decode;

pub(in crate::policy::workflow) use compiler::finish_owned_document;

mod local;
mod local_admission;
pub use local::{local_compiler_descriptor_with_session, local_conditional_compiler_descriptor_with_session};
