//! Linting of files with language plugins.
//!
//! A language plugin is a JS module that parses a file (for example, a Vue single-file component)
//! into its own syntax tree. JS plugin rules run on that syntax tree, in one JS call per file.
//! Rust rules still run on the `<script>` blocks that the partial loader finds.

use std::{collections::BTreeMap, iter, path::Path, sync::Arc};

use oxc_ast_visit::utf8_to_utf16::Utf8ToUtf16;
use oxc_diagnostics::OxcDiagnostic;
use oxc_span::Span;

use crate::{
    AllowWarnDeny, ExternalOptionsId, ExternalRuleId, LanguagePluginBinding, Linter,
    config::LintConfig,
    disable_directives::{DisableDirectives, DisableDirectivesBuilder},
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
    /// `None` if the plugin does not return them.
    pub markup_references: Option<Vec<MarkupReference>>,
    /// Comments in the markup.
    pub markup_comments: Vec<MarkupComment>,
}

impl LanguageFileResult {
    /// Build the disable directives of the markup comments, such as `<!-- eslint-disable -->`.
    ///
    /// A block directive (`eslint-disable` and `eslint-enable`) applies until the `scopeEnd` of
    /// its comment, or until the end of the file. So comments are built in groups by `scopeEnd`.
    /// Returns `None` if there are no markup comments.
    pub(crate) fn markup_directives(
        &self,
        source_text: &str,
        respect_eslint_disable_directives: bool,
    ) -> Option<DisableDirectives> {
        if self.markup_comments.is_empty() {
            return None;
        }
        #[expect(clippy::cast_possible_truncation)]
        let source_len = source_text.len() as u32;

        let mut groups = BTreeMap::<u32, Vec<(Span, Span)>>::new();
        for comment in &self.markup_comments {
            let scope_end = comment.scope_end.unwrap_or(source_len).min(source_len);
            let Some(comment_text) = source_text.get(comment.start as usize..comment.end as usize)
            else {
                continue;
            };
            // The text of the comment, without its delimiters (such as `<!--` and `-->`)
            let Some(text_start) = comment_text.find(comment.text.as_str()) else { continue };
            if comment.end > scope_end {
                continue;
            }
            #[expect(clippy::cast_possible_truncation)]
            let text_start = comment.start + text_start as u32;
            #[expect(clippy::cast_possible_truncation)]
            let text_span = Span::new(text_start, text_start + comment.text.len() as u32);
            groups
                .entry(scope_end)
                .or_default()
                .push((Span::new(comment.start, comment.end), text_span));
        }

        let groups = groups.into_iter().map(|(scope_end, mut comments)| {
            comments.sort_unstable_by_key(|(span, _)| span.start);
            let directives = DisableDirectivesBuilder::new()
                .with_respect_eslint_disable_directives(respect_eslint_disable_directives)
                .build_markup(&source_text[..scope_end as usize], comments);
            (directives, 0)
        });
        Some(DisableDirectives::merge_sections(groups))
    }
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
            has_bom,
            rule_ids: external_rules.iter().map(|(rule_id, _, _)| rule_id.raw()).collect(),
            options_ids: external_rules.iter().map(|(_, options_id, _)| options_id.raw()).collect(),
            settings_json,
            globals_json,
            collect_timings: rule_timing_store.is_some(),
            workspace_uri: self.workspace_uri.as_ref().map(ToString::to_string),
        });

        let LintLanguageFileOutput {
            diagnostics,
            timings,
            markup_references,
            markup_comments,
            parse_error,
        } = match output {
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

        if let Some(parse_error) = parse_error {
            let span = to_utf8_span(parse_error.offset, parse_error.offset);
            result.diagnostics.push((
                Message::new(
                    OxcDiagnostic::error(format!("Parsing error: {}", parse_error.message))
                        .with_label(span)
                        .with_help(format!(
                            "Language plugin `{plugin_name}` could not parse this file."
                        )),
                    PossibleFixes::None,
                ),
                None,
            ));
        }

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

        result.markup_references = markup_references.map(|references| {
            references
                .into_iter()
                .map(|mut reference| {
                    let span = to_utf8_span(reference.start, reference.end);
                    reference.start = span.start;
                    reference.end = span.end;
                    reference
                })
                .collect()
        });
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

#[cfg(test)]
mod tests {
    use oxc_span::Span;

    use super::LanguageFileResult;
    use crate::external_linter::MarkupComment;

    #[expect(clippy::cast_possible_truncation)]
    fn comment(source_text: &str, comment: &str, scope_end: Option<&str>) -> MarkupComment {
        let start = source_text.find(comment).unwrap() as u32;
        MarkupComment {
            start,
            end: start + comment.len() as u32,
            text: comment["<!--".len()..comment.len() - "-->".len()].to_string(),
            scope_end: scope_end.map(|end| (source_text.find(end).unwrap() + end.len()) as u32),
        }
    }

    #[expect(clippy::cast_possible_truncation)]
    fn span_of(source_text: &str, needle: &str) -> Span {
        let start = source_text.rfind(needle).unwrap() as u32;
        Span::sized(start, needle.len() as u32)
    }

    #[test]
    fn block_markup_directives_stop_at_their_scope_end() {
        let source_text = "<template>\n<!-- eslint-disable no-console -->\n<a></a>\n</template>\n<!-- eslint-disable no-debugger -->\n<script>\nx;\n</script>\n<script setup>\ny;\n</script>\n<!-- eslint-disable no-alert -->\nz;\n";
        let result = LanguageFileResult {
            markup_comments: vec![
                comment(source_text, "<!-- eslint-disable no-console -->", Some("</template>")),
                comment(source_text, "<!-- eslint-disable no-debugger -->", Some("</script>")),
                comment(source_text, "<!-- eslint-disable no-alert -->", None),
            ],
            ..LanguageFileResult::default()
        };
        let directives = result.markup_directives(source_text, true).unwrap();

        // `no-console` covers the template only
        assert!(directives.contains("no-console", span_of(source_text, "<a></a>")));
        assert!(!directives.contains("no-console", span_of(source_text, "x;")));
        // `no-debugger` covers the first `<script>` only
        assert!(directives.contains("no-debugger", span_of(source_text, "x;")));
        assert!(!directives.contains("no-debugger", span_of(source_text, "y;")));
        // `no-alert` has no `scopeEnd`, so it covers the rest of the file
        assert!(directives.contains("no-alert", span_of(source_text, "z;")));
    }

    #[test]
    fn markup_directives_are_ignored_if_disable_directives_are_not_respected() {
        let source_text = "<!-- eslint-disable-next-line no-console -->\n<a></a>\n";
        let result = LanguageFileResult {
            markup_comments: vec![comment(
                source_text,
                "<!-- eslint-disable-next-line no-console -->",
                None,
            )],
            ..LanguageFileResult::default()
        };
        let directives = result.markup_directives(source_text, false).unwrap();
        assert!(!directives.contains("no-console", span_of(source_text, "<a></a>")));
    }
}
