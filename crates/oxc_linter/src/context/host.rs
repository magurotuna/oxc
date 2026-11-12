use std::{
    cell::{Cell, OnceCell, RefCell},
    ffi::OsStr,
    path::Path,
    rc::Rc,
    sync::Arc,
};

use oxc_allocator::{Allocator, ArenaBox};
use oxc_parser::Token;
use oxc_semantic::Semantic;
use oxc_span::{SourceType, Span};
use rustc_hash::FxHashMap;

use crate::{
    AllowWarnDeny, FrameworkFlags,
    config::{
        LintConfig, LintPlugins, OxlintEnv, OxlintGlobals, OxlintSettings,
        plugins::plugin_display_name,
    },
    disable_directives::{
        DisableDirectives, DisableDirectivesBuilder, create_unused_directives_messages,
    },
    fixer::{FixKind, Message},
    frameworks::FrameworkOptions,
    module_record::ModuleRecord,
    options::LintOptions,
    rules::RuleEnum,
    utils::{ReactCompilerEnvOptions, ReactCompilerResults},
};

#[cfg(not(test))]
use crate::frameworks::{has_jest_imports, has_vitest_imports, is_jestlike_file};

use super::LintContext;

/// Stores shared information about a script block being linted.
pub struct ContextSubHost<'a> {
    /// Semantic information about the file being linted, which includes scopes, symbols and AST nodes.
    /// See [`Semantic`].
    pub(super) semantic: Semantic<'a>,
    /// Cross module information.
    pub(super) module_record: Arc<ModuleRecord>,
    /// Information about specific rules that should be disabled or enabled, via comment directives like
    /// `eslint-disable` or `eslint-disable-next-line`.
    pub(super) disable_directives: DisableDirectives,
    // Specific framework options, for example, whether the context is inside `<script setup>` in Vue files.
    pub(super) framework_options: FrameworkOptions,
    /// Parser tokens collected during parsing.
    /// Empty if parsing failed, or tokens are disabled (no JS plugins).
    pub(super) parser_tokens: ArenaBox<'a, [Token]>,
    /// The source text offset of the sub host
    pub(super) source_text_offset: u32,
}

impl<'a> ContextSubHost<'a> {
    pub(crate) fn source_text_offset(&self) -> u32 {
        self.source_text_offset
    }

    /// # Panics
    /// If `semantic.cfg()` is `None`.
    pub fn new(
        semantic: Semantic<'a>,
        module_record: Arc<ModuleRecord>,
        source_text_offset: u32,
        options: ContextSubHostOptions<'a>,
    ) -> Self {
        // We should always check for `semantic.cfg()` being `Some` since we depend on it and it is
        // unwrapped without any runtime checks after construction.
        assert!(
            semantic.cfg().is_some(),
            "`LintContext` depends on `Semantic::cfg`, Build your semantic with cfg enabled(`SemanticBuilder::with_cfg`)."
        );

        let disable_directives = DisableDirectivesBuilder::new()
            .with_respect_eslint_disable_directives(options.respect_eslint_disable_directives)
            .build(semantic.source_text(), semantic.comments());

        Self {
            semantic,
            module_record,
            source_text_offset,
            disable_directives,
            framework_options: options.framework_options,
            parser_tokens: options.parser_tokens,
        }
    }

    /// Shared reference to the [`Semantic`] analysis
    #[inline]
    pub fn semantic(&self) -> &Semantic<'a> {
        &self.semantic
    }

    /// Shared reference to the [`ModuleRecord`]
    #[inline]
    pub fn module_record(&self) -> &ModuleRecord {
        &self.module_record
    }

    /// Shared reference to the [`DisableDirectives`]
    pub fn disable_directives(&self) -> &DisableDirectives {
        &self.disable_directives
    }

    /// Shared reference to the [`FrameworkOptions`]
    pub fn framework_options(&self) -> FrameworkOptions {
        self.framework_options
    }

    /// Source text of this script block.
    #[inline]
    pub fn source_text(&self) -> &'a str {
        self.semantic.source_text()
    }
}

#[non_exhaustive]
pub struct ContextSubHostOptions<'a> {
    pub framework_options: FrameworkOptions,
    pub parser_tokens: ArenaBox<'a, [Token]>,
    pub respect_eslint_disable_directives: bool,
}

impl Default for ContextSubHostOptions<'_> {
    fn default() -> Self {
        Self {
            framework_options: FrameworkOptions::Default,
            parser_tokens: ArenaBox::new_empty_boxed_slice(),
            respect_eslint_disable_directives: true,
        }
    }
}

/// How the markup of a file uses a top-level variable of its script, from the
/// `markupReferences` of a language plugin.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MarkupUse {
    /// The markup reads the variable, or uses it as a type.
    pub read: bool,
    /// The markup assigns to the variable.
    pub write: bool,
}

/// The top-level variables of a file's script that its markup uses.
#[derive(Debug, Default)]
pub struct MarkupUses {
    uses: FxHashMap<String, MarkupUse>,
    /// If `true`, only the first section can be used by the markup (the frontmatter of an Astro
    /// file). Other `<script>` blocks of Astro files are separate client scripts.
    first_section_only: bool,
    /// Unresolved references of each section, by name, if the sections share one scope.
    /// For example, `<script setup>` of a Vue file can use the variables of its `<script>`, and the
    /// instance `<script>` of a Svelte file can use the variables of its `<script module>`.
    section_uses: Vec<FxHashMap<String, MarkupUse>>,
}

impl MarkupUses {
    pub(crate) fn new(
        references: &[crate::external_linter::MarkupReference],
        first_section_only: bool,
    ) -> Self {
        use crate::external_linter::MarkupReferenceKind;
        let mut uses = FxHashMap::<String, MarkupUse>::default();
        for reference in references {
            let entry = uses.entry(reference.name.clone()).or_default();
            match reference.kind {
                MarkupReferenceKind::Read | MarkupReferenceKind::Type => entry.read = true,
                MarkupReferenceKind::Write => entry.write = true,
                MarkupReferenceKind::ReadWrite => {
                    entry.read = true;
                    entry.write = true;
                }
            }
        }
        Self { uses, first_section_only, section_uses: vec![] }
    }
}

/// The uses of names that a section does not declare, such as the variables of another section.
fn unresolved_uses(semantic: &Semantic<'_>) -> FxHashMap<String, MarkupUse> {
    let scoping = semantic.scoping();
    let mut uses = FxHashMap::<String, MarkupUse>::default();
    for (name, reference_ids) in scoping.root_unresolved_references() {
        let mut entry = MarkupUse::default();
        for &reference_id in reference_ids {
            let flags = scoping.get_reference(reference_id).flags();
            entry.read |= flags.is_read() || flags.is_type();
            entry.write |= flags.is_write();
        }
        uses.insert(name.as_str().to_string(), entry);
    }
    uses
}

/// Stores shared information about a file being linted.
///
/// When linting a file, there are a number of shared resources that are
/// independent of the rule being linted. [`ContextHost`] stores this context
/// subset. When a lint rule is run, a [`LintContext`] with rule-specific
/// information is spawned using [`ContextHost::spawn`].
///
/// ## API Encapsulation
///
/// In most cases, lint rules should be interacting with a [`LintContext`].  The
/// only current exception to this is
/// [should_run](`crate::rule::Rule::should_run`). Just before a file is linted,
/// rules are filtered out based on that method. Creating a [`LintContext`] for
/// rules that would never get run is a waste of resources, so they must use
/// [`ContextHost`].
///
/// ## References
/// - [Flyweight Pattern](https://en.wikipedia.org/wiki/Flyweight_pattern)
#[must_use]
#[non_exhaustive]
pub struct ContextHost<'a> {
    /// A file can have multiple script entries.
    /// Some rules (like vue) need the information of the other entries.
    pub(super) sub_hosts: Vec<ContextSubHost<'a>>,
    /// Allocator that owns the parsed AST and related semantic data.
    pub(super) allocator: &'a Allocator,
    /// The current index which will be linted.
    current_sub_host_index: Cell<usize>,
    /// Diagnostics reported by the linter.
    ///
    /// Contains diagnostics for all rules across a single file.
    diagnostics: RefCell<Vec<Message>>,
    /// Whether or not to apply code fixes during linting. Defaults to
    /// [`FixKind::None`] (no fixing).
    ///
    /// Set via the `--fix`, `--fix-suggestions`, and `--fix-dangerously` CLI
    /// flags.
    pub(crate) fix: FixKind,
    /// Path to the file being linted.
    pub(super) file_path: Box<Path>,
    /// Extension of the file being linted.
    file_extension: Option<Box<OsStr>>,
    /// Global linter configuration, such as globals to include and the target
    /// environments, and other settings.
    pub(super) config: Arc<LintConfig>,
    /// Front-end frameworks that might be in use in the target file.
    pub(super) frameworks: FrameworkFlags,
    /// If true, the linter will create "ignore this section / line" fixes for all diagnostics
    with_ignore_fixes: bool,
    /// Lazily-computed shared result of the React Compiler lint run, reused by
    /// every rule in the React Compiler family (`react/hooks`, `react/refs`, …).
    /// Stays empty until the first such rule runs on this file.
    pub(super) react_compiler_results: OnceCell<ReactCompilerResults>,
    /// Config-derived environment additions for that shared run (from the
    /// `react/capitalized-calls` rule's options).
    react_compiler_env_options: ReactCompilerEnvOptions,
    /// Uses of top-level script variables in the markup of the file. `None` if no language
    /// plugin returned `markupReferences` for the file.
    markup_uses: Option<MarkupUses>,
}

impl std::fmt::Debug for ContextHost<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContextHost").field("file_path", &self.file_path).finish_non_exhaustive()
    }
}

impl<'a> ContextHost<'a> {
    /// # Panics
    /// If `sub_hosts` is empty.
    pub fn new<P: AsRef<Path>>(
        file_path: P,
        sub_hosts: Vec<ContextSubHost<'a>>,
        allocator: &'a Allocator,
        options: LintOptions,
        config: Arc<LintConfig>,
    ) -> Self {
        const DIAGNOSTICS_INITIAL_CAPACITY: usize = 16;

        assert!(
            !sub_hosts.is_empty(),
            "ContextHost requires at least one ContextSubHost to be analyzed"
        );

        let file_path = file_path.as_ref().to_path_buf().into_boxed_path();
        let file_extension = file_path.extension().map(|ext| ext.to_owned().into_boxed_os_str());

        Self {
            sub_hosts,
            allocator,
            current_sub_host_index: Cell::new(0),
            diagnostics: RefCell::new(Vec::with_capacity(DIAGNOSTICS_INITIAL_CAPACITY)),
            fix: options.fix,
            file_path,
            file_extension,
            config,
            frameworks: options.framework_hints,
            with_ignore_fixes: options.with_ignore_fixes,
            react_compiler_results: OnceCell::new(),
            react_compiler_env_options: ReactCompilerEnvOptions::default(),
            markup_uses: None,
        }
        .sniff_for_frameworks()
    }

    /// Set the config-derived environment additions for the shared React
    /// Compiler run. Must happen before the first React Compiler family rule
    /// runs on this file.
    pub fn with_react_compiler_env_options(mut self, env_options: ReactCompilerEnvOptions) -> Self {
        self.react_compiler_env_options = env_options;
        self
    }

    /// Set the uses of top-level script variables in the markup of the file.
    ///
    /// Unless only the first section can be used by the markup (Astro), the sections of the file
    /// share one scope, as in the ESLint parsers of Vue and Svelte. So the unresolved references of
    /// each section also count as uses of the variables of the other sections.
    pub(crate) fn with_markup_uses(mut self, mut markup_uses: Option<MarkupUses>) -> Self {
        if let Some(markup_uses) = &mut markup_uses
            && !markup_uses.first_section_only
            && self.sub_hosts.len() > 1
        {
            markup_uses.section_uses =
                self.sub_hosts.iter().map(|sub_host| unresolved_uses(&sub_host.semantic)).collect();
        }
        self.markup_uses = markup_uses;
        self
    }

    /// Returns `true` if a language plugin returned the markup references of this file.
    /// Then rules such as `no-unused-vars` can see which variables the markup uses.
    pub fn has_markup_references(&self) -> bool {
        self.markup_uses.is_some()
    }

    /// How the markup, and the other sections of the file, use the top-level variable `name` of
    /// the current section.
    pub(crate) fn markup_use(&self, name: &str) -> Option<MarkupUse> {
        let markup_uses = self.markup_uses.as_ref()?;
        let current_index = self.current_sub_host_index.get();
        if markup_uses.first_section_only && current_index != 0 {
            return None;
        }
        let mut result = markup_uses.uses.get(name).copied();
        for (index, section_uses) in markup_uses.section_uses.iter().enumerate() {
            if index == current_index {
                continue;
            }
            if let Some(section_use) = section_uses.get(name) {
                let entry = result.get_or_insert_default();
                entry.read |= section_use.read;
                entry.write |= section_use.write;
            }
        }
        result
    }

    /// Config-derived environment additions for the shared React Compiler run.
    pub(crate) fn react_compiler_env_options(&self) -> &ReactCompilerEnvOptions {
        &self.react_compiler_env_options
    }

    /// The current [`ContextSubHost`]
    pub fn current_sub_host(&self) -> &ContextSubHost<'a> {
        &self.sub_hosts[self.current_sub_host_index.get()]
    }

    /// Allocator that owns the parsed AST and semantic data.
    #[inline]
    pub fn allocator(&self) -> &'a Allocator {
        self.allocator
    }

    /// Get mutable reference to the current [`ContextSubHost`]
    fn current_sub_host_mut(&mut self) -> &mut ContextSubHost<'a> {
        &mut self.sub_hosts[self.current_sub_host_index.get()]
    }

    // Whether the current sub host is the first one.
    pub fn is_first_sub_host(&self) -> bool {
        self.current_sub_host_index.get() == 0
    }

    /// Shared reference to the [`Semantic`] analysis of current script block.
    #[inline]
    pub fn semantic(&self) -> &Semantic<'a> {
        &self.current_sub_host().semantic
    }

    /// Mutable reference to the [`Semantic`] analysis of current script block.
    #[inline]
    pub fn semantic_mut(&mut self) -> &mut Semantic<'a> {
        &mut self.current_sub_host_mut().semantic
    }

    /// Shared reference to the [`ModuleRecord`] of the current script block.
    #[inline]
    pub fn module_record(&self) -> &ModuleRecord {
        &self.current_sub_host().module_record
    }

    /// Shared reference to the [`DisableDirectives`] of the current script block.
    pub fn disable_directives(&self) -> &DisableDirectives {
        &self.current_sub_host().disable_directives
    }

    /// Shared reference to the parser tokens collected for this script block.
    pub fn parser_tokens(&self) -> &[Token] {
        &self.current_sub_host().parser_tokens[..]
    }

    /// Mutable reference to the parser tokens collected for this script block.
    pub fn parser_tokens_mut(&mut self) -> &mut ArenaBox<'a, [Token]> {
        &mut self.current_sub_host_mut().parser_tokens
    }

    /// Path to the file being linted.
    ///
    /// When created from a [`LintService`](`crate::service::LintService`), this
    /// will be an absolute path.
    #[inline]
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Extension of the file currently being linted, without the leading dot.
    #[inline]
    pub fn file_extension(&self) -> Option<&OsStr> {
        self.file_extension.as_deref()
    }

    /// The source type of the file being linted, e.g. JavaScript, TypeScript,
    /// CJS, ESM, etc.
    #[inline]
    pub fn source_type(&self) -> &SourceType {
        self.semantic().source_type()
    }

    #[inline]
    pub fn plugins(&self) -> LintPlugins {
        self.config.plugins
    }

    #[inline]
    pub fn settings(&self) -> &OxlintSettings {
        &self.config.settings
    }

    #[inline]
    pub fn globals(&self) -> &OxlintGlobals {
        &self.config.globals
    }

    #[inline]
    pub fn env(&self) -> &OxlintEnv {
        &self.config.env
    }

    #[inline]
    pub fn source_text(&self) -> &'a str {
        self.current_sub_host().source_text()
    }

    /// Add a diagnostic message to the end of the list of diagnostics. Can be used
    /// by any rule to report issues.
    #[inline]
    pub(crate) fn push_diagnostic(&self, mut diagnostic: Message) {
        if self.with_ignore_fixes {
            let source_text = self.source_text();
            diagnostic.add_ignore_fix(self.current_sub_host().source_text_offset, source_text);
        }
        if self.current_sub_host().source_text_offset != 0 {
            diagnostic.move_offset(self.current_sub_host().source_text_offset);
        }
        self.diagnostics.borrow_mut().push(diagnostic);
    }

    /// Add the reports of JS plugin rules that ran on the syntax tree of a language plugin.
    ///
    /// Each item is a report with spans in the whole file, and the `plugin/rule` name of its rule
    /// (`None` for errors that are not reports of a rule). A disable directive applies to a report
    /// if the directive is in the section (`<script>` block) where the report starts, as for the
    /// reports of native rules.
    ///
    /// `markup_directives` are the directives of the markup comments, with spans in the whole file.
    /// They apply after the directives of the sections.
    pub(crate) fn add_language_file_diagnostics(
        &self,
        diagnostics: Vec<(Message, Vec<String>)>,
        markup_directives: Option<&DisableDirectives>,
    ) {
        let mut out = self.diagnostics.borrow_mut();
        for (message, rule_names) in diagnostics {
            if !rule_names.is_empty() {
                let Span { start, end, .. } = message.span;
                let section = self.sub_hosts.iter().find(|sub_host| {
                    let offset = sub_host.source_text_offset;
                    #[expect(clippy::cast_possible_truncation)]
                    let section_end = offset + sub_host.source_text().len() as u32;
                    start >= offset && start <= section_end
                });
                if let Some(section) = section {
                    let offset = section.source_text_offset;
                    let span = Span::new(start - offset, end.saturating_sub(offset));
                    if section.disable_directives.contains_any(&rule_names, span) {
                        continue;
                    }
                }
                if markup_directives
                    .is_some_and(|directives| directives.contains_any(&rule_names, message.span))
                {
                    continue;
                }
            }
            out.push(message);
        }
    }

    /// Remove the reports of native rules that a directive in a markup comment covers.
    /// The spans of the reports and of `markup_directives` are positions in the whole file.
    pub(crate) fn apply_markup_directives(&self, markup_directives: &DisableDirectives) {
        self.diagnostics.borrow_mut().retain(|message| {
            let Some(rule_name) = message.error.code.number.as_deref() else { return true };
            !markup_directives.contains(rule_name, message.span)
        });
    }

    // Append a list of diagnostics. Only used in report_unused_directives.
    fn append_diagnostics(&self, mut diagnostics: Vec<Message>) {
        if self.with_ignore_fixes {
            let source_text = self.source_text();
            for diagnostic in &mut diagnostics {
                diagnostic.add_ignore_fix(self.current_sub_host().source_text_offset, source_text);
            }
        }
        if self.current_sub_host().source_text_offset != 0 {
            let offset = self.current_sub_host().source_text_offset;
            for diagnostic in &mut diagnostics {
                diagnostic.move_offset(offset);
            }
        }
        self.diagnostics.borrow_mut().extend(diagnostics);
    }

    pub fn rewind_sub_hosts(&self) {
        self.current_sub_host_index.set(0);
    }

    pub fn next_sub_host(&self) -> bool {
        let next_index = self.current_sub_host_index.get() + 1;
        if next_index < self.sub_hosts.len() {
            self.current_sub_host_index.set(next_index);
            true
        } else {
            false
        }
    }

    /// report unused enable/disable directives, add these as Messages to diagnostics
    pub fn report_unused_directives(&self, severity: AllowWarnDeny) {
        self.append_diagnostics(create_unused_directives_messages(
            self.disable_directives(),
            severity,
            Some((self.fix, self.source_text())),
        ));
    }

    /// Take ownership of all diagnostics collected during linting.
    pub fn take_diagnostics(&self) -> Vec<Message> {
        // NOTE: diagnostics are only ever borrowed here and in push_diagnostic, append_diagnostics.
        // The latter drops the reference as soon as the function returns, so
        // this should never panic.
        let mut messages = self.diagnostics.borrow_mut();
        std::mem::take(&mut *messages)
    }

    /// Take ownership of the disable directives from the first sub host.
    /// This consumes the `ContextHost`.
    ///
    /// # Panics
    /// Panics if `sub_hosts` contains more than one sub host.
    pub fn into_disable_directives(self) -> Option<DisableDirectives> {
        assert!(
            self.sub_hosts.len() <= 1,
            "into_disable_directives expects at most one sub host, but found {}",
            self.sub_hosts.len()
        );
        self.sub_hosts.into_iter().next().map(|sub_host| sub_host.disable_directives)
    }

    /// Take ownership of the disable directives of all sub hosts, merged into one table whose
    /// spans are positions in the whole file. This consumes the `ContextHost`.
    ///
    /// Use this for files with more than one section, such as Vue, Svelte, and Astro files.
    pub fn into_merged_disable_directives(self) -> DisableDirectives {
        self.into_merged_disable_directives_with(None)
    }

    /// Same as [`ContextHost::into_merged_disable_directives`], and also merges the directives of
    /// the markup comments (with spans in the whole file), if any.
    pub(crate) fn into_merged_disable_directives_with(
        self,
        markup_directives: Option<DisableDirectives>,
    ) -> DisableDirectives {
        DisableDirectives::merge_sections(
            self.sub_hosts
                .into_iter()
                .map(|sub_host| (sub_host.disable_directives, sub_host.source_text_offset))
                .chain(markup_directives.map(|directives| (directives, 0))),
        )
    }

    #[cfg(debug_assertions)]
    pub fn get_diagnostics(&self, cb: impl FnOnce(&mut Vec<Message>)) {
        cb(self.diagnostics.borrow_mut().as_mut());
    }

    #[cfg(debug_assertions)]
    pub fn diagnostic_count(&self) -> usize {
        self.diagnostics.borrow().len()
    }

    /// Creates a new [`LintContext`] for a specific rule.
    pub fn spawn(self: Rc<Self>, rule: &RuleEnum, severity: AllowWarnDeny) -> LintContext<'a> {
        let rule_name = rule.name();
        let plugin_name = rule.plugin_name();

        LintContext {
            parent: self,
            current_rule_name: rule_name,
            current_plugin_name: plugin_name,
            current_plugin_display_name: plugin_display_name(plugin_name),
            #[cfg(debug_assertions)]
            current_rule_fix_capabilities: rule.fix(),
            severity: severity.into(),
        }
    }

    /// Run one native rule's callbacks for allocation profiling, without linter setup or teardown.
    ///
    /// Uses the same eligibility and AST type filters as the regular rule runner.
    #[cfg(feature = "track_allocations")]
    pub fn run_rule_for_allocations(self: Rc<Self>, rule: &RuleEnum, severity: AllowWarnDeny) {
        let run_info = rule.run_info();
        let ast_types = rule.types_info();
        if rule.is_tsgolint_rule()
            || (run_info == crate::RuleRunFunctionsImplemented::Run
                && ast_types.is_some_and(|types| !self.semantic().nodes().contains_any(types)))
            || !rule.should_run(&self)
        {
            return;
        }

        let ctx = self.spawn(rule, severity);
        if run_info.is_run_once_implemented() {
            rule.run_once::<false>(&ctx, None);
        }
        if run_info.is_run_implemented() {
            for node in ctx.nodes() {
                if ast_types.is_none_or(|types| types.has(node.kind().ty())) {
                    rule.run::<false>(node, &ctx, None);
                }
            }
        }
    }

    /// Creates a new [`LintContext`] for testing purposes only.
    #[cfg(test)]
    pub(crate) fn spawn_for_test(self: Rc<Self>) -> LintContext<'a> {
        LintContext {
            parent: Rc::clone(&self),
            current_rule_name: "",
            current_plugin_name: "eslint",
            current_plugin_display_name: "eslint",
            #[cfg(debug_assertions)]
            current_rule_fix_capabilities: crate::rule::RuleFixMeta::None,
            severity: oxc_diagnostics::Severity::Warning,
        }
    }

    /// Inspect the target file for clues about what frameworks are being used.
    /// Should only be called once immediately after construction.
    ///
    /// Before invocation, `self.frameworks` contains hints obtained at the
    /// project level. For example, Oxlint may (eventually) search for a
    /// `package.json`` and look for relevant dependencies. This method builds
    /// on top of those hints, providing a more granular understanding of the
    /// frameworks in use.
    #[cfg(not(test))]
    fn sniff_for_frameworks(mut self) -> Self {
        if self.plugins().has_test() {
            let vitest_like = has_vitest_imports(self.module_record());
            let jest_like =
                is_jestlike_file(&self.file_path) || has_jest_imports(self.module_record());

            self.frameworks.set(FrameworkFlags::Vitest, vitest_like);
            self.frameworks.set(FrameworkFlags::Jest, jest_like);
        }

        self
    }

    /// Currently Oxlint isn't searching if Jest or Vitest is in `package.json`.
    /// Once the method read the `package.json` we can discard this conditional flag,
    /// and rely on the tester to create the correct `package.json` to have a reliable
    /// sniff method.
    #[cfg(test)]
    fn sniff_for_frameworks(mut self) -> Self {
        if self.plugins().has_test() {
            self.frameworks.set(FrameworkFlags::Vitest, self.plugins().has_vitest());

            self.frameworks.set(FrameworkFlags::Jest, self.plugins().has_jest());
        }

        self
    }

    /// Returns the framework hints for the target file.
    #[inline]
    pub fn frameworks(&self) -> FrameworkFlags {
        self.frameworks
    }

    pub fn frameworks_options(&self) -> FrameworkOptions {
        self.current_sub_host().framework_options
    }

    pub fn other_file_hosts(&self) -> Vec<&ContextSubHost<'a>> {
        self.sub_hosts
            .iter()
            .enumerate()
            .filter(|&(index, _)| index != self.current_sub_host_index.get())
            .map(|(_, sub_host)| sub_host)
            .collect()
    }
}

impl<'a> From<ContextHost<'a>> for Vec<Message> {
    fn from(ctx_host: ContextHost<'a>) -> Self {
        ctx_host.diagnostics.into_inner()
    }
}
