//! Linting of files with language plugins.
//!
//! A language plugin is a JS module that parses a file (for example, a Vue single-file component)
//! into its own syntax tree. JS plugin rules run on that syntax tree, in one JS call per file.
//! Rust rules still run on the `<script>` blocks that the partial loader finds.

use std::{collections::BTreeMap, iter, path::Path, sync::Arc};

use cow_utils::CowUtils;
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

/// Converts UTF-16 ranges from JS to UTF-8 spans in the whole file.
struct SpanConverter<'s> {
    /// Source text of the whole file, with the BOM if the file has one.
    source_text: &'s str,
    /// Converter for the source text that JS gets, which has no BOM.
    converter: Utf8ToUtf16,
    /// Largest UTF-16 offset that JS can send.
    max_utf16_offset: u32,
}

impl<'s> SpanConverter<'s> {
    fn new(source_text: &'s str) -> Self {
        let converter = match source_text.strip_prefix(BOM) {
            Some(js_source_text) => Utf8ToUtf16::new_with_offset(js_source_text, BOM_LEN),
            None => Utf8ToUtf16::new(source_text),
        };
        #[expect(clippy::cast_possible_truncation)]
        let mut max_utf16_offset = source_text.len() as u32;
        if let Some(mut converter) = converter.converter() {
            converter.convert_offset(&mut max_utf16_offset);
        }
        Self { source_text, converter, max_utf16_offset }
    }

    /// Convert a UTF-16 range from JS to a UTF-8 span.
    ///
    /// JS can send any numbers, so:
    /// - An offset after the end of the source text moves to the end.
    /// - A reversed range becomes an empty span at its start.
    /// - An offset between the two UTF-16 units of a character (for example, an emoji) moves to the
    ///   start of the character. Other code, such as the language server, panics on an offset
    ///   inside a character.
    fn to_utf8_span(&self, start: u32, end: u32) -> Span {
        let start = start.min(self.max_utf16_offset);
        let end = end.min(self.max_utf16_offset).max(start);
        let mut span = Span::new(start, end);
        self.converter.convert_span_back(&mut span);
        #[expect(clippy::cast_possible_truncation)]
        let to_char_start =
            |offset: u32| self.source_text.floor_char_boundary(offset as usize) as u32;
        Span::new(to_char_start(span.start), to_char_start(span.end))
    }
}

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
            // A plugin can return a `scopeEnd` inside a character (for example between the two
            // UTF-16 units of an emoji). Move it to the start of that character.
            let scope_end = comment.scope_end.unwrap_or(source_len).min(source_len);
            #[expect(clippy::cast_possible_truncation)]
            let scope_end = source_text.floor_char_boundary(scope_end as usize) as u32;
            let Some(comment_text) = source_text.get(comment.start as usize..comment.end as usize)
            else {
                continue;
            };
            // The text of the comment, without its delimiters (such as `<!--` and `-->`).
            // Parsers can give `\n` for each `\r\n` of the file (vue-eslint-parser does).
            let Some((text_start, text_len)) = comment_text
                .find(comment.text.as_str())
                .map(|start| (start, comment.text.len()))
                .or_else(|| {
                    let lf_text = comment.text.cow_replace("\r\n", "\n");
                    let text = lf_text.cow_replace('\n', "\r\n");
                    comment_text.find(&*text).map(|start| (start, text.len()))
                })
            else {
                continue;
            };
            if comment.end > scope_end {
                continue;
            }
            #[expect(clippy::cast_possible_truncation)]
            let text_start = comment.start + text_start as u32;
            #[expect(clippy::cast_possible_truncation)]
            let text_span = Span::new(text_start, text_start + text_len as u32);
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
        let span_converter = SpanConverter::new(source_text);
        let to_utf8_span = |start: u32, end: u32| span_converter.to_utf8_span(start, end);

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
                &span_converter.converter,
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
    use cow_utils::CowUtils;
    use oxc_span::Span;

    use super::{LanguageFileResult, SpanConverter};
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
    fn a_scope_end_inside_a_character_is_moved_to_its_start() {
        // Before, slicing the source text at `scopeEnd` panicked
        let source_text = "<!-- eslint-disable no-debugger -->😀\n<script>debugger;</script>\n";
        let mut markup_comment = comment(source_text, "<!-- eslint-disable no-debugger -->", None);
        #[expect(clippy::cast_possible_truncation)]
        let inside_emoji = (source_text.find('😀').unwrap() + 2) as u32;
        markup_comment.scope_end = Some(inside_emoji);
        let result = LanguageFileResult {
            markup_comments: vec![markup_comment],
            ..LanguageFileResult::default()
        };
        let directives = result.markup_directives(source_text, true).unwrap();
        assert!(!directives.contains("no-debugger", span_of(source_text, "debugger;")));
    }

    #[test]
    #[expect(clippy::cast_possible_truncation)]
    fn a_utf16_offset_inside_a_character_is_moved_to_its_start() {
        // Before, the span had a UTF-8 offset inside the emoji, and the language server panicked
        for source_text in ["<template>💩</template>\n", "\u{feff}<template>💩</template>\n"] {
            let converter = SpanConverter::new(source_text);
            let emoji_start = source_text.find('💩').unwrap() as u32;
            // UTF-16 offsets that JS sends are positions in the text without the BOM
            let utf16_emoji_start = "<template>".len() as u32;
            assert_eq!(
                converter.to_utf8_span(utf16_emoji_start + 1, utf16_emoji_start + 1),
                Span::empty(emoji_start),
            );
            assert_eq!(
                converter.to_utf8_span(utf16_emoji_start, utf16_emoji_start + 1),
                Span::empty(emoji_start),
            );
            // A whole emoji is 2 UTF-16 units and 4 UTF-8 bytes
            assert_eq!(
                converter.to_utf8_span(utf16_emoji_start, utf16_emoji_start + 2),
                Span::sized(emoji_start, 4),
            );
        }
    }

    #[test]
    fn a_comment_text_with_lf_matches_a_comment_with_crlf() {
        // vue-eslint-parser gives the text of an HTML comment with `\n` for each `\r\n`.
        // Before, the text was not found in the file, and the directive was dropped.
        let source_text = "<template>\r\n  <!-- eslint-disable-next-line\r\n    no-debugger -->\r\n  <p></p>\r\n</template>\r\n";
        let mut markup_comment =
            comment(source_text, "<!-- eslint-disable-next-line\r\n    no-debugger -->", None);
        markup_comment.text = markup_comment.text.cow_replace("\r\n", "\n").into_owned();
        let result = LanguageFileResult {
            markup_comments: vec![markup_comment],
            ..LanguageFileResult::default()
        };
        let directives = result.markup_directives(source_text, true).unwrap();
        assert!(directives.contains("no-debugger", span_of(source_text, "<p></p>")));
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
