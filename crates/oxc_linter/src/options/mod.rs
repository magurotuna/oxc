use crate::{FrameworkFlags, fixer::FixKind};

mod allow_warn_deny;
mod filter;

pub use allow_warn_deny::AllowWarnDeny;
pub use filter::{InvalidFilterKind, LintFilter, LintFilterKind};

/// Subset of options used directly by the linter.
#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(test, derive(PartialEq, Eq))]
pub struct LintOptions {
    pub fix: FixKind,
    pub framework_hints: FrameworkFlags,
    pub report_unused_directive: Option<AllowWarnDeny>,
    // If true, the linter will create "ignore this section / line" fixes for all diagnostics
    pub with_ignore_fixes: bool,
    /// Whether the type-aware backend also lints Vue, Svelte, and Astro files
    /// (`--run-external-code`). Only then are the disable directives of these files returned,
    /// so that unused directives are reported after the type-aware reports. Otherwise they are
    /// reported with the other reports of the file, and their fixes apply in the same run.
    pub type_aware_partial_loader_files: bool,
}
