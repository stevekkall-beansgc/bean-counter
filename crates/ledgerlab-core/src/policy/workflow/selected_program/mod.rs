//! Actual selected source compiler. Its plan/data cannot authorize persistence.
mod model;
mod support;
pub use support::SelectedCompilerSupport;

mod compiler;
pub use compiler::{
    SelectedDocument, SelectedProgram, SelectedSourceInput, StaticAnalysis, SelectedExecutionReloadKey,
    compile_selected_program, compile_selected_publication, restore_selected_program, rehydrate_selected_execution, load_local_selected_execution, normalize_local_publication_input,NormalizedLocalSourceInput,
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
pub use local::local_compiler_descriptor_with_session;
