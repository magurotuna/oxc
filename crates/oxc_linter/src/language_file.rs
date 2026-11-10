//! Linting of files with language plugins.
//!
//! A language plugin is a JS module that parses a file (for example, a Vue single-file component)
//! into its own syntax tree. JS plugin rules run on that syntax tree, in one JS call per file.
//! Rust rules still run on the `<script>` blocks that the partial loader finds.

use std::{iter, path::Path, sync::Arc};

use oxc_ast_visit::utf8_to_utf16::Utf8ToUtf16;
use oxc_diagnostics::OxcDiagnostic;
use oxc_span::Span;

use crate::{
    AllowWarnDeny, ExternalOptionsId, ExternalRuleId, LanguagePluginBinding, Linter,
    config::LintConfig,
    external_linter::{
        GlobalsAndEnvs, LintFileFailure, LintLanguageFileArgs, LintLanguageFileOutput,
        MarkupComment, MarkupReference, convert_and_merge_js_fixes,
    },
    fixer::{FixKind, Message, PossibleFixes},
    timing::RuleTimingStore,
};

const BOM: &str = "\u{feff}";
#[expect(clippy::cast_possible_truncation)]
const BOM_LEN: u32 = BOM.len() as u32;

/// Result of linting a file with a language plugin. All spans are UTF-8 positions in the whole file.
#[derive(Debug, Default)]
pub struct LanguageFileResult {
    /// Reports of JS plugin rules, with the `plugin/rule` name for disable directives.
    /// Errors of the plugin itself have no rule name.
    pub diagnostics: Vec<(Message, Option<String>)>,
    /// References from the markup to top-level script variables.
    pub markup_references: Vec<MarkupReference>,
    /// Comments in the markup.
    pub markup_comments: Vec<MarkupComment>,
}

impl Linter {
    /// Parse a file with its language plugin, and run JS plugin rules on the plugin's syntax tree.
    ///
    /// `source_text` is the whole file. Returns `None` if there are no callbacks for language plugins.
    #[expect(clippy::too_many_arguments)]
    pub fn lint_language_file(
        &self,
        path: &Path,
        source_text: &str,
        binding: &LanguagePluginBinding,
        external_rules: &[(ExternalRuleId, ExternalOptionsId, AllowWarnDeny)],
        config: &LintConfig,
        fix: FixKind,
        rule_timing_store: Option<&RuleTimingStore>,
    ) -> Option<LanguageFileResult> {
        let callbacks = self.external_linter.as_ref()?.language_plugins.as_ref()?;
        let plugin_name =
            &self.config.external_plugin_store().language_plugin(binding.plugin_id).name;
        let path_string = path.to_string_lossy();

        let mut result = LanguageFileResult::default();
        let push_error = |result: &mut LanguageFileResult, message: String| {
            result
                .diagnostics
                .push((Message::new(OxcDiagnostic::error(message), PossibleFixes::None), None));
        };

        // JS gets the source text without a BOM, as for other files
        let has_bom = source_text.starts_with(BOM);
        let js_source_text = if has_bom { &source_text[BOM_LEN as usize..] } else { source_text };
        let span_converter = if has_bom {
            Utf8ToUtf16::new_with_offset(js_source_text, BOM_LEN)
        } else {
            Utf8ToUtf16::new(js_source_text)
        };
        #[expect(clippy::cast_possible_truncation)]
        let mut source_text_utf16_len = source_text.len() as u32;
        if let Some(mut converter) = span_converter.converter() {
            converter.convert_offset(&mut source_text_utf16_len);
        }
        let to_utf8_span = |start: u32, end: u32| {
            // Clamp offsets to the end of the source, and represent reversed ranges as empty spans
            let start = start.min(source_text_utf16_len);
            let end = end.min(source_text_utf16_len).max(start);
            let mut span = Span::new(start, end);
            span_converter.convert_span_back(&mut span);
            span
        };

        let settings_json = match &config.settings.json {
            Some(json) => serde_json::to_string(json).unwrap_or_else(|e| {
                push_error(
                    &mut result,
                    format!("Error serializing settings.\nFile path: {path_string}\n{e}"),
                );
                "{}".to_string()
            }),
            None => "{}".to_string(),
        };
        let globals_json = serde_json::to_string(&GlobalsAndEnvs::from_config(config))
            .unwrap_or_else(|e| {
                push_error(
                    &mut result,
                    format!("Error serializing globals.\nFile path: {path_string}\n{e}"),
                );
                "{}".to_string()
            });

        let output = (callbacks.lint_language_file)(LintLanguageFileArgs {
            file_path: path_string.to_string(),
            plugin_id: binding.plugin_id.raw(),
            plugin_options_json: Arc::clone(&binding.options_json),
            source_text: js_source_text.to_string(),
            rule_ids: external_rules.iter().map(|(rule_id, _, _)| rule_id.raw()).collect(),
            options_ids: external_rules.iter().map(|(_, options_id, _)| options_id.raw()).collect(),
            settings_json,
            globals_json,
            collect_timings: rule_timing_store.is_some(),
            workspace_uri: self.workspace_uri.as_ref().map(ToString::to_string),
        });

        let LintLanguageFileOutput { diagnostics, timings, markup_references, markup_comments } =
            match output {
                Ok(output) => output,
                Err(LintFileFailure { message, timings }) => {
                    self.record_external_timings(rule_timing_store, external_rules, timings);
                    push_error(
                        &mut result,
                        format!(
                            "Error running language plugin `{plugin_name}`.\nFile path: {path_string}\n{message}"
                        ),
                    );
                    return Some(result);
                }
            };
        self.record_external_timings(rule_timing_store, external_rules, timings);

        for diagnostic in diagnostics {
            let Some(&(external_rule_id, _options_id, severity)) =
                external_rules.get(diagnostic.rule_index as usize)
            else {
                continue;
            };
            let (rule_plugin_name, rule_name) =
                self.config.resolve_plugin_rule_names(external_rule_id);
            let span = to_utf8_span(diagnostic.start, diagnostic.end);

            // Convert a `Vec<JsFix>` to a `Fix`, including converting spans back to UTF-8
            let mut fix_errors = vec![];
            let mut create_fix = |fixes, fix_kind: FixKind| match convert_and_merge_js_fixes(
                fixes,
                source_text,
                &span_converter,
                has_bom,
            ) {
                Ok(fix) => Some(fix.with_kind(fix_kind)),
                Err(err) => {
                    let fixes_type = if fix_kind.contains(FixKind::Suggestion) {
                        "suggestions"
                    } else {
                        "fixes"
                    };
                    fix_errors.push(format!(
                        "Plugin `{rule_plugin_name}/{rule_name}` returned invalid {fixes_type}.\nFile path: {path_string}\n{err}"
                    ));
                    None
                }
            };

            let fix_of_report = diagnostic.fixes.and_then(|fixes| create_fix(fixes, FixKind::Fix));
            let possible_fixes = if let Some(suggestions) = diagnostic.suggestions
                && fix.can_apply(FixKind::Suggestion)
            {
                let suggestions = suggestions
                    .into_iter()
                    .filter_map(|suggestion| {
                        create_fix(suggestion.fixes, FixKind::Suggestion)
                            .map(|fix| fix.with_message(suggestion.message))
                    })
                    .collect::<Vec<_>>();
                PossibleFixes::from_iter(iter::chain(fix_of_report, suggestions))
            } else {
                PossibleFixes::from(fix_of_report)
            };

            result.diagnostics.push((
                Message::new(
                    OxcDiagnostic::error(diagnostic.message)
                        .with_label(span)
                        .with_error_code(rule_plugin_name.to_string(), rule_name.to_string())
                        .with_severity(severity.into()),
                    possible_fixes,
                ),
                Some(format!("{rule_plugin_name}/{rule_name}")),
            ));
            for message in fix_errors {
                push_error(&mut result, message);
            }
        }

        result.markup_references = markup_references
            .into_iter()
            .map(|mut reference| {
                let span = to_utf8_span(reference.start, reference.end);
                reference.start = span.start;
                reference.end = span.end;
                reference
            })
            .collect();
        result.markup_comments = markup_comments
            .into_iter()
            .map(|mut comment| {
                let span = to_utf8_span(comment.start, comment.end);
                comment.start = span.start;
                comment.end = span.end;
                comment.scope_end = comment.scope_end.map(|end| to_utf8_span(end, end).start);
                comment
            })
            .collect();

        Some(result)
    }
}
