//! Actual selected source compiler. Its plan/data cannot authorize persistence.
mod model;
mod support;
pub use support::SelectedCompilerSupport;

mod compiler;
pub use compiler::{
    SelectedDocument, SelectedProgram, SelectedSourceInput, StaticAnalysis,
    compile_selected_program, compile_selected_publication, restore_selected_program,
};

#[cfg(test)]
mod tests;

mod validate;

mod normalize;
