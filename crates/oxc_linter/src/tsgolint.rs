use std::{
    borrow::Cow,
    collections::BTreeSet,
    ffi::OsStr,
    io::{ErrorKind, Read, Write, stderr},
    iter, mem,
    path::{Path, PathBuf},
    process::Child,
    sync::{Arc, Mutex, mpsc},
    thread::JoinHandle,
    time::Duration,
};

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};

use oxc_diagnostics::{DiagnosticSender, DiagnosticService, NamedSource, OxcDiagnostic, Severity};
use oxc_span::{SourceType, Span};

use super::{AllowWarnDeny, ConfigStore, DisableDirectives, ResolvedLinterState, read_to_string};

use crate::{
    CompositeFix, FixKind, Fixer, Message, PossibleFixes, RuleTimingRecord, RuleTimingSource,
    RuleTimingStore, WEBSITE_BASE_RULES_URL, loader::LINT_PARTIAL_LOADER_EXTENSIONS,
    suppression::DiffManager,
};

/// The environment variable that lets `tsrs-lint` run the content mappers in `tsconfig.json`
/// files. It has the same effect as the `--runExternalCode` flag of `tsrs-lint`. Other backends
/// ignore it.
const RUN_EXTERNAL_CODE_ENV: &str = "TSRS_LINT_RUN_EXTERNAL_CODE";

/// State required to initialize the `tsgolint` linter.
#[derive(Debug, Clone)]
pub struct TsGoLintState {
    /// The path to the `tsgolint` executable (at least our best guess at it).
    executable_path: PathBuf,
    /// Current working directory, used for rendering paths in diagnostics.
    cwd: PathBuf,
    /// The configuration store for `tsgolint` (used to resolve configurations outside of `oxc_linter`)
    config_store: ConfigStore,
    /// If `oxlint` will output the diagnostics or not.
    /// When `silent` is true, we do not need to access the file system for nice diagnostics messages.
    silent: bool,
    /// If `true`, request that fixes be returned from `tsgolint`.
    fix: bool,
    /// If `true`, request that suggestions be returned from `tsgolint`.
    fix_suggestions: bool,
    /// If `true`, include TypeScript compiler syntactic and semantic diagnostics.
    type_check: bool,
    /// If `true`, skip type-aware lint rules while retaining TypeScript diagnostics.
    type_check_only: bool,
    /// If `true`, request that per-rule debug timings be returned from `tsgolint`.
    timings: bool,
    /// If `true`, the linter will create "ignore this section / line" fixes for all diagnostics
    with_ignore_fixes: bool,
    /// If `true`, let the backend run the content mappers in `tsconfig.json` files, and send
    /// `.vue`, `.svelte`, and `.astro` files to the backend.
    run_external_code: bool,
}

impl TsGoLintState {
    pub fn new(cwd: &Path, config_store: ConfigStore, fix_kind: FixKind) -> Self {
        let executable_path =
            try_find_tsgolint_executable(cwd).unwrap_or(PathBuf::from("tsgolint"));

        TsGoLintState {
            config_store,
            executable_path,
            cwd: cwd.to_path_buf(),
            silent: false,
            fix: fix_kind.contains(FixKind::Fix),
            fix_suggestions: fix_kind.contains(FixKind::Suggestion),
            type_check: false,
            type_check_only: false,
            timings: false,
            with_ignore_fixes: false,
            run_external_code: false,
        }
    }

    /// Try to create a new TsGoLintState, returning an error if the executable cannot be found.
    ///
    /// # Errors
    /// Returns an error if the tsgolint executable cannot be found.
    pub fn try_new(
        cwd: &Path,
        config_store: ConfigStore,
        fix_kind: FixKind,
    ) -> Result<Self, String> {
        let executable_path = try_find_tsgolint_executable(cwd)?;

        Ok(TsGoLintState {
            config_store,
            executable_path,
            cwd: cwd.to_path_buf(),
            silent: false,
            fix: fix_kind.contains(FixKind::Fix),
            fix_suggestions: fix_kind.contains(FixKind::Suggestion),
            type_check: false,
            type_check_only: false,
            timings: false,
            with_ignore_fixes: false,
            run_external_code: false,
        })
    }

    /// Set to `true` to skip file system reads.
    /// When `silent` is true, we do not need to access the file system for nice diagnostics messages.
    ///
    /// Default is `false`.
    #[must_use]
    pub fn with_silent(mut self, yes: bool) -> Self {
        self.silent = yes;
        self
    }

    /// Set to `true` to include TypeScript compiler syntactic and semantic diagnostics.
    ///
    /// Default is `false`.
    #[must_use]
    pub fn with_type_check(mut self, yes: bool) -> Self {
        self.type_check = yes;
        self
    }

    /// Set to `true` to skip type-aware lint rules.
    ///
    /// Default is `false`.
    #[must_use]
    pub fn with_type_check_only(mut self, yes: bool) -> Self {
        self.type_check_only = yes;
        self
    }

    /// Set to `true` to request that per-rule debug timings be returned from `tsgolint`.
    ///
    /// Default is `false`.
    #[must_use]
    pub fn with_timings(mut self, yes: bool) -> Self {
        self.timings = yes;
        self
    }

    #[must_use]
    pub fn with_ignore_fixes(mut self, yes: bool) -> Self {
        self.with_ignore_fixes = yes;
        self
    }

    /// Set to `true` to let the backend run the content mappers that `tsconfig.json` files list
    /// in `contentMappers`. Then `.vue`, `.svelte`, and `.astro` files are also sent to the
    /// backend.
    ///
    /// Content mappers are programs from dependencies, so only the user can enable this.
    ///
    /// Default is `false`.
    #[must_use]
    pub fn with_run_external_code(mut self, yes: bool) -> Self {
        self.run_external_code = yes;
        self
    }

    /// Returns an error if content mappers are enabled, but the backend cannot run them.
    ///
    /// `tsgolint` cannot run content mappers, and stops with a panic when it gets a `.vue` file.
    ///
    /// # Errors
    /// Returns a message for the user if the backend is `tsgolint`.
    pub fn check_run_external_code(&self) -> Result<(), String> {
        let is_tsgolint = self
            .executable_path
            .file_stem()
            .and_then(OsStr::to_str)
            .is_some_and(|stem| stem.eq_ignore_ascii_case("tsgolint"));
        if self.run_external_code && is_tsgolint {
            return Err(format!(
                "`--run-external-code` needs a type-aware backend that can run content mappers, but `{}` is `tsgolint`, which cannot.\n\
                 To check `.vue`, `.svelte`, and `.astro` files, add the `tsrs` package to your project, or remove `--run-external-code`.",
                self.executable_path.display()
            ));
        }
        Ok(())
    }

    /// Returns `true` if `path` is sent to the backend.
    fn is_backend_file(&self, path: &Path) -> bool {
        SourceType::from_path(path).is_ok()
            || (self.run_external_code && is_content_mapped_file(path))
    }

    /// Runs `tsgolint` on `paths` to completion. Equivalent to [`Self::start`] followed by
    /// [`TsGoLintRun::finish`].
    ///
    /// # Panics
    /// - when `stdin` of subprocess cannot be opened
    /// - when `stdout` of subprocess cannot be opened
    /// - when `tsgolint` process cannot be awaited
    ///
    /// # Errors
    /// A human-readable error message indicating why the linting failed.
    pub fn lint(
        self,
        paths: &[Arc<OsStr>],
        disable_directives_map: Arc<Mutex<FxHashMap<PathBuf, DisableDirectives>>>,
        error_sender: DiagnosticSender,
        file_system: &(dyn crate::RuntimeFileSystem + Sync + Send),
        diff_manager: &Arc<DiffManager>,
        rule_timing_store: Option<&RuleTimingStore>,
    ) -> Result<Vec<TypeAwareNotice>, String> {
        self.start(paths).finish(
            disable_directives_map,
            error_sender,
            file_system,
            diff_manager,
            rule_timing_store,
        )
    }

    /// Spawns `tsgolint` for `paths` and decodes its output on a background thread, so it can
    /// run while the regular lint pass does.
    ///
    /// Nothing is reported until [`TsGoLintRun::finish`], which must run after the regular pass:
    /// diagnostics are filtered by the disable directives that pass collects. Spawn errors are
    /// reported by `finish` as well.
    ///
    /// # Panics
    /// - when `stdin` of subprocess cannot be opened
    /// - when `stdout` of subprocess cannot be opened
    pub fn start(self, paths: &[Arc<OsStr>]) -> TsGoLintRun {
        if paths.is_empty() {
            return TsGoLintRun { started: None };
        }

        let mut resolved_configs: FxHashMap<PathBuf, ResolvedLinterState> = FxHashMap::default();

        let json_input = self.json_input(paths, None, true, &mut resolved_configs);
        if json_input.configs.is_empty() {
            return TsGoLintRun { started: None };
        }

        let all_paths = {
            let mut paths_buff = vec![];
            for path in paths {
                if self.is_backend_file(Path::new(path)) {
                    let path_buf = PathBuf::from(path);
                    paths_buff.push(path_buf.clone());
                }
            }
            paths_buff
        };

        let process = self.spawn_tsgolint(&json_input).map(|mut child| {
            let stdout = child.stdout.take().expect("Failed to open tsgolint stdout");
            let (sender, messages) = mpsc::channel();
            // Drain stdout as it is written so `tsgolint` never blocks on a full pipe. Message
            // processing stops at the first error, so stop reading there too.
            let reader = std::thread::spawn(move || {
                for msg in TsGoLintMessageStream::new(stdout) {
                    let is_error = !matches!(
                        msg,
                        Ok(TsGoLintMessage::Diagnostic(_)
                            | TsGoLintMessage::Timing(_)
                            | TsGoLintMessage::Notice(_))
                    );
                    if sender.send(msg).is_err() || is_error {
                        break;
                    }
                }
            });
            TsGoLintProcess { child, messages, reader }
        });

        TsGoLintRun {
            started: Some(StartedRun { state: self, resolved_configs, all_paths, process }),
        }
    }
}

/// A `tsgolint` run started by [`TsGoLintState::start`].
pub struct TsGoLintRun {
    /// `None` when there was nothing to lint.
    started: Option<StartedRun>,
}

struct StartedRun {
    state: TsGoLintState,
    resolved_configs: FxHashMap<PathBuf, ResolvedLinterState>,
    all_paths: Vec<PathBuf>,
    /// The spawned process, or the error from spawning it.
    process: Result<TsGoLintProcess, String>,
}

struct TsGoLintProcess {
    child: Child,
    /// Messages decoded from stdout by `reader`, in the order `tsgolint` wrote them.
    messages: mpsc::Receiver<Result<TsGoLintMessage, String>>,
    reader: JoinHandle<()>,
}

impl TsGoLintRun {
    /// Waits for `tsgolint` to exit and reports its diagnostics, filtered by the disable
    /// directives that the regular lint pass collected, then applies its fixes.
    ///
    /// # Panics
    /// - when `tsgolint` process cannot be awaited
    ///
    /// # Errors
    /// A human-readable error message indicating why the linting failed.
    ///
    /// Returns the notices that the backend sent, without duplicates, sorted.
    pub fn finish(
        mut self,
        disable_directives_map: Arc<Mutex<FxHashMap<PathBuf, DisableDirectives>>>,
        error_sender: DiagnosticSender,
        file_system: &(dyn crate::RuntimeFileSystem + Sync + Send),
        diff_manager: &Arc<DiffManager>,
        rule_timing_store: Option<&RuleTimingStore>,
    ) -> Result<Vec<TypeAwareNotice>, String> {
        let Some(StartedRun { state, resolved_configs, all_paths, process }) = self.started.take()
        else {
            return Ok(vec![]);
        };

        let should_fix = state.fix || state.fix_suggestions;
        let cwd = state.cwd.clone();
        let sender_for_fixes = error_sender.clone();

        let diff_manager_clone_to_ts_go = Arc::<DiffManager>::clone(diff_manager);

        let handler = std::thread::spawn(move || {
            let TsGoLintProcess { mut child, messages, reader } = process?;

            // Process messages in a separate thread to send diagnostics as they arrive
            let stdout_handler = std::thread::spawn(move || -> Result<TsGoLintOutput, String> {
                let disable_directives_map =
                    disable_directives_map.lock().expect("disable_directives_map mutex poisoned");

                let mut diagnostic_handler = DiagnosticHandler::new(
                    state.cwd.clone(),
                    state.silent,
                    should_fix,
                    error_sender,
                );
                let mut timings = vec![];
                let mut notices = BTreeSet::new();
                let mut bom_offsets = BomOffsets::default();

                for msg in messages {
                    match msg {
                        Ok(TsGoLintMessage::Error(err)) => {
                            return Err(err.error);
                        }
                        Ok(TsGoLintMessage::Diagnostic(tsgolint_diagnostic)) => {
                            match tsgolint_diagnostic {
                                TsGoLintDiagnostic::Rule(mut tsgolint_diagnostic) => {
                                    drop_fixes_in_content_mapped_file(&mut tsgolint_diagnostic);
                                    bom_offsets.move_rule_diagnostic(&mut tsgolint_diagnostic);
                                    let path = &tsgolint_diagnostic.file_path;

                                    let severity = resolved_configs
                                            .get(path)
                                            .or_else(|| {
                                                debug_assert!(false, "resolved_configs should have an entry for every file we linted {tsgolint_diagnostic:?}");
                                                None
                                            })
                                            .and_then(|resolved_config| {
                                                resolved_config
                                                    .rules
                                                    .iter()
                                                    .find(|(rule, _)| {
                                                        rule.is_tsgolint_rule()
                                                            && rule.name() == tsgolint_diagnostic.rule
                                                    })
                                                    .map(|(_, status)| *status)
                                            })
                                            .or_else(|| {
                                                debug_assert!(false, "resolved_config should have a matching rule for every diagnostic we received {tsgolint_diagnostic:?}");
                                                None
                                            });
                                    let Some(severity) = severity else {
                                        // If the severity is not found, we should not report
                                        // the diagnostic
                                        continue;
                                    };

                                    if should_skip_diagnostic(
                                        &disable_directives_map,
                                        path,
                                        &tsgolint_diagnostic,
                                    ) {
                                        continue;
                                    }

                                    diagnostic_handler.handle_rule_diagnostic(
                                        tsgolint_diagnostic,
                                        severity,
                                        diff_manager_clone_to_ts_go.skip(),
                                    );
                                }
                                TsGoLintDiagnostic::Internal(mut e) => {
                                    bom_offsets.move_internal_diagnostic(&mut e);
                                    diagnostic_handler.handle_internal_diagnostic(e);
                                }
                            }
                        }
                        Ok(TsGoLintMessage::Timing(payload)) => {
                            timings.extend(payload.rules);
                        }
                        Ok(TsGoLintMessage::Notice(notice)) => {
                            if let Some(notice) = TypeAwareNotice::from_payload(notice) {
                                notices.insert(notice);
                            }
                        }
                        Err(e) => {
                            return Err(e);
                        }
                    }
                }

                // The channel also closes when decoding panicked. Fail this thread then, as it
                // did when it decoded stdout itself.
                if let Err(panic) = reader.join() {
                    std::panic::resume_unwind(panic);
                }

                Ok(TsGoLintOutput {
                    messages_requiring_fixes: diagnostic_handler
                        .into_messages_requiring_fixes(&diff_manager_clone_to_ts_go, all_paths),
                    timings,
                    notices: notices.into_iter().collect(),
                })
            });

            // Wait for process to complete and stdout processing to finish
            let exit_status = child.wait().expect("Failed to wait for tsgolint process");
            let stdout_result = stdout_handler.join();

            if !exit_status.success() {
                return Err(
                    if let Some(err) = &stdout_result.ok().and_then(std::result::Result::err) {
                        format!("exit status: {exit_status}, error: {err}")
                    } else {
                        format!("exit status: {exit_status}")
                    },
                );
            }

            match stdout_result {
                Ok(Ok(messages)) => Ok(messages),
                Ok(Err(err)) => Err(format!("exit status: {exit_status}, error: {err}")),
                Err(_) => Err("Failed to join stdout processing thread".to_string()),
            }
        });

        match handler.join() {
            Ok(Ok(output)) => {
                let TsGoLintOutput { messages_requiring_fixes, timings, notices } = output;
                if let Some(rule_timing_store) = rule_timing_store {
                    rule_timing_store.merge(timings.into_iter().map(|timing| RuleTimingRecord {
                        source: RuleTimingSource::TypeAware,
                        plugin_name: "typescript".to_string(),
                        rule_name: timing.rule_name,
                        duration: Duration::from_nanos(timing.duration),
                        calls: timing.calls,
                    }));
                }

                for (path, source_text, messages) in messages_requiring_fixes {
                    let source_type = SourceType::from_path(&path)
                        .ok()
                        .map(|st| if st.is_javascript() { st.with_jsx(true) } else { st });
                    let fix_result = Fixer::new(&source_text, messages, source_type).fix();

                    if fix_result.fixed
                        && let Err(error) = file_system.write_file(&path, &fix_result.fixed_code)
                    {
                        sender_for_fixes
                            .send(vec![
                                OxcDiagnostic::error(format!(
                                    "Failed to write file {} with error \"{error}\"",
                                    path.display()
                                ))
                                .into(),
                            ])
                            .expect("Failed to send diagnostics");
                    }

                    if fix_result.messages.is_empty() {
                        diff_manager.collect_empty_file(path.as_path(), &cwd);
                    } else {
                        let source_for_diagnostics: &str =
                            if fix_result.fixed { &fix_result.fixed_code } else { &source_text };

                        let filtered_messages: Vec<OxcDiagnostic> = if diff_manager.skip() {
                            fix_result.messages.into_iter().map(Into::into).collect()
                        } else {
                            diff_manager
                                .collect_file(
                                    path.as_path(),
                                    &cwd,
                                    fix_result.messages.into_iter().collect(),
                                )
                                .into_iter()
                                .map(Into::into)
                                .collect()
                        };

                        let diagnostics = DiagnosticService::wrap_diagnostics(
                            &cwd,
                            &path,
                            source_for_diagnostics,
                            filtered_messages,
                        );
                        sender_for_fixes.send(diagnostics).expect("Failed to send diagnostics");
                    }
                }
                Ok(notices)
            }
            Ok(Err(err)) => Err(format!("Error running tsgolint: {err:?}")),
            Err(err) => Err(format!("Error running tsgolint: {err:?}")),
        }
    }
}

impl Drop for TsGoLintRun {
    /// Stops `tsgolint` when the run is dropped without [`TsGoLintRun::finish`] (e.g. the regular
    /// pass unwound), so the process does not outlive the run.
    fn drop(&mut self) {
        if let Some(StartedRun { process: Ok(process), .. }) = &mut self.started {
            let _ = process.child.kill();
            let _ = process.child.wait();
        }
    }
}

impl TsGoLintState {
    /// Spawn the tsgolint process with the given input.
    fn spawn_tsgolint(&self, json_input: &Payload) -> Result<std::process::Child, String> {
        let mut cmd = std::process::Command::new(&self.executable_path);
        cmd.arg("headless")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(stderr());

        if self.timings {
            cmd.args(["-debug", "timings"]);
        }

        if self.fix {
            cmd.arg("-fix");
        }

        if self.fix_suggestions {
            cmd.arg("-fix-suggestions");
        }

        // Only `--run-external-code` decides if the backend runs content mappers, so do not pass
        // this variable from the environment of Oxlint.
        if self.run_external_code {
            cmd.env(RUN_EXTERNAL_CODE_ENV, "1");
        } else {
            cmd.env_remove(RUN_EXTERNAL_CODE_ENV);
        }

        if let Ok(trace_file) = std::env::var("OXLINT_TSGOLINT_TRACE") {
            cmd.arg(format!("-trace={trace_file}"));
        }
        if let Ok(cpuprof_file) = std::env::var("OXLINT_TSGOLINT_CPU") {
            cmd.arg(format!("-cpuprof={cpuprof_file}"));
        }
        if let Ok(heap_file) = std::env::var("OXLINT_TSGOLINT_HEAP") {
            cmd.arg(format!("-heap={heap_file}"));
        }
        if let Ok(allocs_file) = std::env::var("OXLINT_TSGOLINT_ALLOCS") {
            cmd.arg(format!("-allocs={allocs_file}"));
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Err(format!(
                    "Failed to spawn tsgolint from path `{}`, with error: {e}",
                    self.executable_path.display()
                ));
            }
        };

        let mut stdin = child.stdin.take().expect("Failed to open tsgolint stdin");

        let json = serde_json::to_string(json_input).expect("Failed to serialize JSON");
        if let Err(e) = stdin.write_all(json.as_bytes())
            && e.kind() != ErrorKind::BrokenPipe
        {
            return Err(format!("Failed to write to tsgolint stdin: {e}"));
        }
        drop(stdin);

        Ok(child)
    }

    /// # Panics
    /// - when `stdin` of subprocess cannot be opened
    /// - when `stdout` of subprocess cannot be opened
    /// - when `tsgolint` process cannot be awaited
    ///
    /// # Errors
    /// A human-readable error message indicating why the linting failed.
    pub fn lint_source(
        &self,
        paths: &[Arc<OsStr>],
        file_system: &(dyn crate::RuntimeFileSystem + Sync + Send),
        disable_directives_map: &Arc<Mutex<FxHashMap<PathBuf, DisableDirectives>>>,
    ) -> Result<Vec<Message>, String> {
        if paths.is_empty() {
            return Ok(vec![]);
        }

        let mut resolved_configs: FxHashMap<PathBuf, ResolvedLinterState> = FxHashMap::default();
        let mut source_overrides: FxHashMap<String, String> = FxHashMap::default();
        let allocator = Allocator::default();

        // Read all sources into overrides
        for path in paths {
            let path_ref = Path::new(path.as_ref());
            let Ok(source_text) = file_system.read_to_arena_str(path_ref, &allocator) else {
                return Err(format!(
                    "Failed to read source text for file: {}",
                    path.to_string_lossy()
                ));
            };
            source_overrides.insert(path.to_string_lossy().to_string(), source_text.to_string());
        }

        let json_input =
            self.json_input(paths, Some(source_overrides.clone()), false, &mut resolved_configs);

        //  Get the file name of the first path for internal diagnostic filtering
        let path_file_name =
            Path::new(paths[0].as_ref()).file_name().unwrap_or_default().to_os_string();

        let mut child = self.spawn_tsgolint(&json_input)?;
        let stdout = child.stdout.take().expect("Failed to open tsgolint stdout");
        let diagnostics = (|| -> Result<Vec<Message>, String> {
            let msg_iter = TsGoLintMessageStream::new(stdout);
            let mut result = vec![];

            for msg in msg_iter {
                match msg {
                    Ok(TsGoLintMessage::Error(err)) => {
                        return Err(err.error);
                    }
                    Ok(TsGoLintMessage::Diagnostic(tsgolint_diagnostic)) => {
                        match tsgolint_diagnostic {
                            TsGoLintDiagnostic::Rule(mut tsgolint_diagnostic) => {
                                drop_fixes_in_content_mapped_file(&mut tsgolint_diagnostic);
                                let path = tsgolint_diagnostic.file_path.clone();
                                let Some(resolved_config) = resolved_configs.get(&path) else {
                                    // If we don't have a resolved config for this path, skip it. We should always
                                    // have a resolved config though, since we processed them already above.
                                    continue;
                                };

                                let severity =
                                    resolved_config.rules.iter().find_map(|(rule, status)| {
                                        if rule.is_tsgolint_rule()
                                            && rule.name() == tsgolint_diagnostic.rule
                                        {
                                            Some(*status)
                                        } else {
                                            None
                                        }
                                    });
                                let Some(severity) = severity else {
                                    // If the severity is not found, we should not report the diagnostic
                                    continue;
                                };

                                if should_skip_diagnostic(
                                    &disable_directives_map
                                        .lock()
                                        .expect("disable_directives_map mutex poisoned"),
                                    &path,
                                    &tsgolint_diagnostic,
                                ) {
                                    continue;
                                }

                                // Use the corresponding source override text
                                let Some(source_text_owned) = source_overrides
                                    .get(&path.to_string_lossy().to_string())
                                    .cloned()
                                else {
                                    // should never happen, because we populated source_overrides above
                                    continue;
                                };

                                let mut message = Message::from_tsgo_lint_diagnostic(
                                    tsgolint_diagnostic,
                                    &source_text_owned,
                                );

                                // In Phase 1, a fix in a content-mapped file can include code
                                // that the mapper generated, so these files get no fixes.
                                if self.with_ignore_fixes && !is_content_mapped_file(&path) {
                                    message.add_ignore_fix(0, &source_text_owned);
                                }

                                message.error.severity = if severity == AllowWarnDeny::Deny {
                                    Severity::Error
                                } else {
                                    Severity::Warning
                                };

                                result.push(message);
                            }
                            TsGoLintDiagnostic::Internal(e) => {
                                let span = e
                                    .file_path
                                    .as_ref()
                                    .is_some_and(|f| {
                                        f.file_name().unwrap_or_default() == path_file_name
                                    })
                                    .then_some(e.span)
                                    .flatten()
                                    .unwrap_or_default();
                                let mut diagnostic: OxcDiagnostic = e.into();
                                diagnostic = diagnostic.with_label(span);
                                result.push(Message::new(diagnostic, PossibleFixes::None));
                            }
                        }
                    }
                    Ok(TsGoLintMessage::Timing(_) | TsGoLintMessage::Notice(_)) => {}
                    Err(e) => {
                        return Err(e);
                    }
                }
            }
            Ok(result)
        })();

        // Kill the child process if it's still running to avoid zombie processes
        let _ = child.kill();
        let _ = child.wait();

        diagnostics
    }

    /// Create a JSON input for STDIN of tsgolint in this format:
    ///
    /// ```json
    /// {
    ///   "files": [
    ///     {
    ///       "file_path": "/absolute/path/to/file.ts",
    ///       "rules": ["rule-1", "another-rule"]
    ///     }
    ///   ]
    /// }
    /// ```
    ///
    /// If `report_notices` is `true`, the backend can send notices, such as a notice that a
    /// `tsconfig.json` file lists content mappers, but they did not run.
    #[inline]
    fn json_input(
        &self,
        paths: &[Arc<OsStr>],
        source_overrides: Option<FxHashMap<String, String>>,
        report_notices: bool,
        resolved_configs: &mut FxHashMap<PathBuf, ResolvedLinterState>,
    ) -> Payload {
        if self.type_check_only {
            let file_paths: Vec<String> = paths
                .iter()
                .filter(|path| self.is_backend_file(Path::new(path)))
                .map(|path| path.to_string_lossy().into_owned())
                .collect();
            return Payload {
                version: 2,
                configs: if file_paths.is_empty() {
                    vec![]
                } else {
                    vec![Config { file_paths, rules: vec![] }]
                },
                source_overrides,
                report_syntactic: self.type_check,
                report_semantic: self.type_check,
                report_notices,
            };
        }

        let mut config_groups: FxHashMap<BTreeSet<Rule>, Vec<String>> = FxHashMap::default();

        for path in paths {
            if self.is_backend_file(Path::new(path)) {
                let file_path = path.to_string_lossy().to_string();
                let path_buf = PathBuf::from(path);
                let resolved_config = resolved_configs
                    .entry(path_buf.clone())
                    .or_insert_with(|| self.config_store.resolve(&path_buf));

                let disabled_rules = disabled_rules_in_content_mapped_file(&path_buf);
                let rules: BTreeSet<Rule> = resolved_config
                    .rules
                    .iter()
                    .filter_map(|(rule, status)| {
                        if status.is_warn_deny()
                            && rule.is_tsgolint_rule()
                            && !disabled_rules.contains(&rule.name())
                        {
                            let rule_name = rule.name().to_string();
                            let options = match rule.to_configuration() {
                                Some(Ok(config)) => Some(config),
                                Some(Err(_)) | None => None,
                            };
                            Some(Rule { name: rule_name, options })
                        } else {
                            None
                        }
                    })
                    .collect();

                config_groups.entry(rules).or_default().push(file_path);
            }
        }

        Payload {
            version: 2,
            configs: config_groups
                .into_iter()
                .map(|(rules, file_paths)| Config {
                    file_paths,
                    rules: rules.into_iter().collect(),
                })
                .collect(),
            source_overrides,
            report_syntactic: self.type_check,
            report_semantic: self.type_check,
            report_notices,
        }
    }
}

/// The backend reads files from disk without their byte order mark (BOM), so its offsets are
/// positions in the text without the BOM. Oxlint uses the text with the BOM, so the offsets of
/// these files must move by the length of the BOM.
///
/// When Oxlint sends the source text in `source_overrides`, the text keeps its BOM, and the
/// offsets are already right. So use this only for files that the backend reads from disk.
#[derive(Default)]
struct BomOffsets(FxHashMap<PathBuf, u32>);

impl BomOffsets {
    /// The UTF-8 encoding of U+FEFF.
    const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

    fn get(&mut self, path: &Path) -> u32 {
        *self.0.entry(path.to_path_buf()).or_insert_with(|| {
            let mut start = [0u8; 3];
            let starts_with_bom = std::fs::File::open(path)
                .and_then(|mut file| file.read_exact(&mut start))
                .is_ok_and(|()| start == Self::BOM);
            if starts_with_bom { 3 } else { 0 }
        })
    }

    fn move_rule_diagnostic(&mut self, diagnostic: &mut TsGoLintRuleDiagnostic) {
        let offset = self.get(&diagnostic.file_path);
        if offset == 0 {
            return;
        }
        diagnostic.span = diagnostic.span.move_right(offset);
        for labeled_range in &mut diagnostic.labeled_ranges {
            labeled_range.range.move_right(offset);
        }
        for fix in diagnostic
            .fixes
            .iter_mut()
            .chain(diagnostic.suggestions.iter_mut().flat_map(|suggestion| &mut suggestion.fixes))
        {
            fix.range.move_right(offset);
        }
    }

    fn move_internal_diagnostic(&mut self, diagnostic: &mut TsGoLintInternalDiagnostic) {
        if let Some(file_path) = &diagnostic.file_path
            && let Some(span) = &mut diagnostic.span
        {
            let offset = self.get(file_path);
            if offset != 0 {
                *span = span.move_right(offset);
            }
        }
    }
}

/// Returns `true` if `path` is a file that a content mapper can turn into TypeScript: a `.vue`,
/// `.svelte`, or `.astro` file. The check uses only the extension.
pub fn is_content_mapped_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|ext| LINT_PARTIAL_LOADER_EXTENSIONS.contains(&ext))
}

/// Type-aware rules that give too many false or missed reports on the TypeScript that content
/// mappers generate for each framework. Oxlint does not run them on these files.
fn disabled_rules_in_content_mapped_file(path: &Path) -> &'static [&'static str] {
    match path.extension().and_then(OsStr::to_str) {
        Some("vue") => DISABLED_RULES_IN_VUE_FILES,
        Some("svelte") => DISABLED_RULES_IN_SVELTE_FILES,
        Some("astro") => DISABLED_RULES_IN_ASTRO_FILES,
        _ => &[],
    }
}

const DISABLED_RULES_IN_VUE_FILES: &[&str] = &[];
const DISABLED_RULES_IN_SVELTE_FILES: &[&str] = &[];
const DISABLED_RULES_IN_ASTRO_FILES: &[&str] = &[];

/// In Phase 1, Oxlint does not apply fixes or suggestions from type-aware rules in
/// content-mapped files, because a fix can include code that the mapper generated.
fn drop_fixes_in_content_mapped_file(diagnostic: &mut TsGoLintRuleDiagnostic) {
    if is_content_mapped_file(&diagnostic.file_path) {
        diagnostic.fixes.clear();
        diagnostic.suggestions.clear();
    }
}

/// Represents the input JSON to `tsgolint`, like:
///
/// ```json
/// {
///   "version": 2,
///   "configs": [
///     {
///       "file_paths": ["/absolute/path/to/file.ts", "/another/file.ts"],
///       "rules": [
///         { "name": "rule-1" },
///         { "name": "another-rule" },
///       ]
///     }
///   ]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Payload {
    pub version: i32,
    pub configs: Vec<Config>,
    pub source_overrides: Option<FxHashMap<String, String>>,
    pub report_syntactic: bool,
    pub report_semantic: bool,
    /// Lets the backend send notices (message type 3). Backends that do not know this field
    /// ignore it. Not serialized when `false`, so the payload stays the same as before.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub report_notices: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Absolute path to the file to lint
    pub file_paths: Vec<String>,
    /// List of rules to apply to this file
    /// Example: `["no-floating-promises"]`
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub struct Rule {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<serde_json::Value>,
}

impl PartialOrd for Rule {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Rule {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // First compare by name
        match self.name.cmp(&other.name) {
            std::cmp::Ordering::Equal => {
                // If names are equal, compare by serialized options
                // Serialize to canonical JSON string for comparison
                let self_options = self.options.as_ref().map(|v| serde_json::to_string(v).ok());
                let other_options = other.options.as_ref().map(|v| serde_json::to_string(v).ok());
                self_options.cmp(&other_options)
            }
            other_ordering => other_ordering,
        }
    }
}

/// Diagnostic kind discriminator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DiagnosticKind {
    Rule = 0,
    Internal = 1,
}

impl Serialize for DiagnosticKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for DiagnosticKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        match value {
            0 => Ok(DiagnosticKind::Rule),
            1 => Ok(DiagnosticKind::Internal),
            _ => Err(serde::de::Error::custom(format!("Invalid DiagnosticKind value: {value}"))),
        }
    }
}

/// Represents the raw output binary data from `tsgolint`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TsGoLintDiagnosticPayload {
    pub kind: DiagnosticKind,
    pub range: Option<Range>,
    pub message: RuleMessage,
    pub file_path: Option<String>,
    // Only for kind="rule"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fixes: Vec<Fix>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<Suggestion>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labeled_ranges: Vec<LabeledRange>,
}

/// Represents the error payload from `tsgolint`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TsGoLintErrorPayload {
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsGoLintTimingPayload {
    pub rules: Vec<TsGoLintRuleTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsGoLintRuleTiming {
    pub rule_name: String,
    pub duration: u64,
    pub calls: u64,
}

/// Represents a notice from the backend: information for the user that is not a diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TsGoLintNoticePayload {
    pub id: String,
    #[serde(default)]
    pub tsconfig: Option<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
}

/// A notice from the type-aware backend. Oxlint shows it separately from diagnostics. It does
/// not count as a warning or an error, and does not change the exit code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypeAwareNotice {
    /// A `tsconfig.json` file lists content mappers, but Oxlint ran without
    /// `--run-external-code`, so type-aware rules did not check the files of these mappers.
    ContentMappersRequireRunExternalCode {
        /// The absolute path of the `tsconfig.json` file.
        tsconfig: PathBuf,
        /// The file extensions of the mappers, such as `.vue`.
        extensions: Vec<String>,
    },
}

impl TypeAwareNotice {
    const CONTENT_MAPPERS_REQUIRE_RUN_EXTERNAL_CODE: &str =
        "content-mappers-require-run-external-code";

    /// Returns `None` for notices that this version of Oxlint does not know.
    fn from_payload(payload: TsGoLintNoticePayload) -> Option<Self> {
        match payload.id.as_str() {
            Self::CONTENT_MAPPERS_REQUIRE_RUN_EXTERNAL_CODE => {
                Some(Self::ContentMappersRequireRunExternalCode {
                    tsconfig: PathBuf::from(payload.tsconfig?),
                    extensions: payload.extensions,
                })
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum TsGoLintMessage {
    Diagnostic(TsGoLintDiagnostic),
    Error(TsGoLintError),
    Timing(TsGoLintTimingPayload),
    Notice(TsGoLintNoticePayload),
}

#[derive(Debug, Clone)]
pub enum TsGoLintDiagnostic {
    Rule(TsGoLintRuleDiagnostic),
    Internal(TsGoLintInternalDiagnostic),
}

#[derive(Debug, Clone)]
pub struct TsGoLintRuleDiagnostic {
    pub span: Span,
    pub rule: String,
    pub message: RuleMessage,
    pub fixes: Vec<Fix>,
    pub suggestions: Vec<Suggestion>,
    pub labeled_ranges: Vec<LabeledRange>,
    pub file_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct TsGoLintInternalDiagnostic {
    pub message: RuleMessage,
    pub span: Option<Span>,
    pub file_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TsGoLintError {
    pub error: String,
}

impl From<TsGoLintDiagnostic> for OxcDiagnostic {
    fn from(val: TsGoLintDiagnostic) -> Self {
        match val {
            TsGoLintDiagnostic::Rule(d) => d.into(),
            TsGoLintDiagnostic::Internal(d) => d.into(),
        }
    }
}

impl From<TsGoLintRuleDiagnostic> for OxcDiagnostic {
    fn from(val: TsGoLintRuleDiagnostic) -> Self {
        let mut d = OxcDiagnostic::warn(val.message.description)
            .with_url(format!("{}/{}/{}.html", WEBSITE_BASE_RULES_URL, "typescript", val.rule))
            .with_error_code("typescript", val.rule);
        if let Some(help) = val.message.help {
            d = d.with_help(help);
        }
        if val.labeled_ranges.is_empty() {
            d = d.with_label(val.span);
        } else {
            let labels = val
                .labeled_ranges
                .into_iter()
                .map(|lr| Span::new(lr.range.pos, lr.range.end).label(lr.label));
            d = d.with_labels(labels);
            // If the main span is empty, don't add it as a label since it doesn't have any meaning (means tsgolint sent nothing).
            // Just use the labeled ranges that were passed instead.
            if !val.span.is_unspanned() {
                d = d.and_label(val.span.primary());
            }
        }
        d
    }
}
impl From<TsGoLintInternalDiagnostic> for OxcDiagnostic {
    fn from(val: TsGoLintInternalDiagnostic) -> Self {
        let mut d = OxcDiagnostic::error(val.message.description)
            .with_error_code("typescript", val.message.id);
        if let Some(help) = val.message.help {
            d = d.with_help(help);
        }
        if val.file_path.is_some()
            && let Some(span) = val.span
        {
            d = d.with_label(span);
        }
        d
    }
}

impl Message {
    /// Converts a `TsGoLintDiagnostic` into a `Message` with possible fixes.
    fn from_tsgo_lint_diagnostic(mut val: TsGoLintRuleDiagnostic, source_text: &str) -> Self {
        let fix = if val.fixes.is_empty() {
            None
        } else {
            let fix_vec = mem::take(&mut val.fixes)
                .into_iter()
                .map(|fix| crate::fixer::Fix {
                    content: Cow::Owned(fix.text),
                    span: Span::new(fix.range.pos, fix.range.end),
                    message: None,
                    kind: FixKind::Fix,
                })
                .collect();

            Some(CompositeFix::merge_fixes(fix_vec, source_text))
        };

        let suggestions = mem::take(&mut val.suggestions).into_iter().map(|suggestion| {
            let fix_vec = suggestion
                .fixes
                .into_iter()
                .map(|fix| crate::fixer::Fix {
                    content: Cow::Owned(fix.text),
                    span: Span::new(fix.range.pos, fix.range.end),
                    message: None,
                    kind: FixKind::Suggestion,
                })
                .collect();

            CompositeFix::merge_fixes(fix_vec, source_text)
                .with_message(suggestion.message.description)
        });

        let possible_fixes = PossibleFixes::from_iter(iter::chain(fix, suggestions));

        Self::new(val.into(), possible_fixes)
    }
}

// TODO: Should this be removed and replaced with a `Span`?
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Range {
    pub pos: u32,
    pub end: u32,
}

impl Range {
    fn move_right(&mut self, offset: u32) {
        self.pos += offset;
        self.end += offset;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuleMessage {
    pub id: String,
    pub description: String,
    pub help: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fix {
    pub text: String,
    pub range: Range,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suggestion {
    pub message: RuleMessage,
    pub fixes: Vec<Fix>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LabeledRange {
    pub label: String,
    pub range: Range,
}

#[derive(Eq, PartialEq, Clone, Debug, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageType {
    Error = 0,
    Diagnostic = 1,
    Timing = 2,
    Notice = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidMessageType(pub u8);

impl std::fmt::Display for InvalidMessageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid message type: {}", self.0)
    }
}

impl std::error::Error for InvalidMessageType {}

impl TryFrom<u8> for MessageType {
    type Error = InvalidMessageType;

    fn try_from(value: u8) -> Result<Self, InvalidMessageType> {
        match value {
            0 => Ok(Self::Error),
            1 => Ok(Self::Diagnostic),
            2 => Ok(Self::Timing),
            3 => Ok(Self::Notice),
            _ => Err(InvalidMessageType(value)),
        }
    }
}

/// Iterator that streams messages from tsgolint stdout.
struct TsGoLintMessageStream {
    stdout: std::process::ChildStdout,
    buffer: Vec<u8>,
}

impl TsGoLintMessageStream {
    fn new(stdout: std::process::ChildStdout) -> TsGoLintMessageStream {
        TsGoLintMessageStream { stdout, buffer: Vec::with_capacity(8192) }
    }
}

impl Iterator for TsGoLintMessageStream {
    type Item = Result<TsGoLintMessage, String>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut read_buf = [0u8; 8192];

        loop {
            // Try to parse a complete message from the existing buffer
            let mut cursor = std::io::Cursor::new(self.buffer.as_slice());

            if cursor.position() < self.buffer.len() as u64 {
                match parse_single_message(&mut cursor) {
                    Ok(message) => {
                        // Successfully parsed a message, remove it from buffer
                        #[expect(clippy::cast_possible_truncation)]
                        self.buffer.drain(..cursor.position() as usize);
                        return Some(Ok(message));
                    }
                    Err(TsGoLintMessageParseError::IncompleteData) => {}
                    Err(e) => {
                        return Some(Err(e.to_string()));
                    }
                }
            }

            // Read more data from stdout
            match self.stdout.read(&mut read_buf) {
                Ok(0) => {
                    return None;
                }
                Ok(n) => {
                    self.buffer.extend_from_slice(&read_buf[..n]);
                }
                Err(e) => {
                    return Some(Err(format!("Failed to read from tsgolint stdout: {e}")));
                }
            }
        }
    }
}

enum TsGoLintMessageParseError {
    IncompleteData,
    InvalidMessageType(InvalidMessageType),
    InvalidErrorPayload(serde_json::Error),
    InvalidDiagnosticPayload(serde_json::Error),
    InvalidTimingPayload(serde_json::Error),
    InvalidNoticePayload(serde_json::Error),
}

impl std::fmt::Display for TsGoLintMessageParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TsGoLintMessageParseError::IncompleteData => write!(f, "Incomplete data"),
            TsGoLintMessageParseError::InvalidMessageType(e) => write!(f, "{e}"),
            TsGoLintMessageParseError::InvalidErrorPayload(e) => {
                write!(f, "Failed to parse tsgolint error payload: {e}")
            }
            TsGoLintMessageParseError::InvalidDiagnosticPayload(e) => {
                write!(f, "Failed to parse tsgolint diagnostic payload: {e}")
            }
            TsGoLintMessageParseError::InvalidTimingPayload(e) => {
                write!(f, "Failed to parse tsgolint timing payload: {e}")
            }
            TsGoLintMessageParseError::InvalidNoticePayload(e) => {
                write!(f, "Failed to parse tsgolint notice payload: {e}")
            }
        }
    }
}

/// Cache for source text to avoid reading the same file multiple times.
#[derive(Default)]
struct SourceTextCache(FxHashMap<PathBuf, String>);

impl SourceTextCache {
    fn get_or_insert(&mut self, path: &Path) -> &str {
        self.0
            .entry(path.to_path_buf())
            .or_insert_with(|| read_to_string(path).unwrap_or_default())
            .as_str()
    }
}

struct TsGoLintOutput {
    messages_requiring_fixes: Vec<(PathBuf, String, Vec<Message>)>,
    timings: Vec<TsGoLintRuleTiming>,
    notices: Vec<TypeAwareNotice>,
}

/// Handles streaming and collecting diagnostics from tsgolint.
struct DiagnosticHandler {
    cwd: PathBuf,
    silent: bool,
    should_fix: bool,
    source_text_cache: SourceTextCache,
    /// One source per file, shared by every diagnostic of that file that is sent as it arrives.
    named_sources: FxHashMap<PathBuf, Arc<NamedSource<String>>>,
    error_sender: DiagnosticSender,
    /// Messages requiring fixes, grouped by file path: messages.
    messages_requiring_fixes: FxHashMap<PathBuf, Vec<Message>>,
    messages_not_requiring_fixes: FxHashMap<PathBuf, Vec<(TsGoLintRuleDiagnostic, AllowWarnDeny)>>,
}

impl DiagnosticHandler {
    fn new(cwd: PathBuf, silent: bool, should_fix: bool, error_sender: DiagnosticSender) -> Self {
        Self {
            cwd,
            silent,
            should_fix,
            source_text_cache: SourceTextCache::default(),
            named_sources: FxHashMap::default(),
            error_sender,
            messages_requiring_fixes: FxHashMap::default(),
            messages_not_requiring_fixes: FxHashMap::default(),
        }
    }

    fn get_source_text(&mut self, path: &Path) -> &str {
        if self.silent && !self.should_fix {
            // The source text is not needed in silent mode, the diagnostic isn't printed.
            ""
        } else {
            self.source_text_cache.get_or_insert(path)
        }
    }

    fn handle_rule_diagnostic(
        &mut self,
        diagnostic: TsGoLintRuleDiagnostic,
        severity: AllowWarnDeny,
        ignore_suppression: bool,
    ) {
        let path = diagnostic.file_path.clone();
        let has_fixes =
            self.should_fix && (!diagnostic.fixes.is_empty() || !diagnostic.suggestions.is_empty());

        if has_fixes {
            // Collect for later fix application
            let mut message =
                Message::from_tsgo_lint_diagnostic(diagnostic, self.get_source_text(&path));
            message.error.severity =
                if severity == AllowWarnDeny::Deny { Severity::Error } else { Severity::Warning };

            let entry = self.messages_requiring_fixes.entry(path).or_default();

            entry.push(message);
        } else if !ignore_suppression {
            let entry = self.messages_not_requiring_fixes.entry(path).or_default();

            entry.push((diagnostic, severity));
        } else {
            self.send_diagnostic(&path, diagnostic.into(), severity);
        }
    }

    fn handle_internal_diagnostic(&mut self, e: TsGoLintInternalDiagnostic) {
        let file_path = e.file_path.clone();
        let oxc_diagnostic: OxcDiagnostic = e.into();

        let diagnostics = if let Some(ref file_path) = file_path {
            let source_text = self.get_source_text(file_path).to_string();
            DiagnosticService::wrap_diagnostics(
                &self.cwd,
                file_path,
                &source_text,
                vec![oxc_diagnostic],
            )
        } else {
            vec![oxc_diagnostic.into()]
        };

        self.error_sender.send(diagnostics).expect("Failed to send diagnostics");
    }

    fn send_diagnostic(
        &mut self,
        path: &Path,
        oxc_diagnostic: OxcDiagnostic,
        severity: AllowWarnDeny,
    ) {
        let oxc_diagnostic = oxc_diagnostic.with_severity(if severity == AllowWarnDeny::Deny {
            Severity::Error
        } else {
            Severity::Warning
        });
        let source = self.named_source(path);
        self.error_sender
            .send(vec![oxc_diagnostic.with_source_code(source)])
            .expect("Failed to send diagnostics");
    }

    fn named_source(&mut self, path: &Path) -> Arc<NamedSource<String>> {
        if let Some(source) = self.named_sources.get(path) {
            return Arc::clone(source);
        }
        let cwd = self.cwd.clone();
        let source = DiagnosticService::named_source(cwd, path, self.get_source_text(path));
        self.named_sources.insert(path.to_path_buf(), Arc::clone(&source));
        source
    }

    /// Consume the handler and return collected messages requiring fixes.
    fn into_messages_requiring_fixes(
        self,
        diff_manager: &Arc<DiffManager>,
        paths: Vec<PathBuf>,
    ) -> Vec<(PathBuf, String, Vec<Message>)> {
        let Self { messages_requiring_fixes, mut source_text_cache, should_fix, silent, .. } = self;

        if !diff_manager.skip() {
            for path in paths {
                if let Some(messages) = self.messages_not_requiring_fixes.get(&path) {
                    let source_text = source_text_cache.0.remove(&path).unwrap_or_else(|| {
                        if !silent || should_fix {
                            read_to_string(&path).unwrap_or_default()
                        } else {
                            String::new()
                        }
                    });

                    let filtered_messages: Vec<OxcDiagnostic> = diff_manager
                        .collect_file(
                            path.as_path(),
                            self.cwd.as_path(),
                            messages
                                .iter()
                                .cloned()
                                .map(|(diagnostic, severity)| {
                                    let mut message = Message::from_tsgo_lint_diagnostic(
                                        diagnostic,
                                        source_text.as_str(),
                                    );
                                    message.error.severity = if severity == AllowWarnDeny::Deny {
                                        Severity::Error
                                    } else {
                                        Severity::Warning
                                    };
                                    message
                                })
                                .collect(),
                        )
                        .into_iter()
                        .map(Into::into)
                        .collect();

                    let diagnostics = DiagnosticService::wrap_diagnostics(
                        &self.cwd,
                        &path,
                        source_text.as_str(),
                        filtered_messages,
                    );
                    self.error_sender.send(diagnostics).expect("Failed to send diagnostics");
                } else if !messages_requiring_fixes.contains_key(&path) {
                    diff_manager.collect_empty_file(path.as_path(), self.cwd.as_path());
                }
            }
        }

        messages_requiring_fixes
            .into_iter()
            .map(|(path, messages)| {
                let source_text = source_text_cache.0.remove(&path).unwrap_or_else(|| {
                    if !silent || should_fix {
                        read_to_string(&path).unwrap_or_default()
                    } else {
                        String::new()
                    }
                });
                (path, source_text, messages)
            })
            .collect()
    }
}

fn should_skip_diagnostic(
    disable_directives_map: &FxHashMap<PathBuf, DisableDirectives>,
    path: &Path,
    tsgolint_diagnostic: &TsGoLintRuleDiagnostic,
) -> bool {
    let span = if tsgolint_diagnostic.span.is_unspanned() {
        tsgolint_diagnostic
            .labeled_ranges
            .first()
            .map_or(tsgolint_diagnostic.span, |range| Span::new(range.range.pos, range.range.end))
    } else {
        tsgolint_diagnostic.span
    };

    if let Some(directives) = disable_directives_map.get(path) {
        directives.contains(&tsgolint_diagnostic.rule, span)
            || directives.contains(&format!("typescript-eslint/{}", tsgolint_diagnostic.rule), span)
            || directives
                .contains(&format!("@typescript-eslint/{}", tsgolint_diagnostic.rule), span)
    } else {
        // A content-mapped file has no directives when it has no `<script>` block.
        debug_assert!(
            is_content_mapped_file(path),
            "missing disable_directives_map entry for {}; expected directives to be collected for every linted file",
            path.display()
        );
        false
    }
}

/// Parses a single message from the binary tsgolint output.
// Messages are encoded as follows:
// | Payload Size (uint32 LE) - 4 bytes | Message Type (uint8) - 1 byte | Payload |
fn parse_single_message(
    cursor: &mut std::io::Cursor<&[u8]>,
) -> Result<TsGoLintMessage, TsGoLintMessageParseError> {
    let mut size_bytes = [0u8; 4];
    if cursor.read_exact(&mut size_bytes).is_err() {
        return Err(TsGoLintMessageParseError::IncompleteData);
    }
    let size = u32::from_le_bytes(size_bytes) as usize;

    let mut message_type_byte = [0u8; 1];
    if cursor.read_exact(&mut message_type_byte).is_err() {
        return Err(TsGoLintMessageParseError::IncompleteData);
    }

    let message_type = MessageType::try_from(message_type_byte[0])
        .map_err(TsGoLintMessageParseError::InvalidMessageType)?;

    let mut payload_bytes = vec![0u8; size];
    if cursor.read_exact(&mut payload_bytes).is_err() {
        return Err(TsGoLintMessageParseError::IncompleteData);
    }
    let payload_str = String::from_utf8_lossy(&payload_bytes);

    match message_type {
        MessageType::Error => {
            let error_payload = serde_json::from_str::<TsGoLintErrorPayload>(&payload_str)
                .map_err(TsGoLintMessageParseError::InvalidErrorPayload)?;

            Ok(TsGoLintMessage::Error(TsGoLintError { error: error_payload.error }))
        }
        MessageType::Diagnostic => {
            let diagnostic_payload =
                serde_json::from_str::<TsGoLintDiagnosticPayload>(&payload_str)
                    .map_err(TsGoLintMessageParseError::InvalidDiagnosticPayload)?;

            Ok(TsGoLintMessage::Diagnostic(match diagnostic_payload.kind {
                DiagnosticKind::Rule => TsGoLintDiagnostic::Rule(TsGoLintRuleDiagnostic {
                    rule: diagnostic_payload
                        .rule
                        .expect("Rule name must be present for rule diagnostics"),
                    span: diagnostic_payload.range.map_or_else(
                        || {
                            debug_assert!(false, "Range must be present for rule diagnostics");
                            Span::default()
                        },
                        |range| Span::new(range.pos, range.end),
                    ),
                    message: diagnostic_payload.message,
                    fixes: diagnostic_payload.fixes,
                    suggestions: diagnostic_payload.suggestions,
                    labeled_ranges: diagnostic_payload.labeled_ranges,
                    file_path: PathBuf::from(
                        diagnostic_payload
                            .file_path
                            .expect("File path must be present for rule diagnostics"),
                    ),
                }),
                DiagnosticKind::Internal => {
                    TsGoLintDiagnostic::Internal(TsGoLintInternalDiagnostic {
                        message: diagnostic_payload.message,
                        span: diagnostic_payload.range.map(|range| Span::new(range.pos, range.end)),
                        file_path: diagnostic_payload.file_path.map(PathBuf::from),
                    })
                }
            }))
        }
        MessageType::Timing => {
            let timing_payload = serde_json::from_str::<TsGoLintTimingPayload>(&payload_str)
                .map_err(TsGoLintMessageParseError::InvalidTimingPayload)?;

            Ok(TsGoLintMessage::Timing(timing_payload))
        }
        MessageType::Notice => {
            let notice_payload = serde_json::from_str::<TsGoLintNoticePayload>(&payload_str)
                .map_err(TsGoLintMessageParseError::InvalidNoticePayload)?;

            Ok(TsGoLintMessage::Notice(notice_payload))
        }
    }
}

/// Tries to find a type-aware linter executable. In priority order, this will check:
/// 1. The `OXLINT_TSGOLINT_PATH` environment variable.
/// 2. `node_modules/.bin` in the current working directory and its ancestors.
/// 3. The system `PATH`.
///
/// Within each directory, `tsrs-lint` is preferred over `tsgolint`, unless Go profiling
/// is requested with `OXLINT_TSGOLINT_TRACE`, `OXLINT_TSGOLINT_CPU`,
/// `OXLINT_TSGOLINT_HEAP`, or `OXLINT_TSGOLINT_ALLOCS`. In that case, only
/// `tsgolint` is considered. An explicit executable path overrides this selection.
///
/// # Errors
/// Returns an error if `OXLINT_TSGOLINT_PATH` is set but does not exist or is not a file.
/// Returns an error if no supported executable could be found.
pub fn try_find_tsgolint_executable(cwd: &Path) -> Result<PathBuf, String> {
    let go_profiling = [
        "OXLINT_TSGOLINT_TRACE",
        "OXLINT_TSGOLINT_CPU",
        "OXLINT_TSGOLINT_HEAP",
        "OXLINT_TSGOLINT_ALLOCS",
    ]
    .iter()
    .any(|name| std::env::var(name).is_ok());

    // Package managers on Windows provide CMD shims or, in Bun's case, EXE shims.
    #[cfg(windows)]
    let files: &[&str] = if go_profiling {
        &["tsgolint.CMD", "tsgolint.exe"]
    } else {
        &["tsrs-lint.CMD", "tsrs-lint.exe", "tsgolint.CMD", "tsgolint.exe"]
    };
    #[cfg(not(windows))]
    let files: &[&str] = if go_profiling { &["tsgolint"] } else { &["tsrs-lint", "tsgolint"] };

    if let Ok(path_str) = std::env::var("OXLINT_TSGOLINT_PATH") {
        let path = PathBuf::from(&path_str);
        if path.is_dir() {
            for file in files {
                let executable_path = path.join(file);
                if executable_path.is_file() {
                    return Ok(executable_path);
                }
            }
            let expected = if go_profiling { "'tsgolint'" } else { "'tsrs-lint' or 'tsgolint'" };
            return Err(format!(
                "Failed to find type-aware linter executable: OXLINT_TSGOLINT_PATH points to directory '{path_str}' but no {expected} binary found inside"
            ));
        }
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "Failed to find tsgolint executable: OXLINT_TSGOLINT_PATH points to '{path_str}' which does not exist"
        ));
    }

    let mut current_dir = cwd.to_path_buf();
    loop {
        for file in files {
            let node_modules_bin = current_dir.join("node_modules").join(".bin").join(file);
            if node_modules_bin.is_file() {
                return Ok(node_modules_bin);
            }
        }

        if !current_dir.pop() {
            break;
        }
    }

    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            for file in files {
                let candidate = dir.join(file);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
    }

    if go_profiling {
        Err("Failed to find tsgolint executable. Go profiling requires the `oxlint-tsgolint` package.".to_string())
    } else {
        Err("Failed to find type-aware linter executable. You may need to add the `tsrs` or `oxlint-tsgolint` package to your project?".to_string())
    }
}

#[cfg(test)]
mod test {
    use oxc_diagnostics::{LabeledSpan, OxcCode, Severity};
    use oxc_span::Span;

    use crate::{
        fixer::{FixKind, Message, PossibleFixes},
        tsgolint::{Fix, LabeledRange, Range, RuleMessage, Suggestion, TsGoLintRuleDiagnostic},
    };

    #[test]
    fn test_message_from_tsgo_lint_diagnostic_basic() {
        let diagnostic = TsGoLintRuleDiagnostic {
            span: Span::new(0, 10),
            rule: "some_rule".into(),
            message: RuleMessage {
                id: "some_id".into(),
                description: "Some description".into(),
                help: Some("Some help".into()),
            },
            fixes: vec![],
            suggestions: vec![],
            labeled_ranges: vec![],
            file_path: "some/file/path".into(),
        };

        let message = Message::from_tsgo_lint_diagnostic(diagnostic, "Some text over 10 bytes.");

        assert_eq!(message.error.message, "Some description");
        assert_eq!(message.error.severity, Severity::Warning);
        assert_eq!(message.span, Span::new(0, 10));
        assert_eq!(
            message.error.code,
            OxcCode { scope: Some("typescript".into()), number: Some("some_rule".into()) }
        );
        assert!(!message.error.labels.is_empty());
        assert_eq!(message.error.labels.len(), 1);
        assert_eq!(message.error.labels[0], LabeledSpan::new(None, 0, 10));
        assert_eq!(message.error.help, Some("Some help".into()));
        assert!(message.fixes.is_empty());
    }

    #[test]
    fn test_message_from_tsgo_lint_diagnostic_with_fixes() {
        let diagnostic = TsGoLintRuleDiagnostic {
            span: Span::new(0, 10),
            rule: "some_rule".into(),
            message: RuleMessage {
                id: "some_id".into(),
                description: "Some description".into(),
                help: None,
            },
            fixes: vec![
                Fix { text: "fixed".into(), range: Range { pos: 0, end: 5 } },
                Fix { text: "hello".into(), range: Range { pos: 5, end: 10 } },
            ],
            suggestions: vec![],
            labeled_ranges: vec![],
            file_path: "some/file/path".into(),
        };

        let message = Message::from_tsgo_lint_diagnostic(diagnostic, "Some text over 10 bytes.");

        assert_eq!(message.fixes.len(), 1);
        assert_eq!(
            message.fixes,
            PossibleFixes::Single(crate::fixer::Fix {
                content: "fixedhello".into(),
                span: Span::new(0, 10),
                message: None,
                kind: FixKind::Fix
            })
        );
    }

    #[test]
    fn test_message_from_tsgo_lint_diagnostic_with_multiple_suggestions() {
        let diagnostic = TsGoLintRuleDiagnostic {
            span: Span::new(0, 10),
            rule: "some_rule".into(),
            message: RuleMessage {
                id: "some_id".into(),
                description: "Some description".into(),
                help: None,
            },
            fixes: vec![],
            suggestions: vec![
                Suggestion {
                    message: RuleMessage {
                        id: "some_id".into(),
                        description: "Suggestion 1".into(),
                        help: None,
                    },
                    fixes: vec![Fix { text: "hello".into(), range: Range { pos: 0, end: 5 } }],
                },
                Suggestion {
                    message: RuleMessage {
                        id: "some_id".into(),
                        description: "Suggestion 2".into(),
                        help: None,
                    },
                    fixes: vec![
                        Fix { text: "hello".into(), range: Range { pos: 0, end: 5 } },
                        Fix { text: "world".into(), range: Range { pos: 5, end: 10 } },
                    ],
                },
            ],
            labeled_ranges: vec![],
            file_path: "some/file/path".into(),
        };

        let message = Message::from_tsgo_lint_diagnostic(diagnostic, "Some text over 10 bytes.");

        assert_eq!(
            message.fixes,
            PossibleFixes::Multiple(vec![
                crate::fixer::Fix {
                    content: "hello".into(),
                    span: Span::new(0, 5),
                    message: Some("Suggestion 1".into()),
                    kind: FixKind::Suggestion
                },
                crate::fixer::Fix {
                    content: "helloworld".into(),
                    span: Span::new(0, 10),
                    message: Some("Suggestion 2".into()),
                    kind: FixKind::Suggestion
                },
            ])
        );
    }

    #[test]
    fn test_message_from_tsgo_lint_diagnostic_with_fix_and_suggestions() {
        let diagnostic = TsGoLintRuleDiagnostic {
            span: Span::new(0, 10),
            rule: "some_rule".into(),
            message: RuleMessage {
                id: "some_id".into(),
                description: "Some description".into(),
                help: None,
            },
            fixes: vec![Fix { text: "fixed".into(), range: Range { pos: 0, end: 5 } }],
            suggestions: vec![Suggestion {
                message: RuleMessage {
                    id: "some_id".into(),
                    description: "Suggestion 1".into(),
                    help: None,
                },
                fixes: vec![Fix { text: "Suggestion 1".into(), range: Range { pos: 0, end: 5 } }],
            }],
            labeled_ranges: vec![],
            file_path: "some/file/path".into(),
        };

        let message = Message::from_tsgo_lint_diagnostic(diagnostic, "Some text over 10 bytes.");

        assert_eq!(message.fixes.len(), 2);
        assert_eq!(
            message.fixes,
            PossibleFixes::Multiple(vec![
                crate::fixer::Fix {
                    content: "fixed".into(),
                    span: Span::new(0, 5),
                    message: None,
                    kind: FixKind::Fix
                },
                crate::fixer::Fix {
                    content: "Suggestion 1".into(),
                    span: Span::new(0, 5),
                    message: Some("Suggestion 1".into()),
                    kind: FixKind::Suggestion,
                },
            ])
        );
    }

    #[test]
    fn test_message_from_tsgo_lint_diagnostic_with_labeled_ranges() {
        let diagnostic = TsGoLintRuleDiagnostic {
            span: Span::new(0, 10),
            rule: "some_rule".into(),
            message: RuleMessage {
                id: "some_id".into(),
                description: "Some description".into(),
                help: None,
            },
            fixes: vec![],
            suggestions: vec![],
            labeled_ranges: vec![
                LabeledRange { label: "Label 1".into(), range: Range { pos: 0, end: 5 } },
                LabeledRange { label: "Label 2".into(), range: Range { pos: 5, end: 10 } },
            ],
            file_path: "some/file/path".into(),
        };

        let message = Message::from_tsgo_lint_diagnostic(diagnostic, "Some text over 10 bytes.");

        assert!(!message.error.labels.is_empty());
        let labels = &message.error.labels;
        assert_eq!(labels.len(), 3);
        assert_eq!(labels[0], LabeledSpan::new(Some("Label 1".into()), 0, 5));
        assert_eq!(labels[1], LabeledSpan::new(Some("Label 2".into()), 5, 5));
    }

    #[test]
    fn test_diagnostic_payload_deserialize_without_fixes_or_suggestions() {
        use super::TsGoLintDiagnosticPayload;

        // Test payload with both fixes and suggestions omitted
        let json = r#"{
            "kind": 0,
            "range": {"pos": 0, "end": 10},
            "rule": "no-unused-vars",
            "message": {
                "id": "msg_id",
                "description": "Variable is unused",
                "help": null
            },
            "file_path": "test.ts"
        }"#;

        let payload: TsGoLintDiagnosticPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.fixes.len(), 0);
        assert_eq!(payload.suggestions.len(), 0);
        assert_eq!(payload.rule, Some("no-unused-vars".to_string()));

        // Test payload with only fixes omitted
        let json_with_suggestions = r#"{
            "kind": 0,
            "range": {"pos": 0, "end": 10},
            "rule": "no-unused-vars",
            "message": {
                "id": "msg_id",
                "description": "Variable is unused",
                "help": null
            },
            "suggestions": [
                {
                    "message": {
                        "id": "suggestion_id",
                        "description": "Remove unused variable",
                        "help": null
                    },
                    "fixes": []
                }
            ],
            "file_path": "test.ts"
        }"#;

        let payload: TsGoLintDiagnosticPayload =
            serde_json::from_str(json_with_suggestions).unwrap();
        assert_eq!(payload.fixes.len(), 0);
        assert_eq!(payload.suggestions.len(), 1);

        // Test payload with only suggestions omitted
        let json_with_fixes = r#"{
            "kind": 0,
            "range": {"pos": 0, "end": 10},
            "rule": "no-unused-vars",
            "message": {
                "id": "msg_id",
                "description": "Variable is unused",
                "help": null
            },
            "fixes": [
                {
                    "text": "fixed",
                    "range": {"pos": 0, "end": 5}
                }
            ],
            "file_path": "test.ts"
        }"#;

        let payload: TsGoLintDiagnosticPayload = serde_json::from_str(json_with_fixes).unwrap();
        assert_eq!(payload.fixes.len(), 1);
        assert_eq!(payload.suggestions.len(), 0);
    }

    #[test]
    fn test_diagnostic_payload_deserialize_with_labeled_ranges() {
        use super::TsGoLintDiagnosticPayload;

        let json = r#"{
            "kind": 0,
            "range": {"pos": 0, "end": 10},
            "rule": "some_rule",
            "message": {
                "id": "some_id",
                "description": "Some description",
                "help": null
            },
            "labeled_ranges": [
                {
                    "label": "Label 1",
                    "range": {"pos": 0, "end": 5}
                },
                {
                    "label": "Label 2",
                    "range": {"pos": 5, "end": 10}
                }
            ],
            "file_path": "test.ts"
        }"#;

        let payload: TsGoLintDiagnosticPayload = serde_json::from_str(json).unwrap();
        assert_eq!(payload.labeled_ranges.len(), 2);
        assert_eq!(payload.labeled_ranges[0].label, "Label 1");
        assert_eq!(payload.labeled_ranges[0].range.pos, 0);
        assert_eq!(payload.labeled_ranges[0].range.end, 5);
        assert_eq!(payload.labeled_ranges[1].label, "Label 2");
        assert_eq!(payload.labeled_ranges[1].range.pos, 5);
        assert_eq!(payload.labeled_ranges[1].range.end, 10);
    }

    #[test]
    fn test_timing_message_deserialize() {
        use super::{TsGoLintMessage, parse_single_message};

        let payload = serde_json::json!({
            "rules": [
                {"rule_name": "no-floating-promises", "duration": 123_456_u64, "calls": 7_u64}
            ]
        })
        .to_string();
        let payload = payload.as_bytes();

        let mut bytes = Vec::new();
        let payload_len: u32 = payload.len().try_into().expect("payload length should fit in u32");
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        bytes.push(2);
        bytes.extend_from_slice(payload);

        let mut cursor = std::io::Cursor::new(bytes.as_slice());
        let message = parse_single_message(&mut cursor).unwrap_or_else(|err| panic!("{err}"));

        let TsGoLintMessage::Timing(payload) = message else {
            panic!("expected timing message");
        };
        assert_eq!(payload.rules.len(), 1);
        assert_eq!(payload.rules[0].rule_name, "no-floating-promises");
        assert_eq!(payload.rules[0].duration, 123_456);
        assert_eq!(payload.rules[0].calls, 7);
    }

    fn notice_frame(payload: &serde_json::Value) -> Vec<u8> {
        let payload = payload.to_string();
        let payload = payload.as_bytes();
        let mut bytes = Vec::new();
        let payload_len: u32 = payload.len().try_into().expect("payload length should fit in u32");
        bytes.extend_from_slice(&payload_len.to_le_bytes());
        bytes.push(3);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn test_notice_message_deserialize() {
        use std::path::PathBuf;

        use super::{TsGoLintMessage, TypeAwareNotice, parse_single_message};

        let bytes = notice_frame(&serde_json::json!({
            "id": "content-mappers-require-run-external-code",
            "tsconfig": "/project/tsconfig.json",
            "extensions": [".vue", ".svelte"],
        }));
        let mut cursor = std::io::Cursor::new(bytes.as_slice());
        let message = parse_single_message(&mut cursor).unwrap_or_else(|err| panic!("{err}"));
        let TsGoLintMessage::Notice(payload) = message else {
            panic!("expected notice message");
        };
        assert_eq!(
            TypeAwareNotice::from_payload(payload),
            Some(TypeAwareNotice::ContentMappersRequireRunExternalCode {
                tsconfig: PathBuf::from("/project/tsconfig.json"),
                extensions: vec![".vue".to_string(), ".svelte".to_string()],
            })
        );
    }

    #[test]
    fn test_unknown_notice_is_ignored() {
        use super::{TsGoLintMessage, TypeAwareNotice, parse_single_message};

        // A newer backend can send notices that this version of Oxlint does not know.
        let bytes = notice_frame(&serde_json::json!({ "id": "some-future-notice", "data": 1 }));
        let mut cursor = std::io::Cursor::new(bytes.as_slice());
        let message = parse_single_message(&mut cursor).unwrap_or_else(|err| panic!("{err}"));
        let TsGoLintMessage::Notice(payload) = message else {
            panic!("expected notice message");
        };
        assert_eq!(TypeAwareNotice::from_payload(payload), None);
    }

    #[test]
    fn test_payload_report_notices_is_omitted_when_false() {
        use super::Payload;

        // Older backends must receive the same payload as before.
        let mut payload = Payload {
            version: 2,
            configs: vec![],
            source_overrides: None,
            report_syntactic: false,
            report_semantic: false,
            report_notices: false,
        };
        let json = serde_json::to_value(&payload).unwrap();
        assert!(json.get("report_notices").is_none());

        payload.report_notices = true;
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["report_notices"], serde_json::Value::Bool(true));
        assert_eq!(json["version"], serde_json::json!(2));
    }

    #[test]
    fn test_fixes_are_dropped_in_content_mapped_files() {
        use std::path::{Path, PathBuf};

        use super::{drop_fixes_in_content_mapped_file, is_content_mapped_file};

        assert!(is_content_mapped_file(Path::new("/project/App.vue")));
        assert!(is_content_mapped_file(Path::new("/project/Page.svelte")));
        assert!(is_content_mapped_file(Path::new("/project/index.astro")));
        assert!(!is_content_mapped_file(Path::new("/project/index.ts")));
        assert!(!is_content_mapped_file(Path::new("/project/vue")));

        let diagnostic = |file_path: &str| TsGoLintRuleDiagnostic {
            span: Span::new(0, 6),
            rule: "no-floating-promises".to_string(),
            message: RuleMessage {
                id: "floating".to_string(),
                description: "Promises must be awaited.".to_string(),
                help: None,
            },
            fixes: vec![Fix { text: "void ".to_string(), range: Range { pos: 0, end: 0 } }],
            suggestions: vec![Suggestion {
                message: RuleMessage {
                    id: "addVoid".to_string(),
                    description: "Add void operator.".to_string(),
                    help: None,
                },
                fixes: vec![Fix { text: "void ".to_string(), range: Range { pos: 0, end: 0 } }],
            }],
            labeled_ranges: vec![],
            file_path: PathBuf::from(file_path),
        };

        let mut vue = diagnostic("/project/App.vue");
        drop_fixes_in_content_mapped_file(&mut vue);
        assert!(vue.fixes.is_empty());
        assert!(vue.suggestions.is_empty());

        let mut ts = diagnostic("/project/index.ts");
        drop_fixes_in_content_mapped_file(&mut ts);
        assert_eq!(ts.fixes.len(), 1);
        assert_eq!(ts.suggestions.len(), 1);
    }

    #[test]
    fn test_btreeset_preserves_rules_with_different_options() {
        use super::Rule;
        use std::collections::BTreeSet;

        // Create two rules with the same name but different options
        let rule1 = Rule {
            name: "no-floating-promises".to_string(),
            options: Some(serde_json::json!({"ignoreVoid": true})),
        };

        let rule2 = Rule {
            name: "no-floating-promises".to_string(),
            options: Some(serde_json::json!({"ignoreVoid": false})),
        };

        let rule3 = Rule { name: "no-floating-promises".to_string(), options: None };

        // Insert into BTreeSet
        let mut rules = BTreeSet::new();
        rules.insert(rule1.clone());
        rules.insert(rule2.clone());
        rules.insert(rule3.clone());

        // All three distinct rules should be preserved
        assert_eq!(rules.len(), 3, "BTreeSet should preserve all rules with different options");

        // Verify all rules are present
        assert!(rules.contains(&rule1), "Rule with ignoreVoid: true should be present");
        assert!(rules.contains(&rule2), "Rule with ignoreVoid: false should be present");
        assert!(rules.contains(&rule3), "Rule with no options should be present");
    }

    #[test]
    fn test_btreeset_deduplicates_identical_rules() {
        use super::Rule;
        use std::collections::BTreeSet;

        let rule1 = Rule {
            name: "no-floating-promises".to_string(),
            options: Some(serde_json::json!({"ignoreVoid": true})),
        };

        let rule2 = Rule {
            name: "no-floating-promises".to_string(),
            options: Some(serde_json::json!({"ignoreVoid": true})),
        };

        let mut rules = BTreeSet::new();
        rules.insert(rule1);
        rules.insert(rule2);

        // Identical rules should be deduplicated
        assert_eq!(rules.len(), 1, "BTreeSet should deduplicate identical rules");
    }
}
