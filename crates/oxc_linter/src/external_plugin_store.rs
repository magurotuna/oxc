use std::{
    fmt,
    path::{Path, PathBuf},
};

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use oxc_index::{IndexVec, define_index_type, index_vec};
use serde::{Serialize, Serializer};

use crate::{ExternalLinter, loader::LINTABLE_EXTENSIONS};

define_index_type! {
    pub struct ExternalPluginId = u32;
    MAX_INDEX = u32::MAX as usize;
}

define_index_type! {
    pub struct ExternalRuleId = u32;
    MAX_INDEX = u32::MAX as usize;
}

impl ExternalRuleId {
    /// Dummy value used in first element of `ExternalPluginStore::options`, which is a dummy
    pub const DUMMY: Self = Self::from_usize(0);
}

define_index_type! {
    pub struct ExternalOptionsId = u32;
    MAX_INDEX = u32::MAX as usize;
}

define_index_type! {
    pub struct LanguagePluginId = u32;
    MAX_INDEX = u32::MAX as usize;
}

impl ExternalOptionsId {
    /// The value `0`.
    /// Used as the ID when a rule does not have options.
    pub const NONE: Self = Self::from_usize(0);
}

#[derive(Debug)]
pub struct ExternalPluginStore {
    registered_plugin_paths: FxHashSet<PathBuf>,

    plugins: IndexVec<ExternalPluginId, ExternalPlugin>,
    plugin_names: FxHashMap<String, ExternalPluginId>,
    rules: IndexVec<ExternalRuleId, ExternalRule>,
    /// Options for a rule, indexed by `ExternalOptionsId`.
    /// The rule ID is also stored, so that can merge options with the rule's default options on JS side.
    options: IndexVec<ExternalOptionsId, (ExternalRuleId, SmallVec<[serde_json::Value; 1]>)>,

    /// Language plugins, indexed by the ID that JS gave them.
    language_plugins: IndexVec<LanguagePluginId, LanguagePlugin>,
    language_plugin_paths: FxHashMap<PathBuf, LanguagePluginId>,

    is_enabled: bool,
}

impl Default for ExternalPluginStore {
    fn default() -> Self {
        Self::new(true)
    }
}

impl ExternalPluginStore {
    pub fn new(is_enabled: bool) -> Self {
        let options = index_vec![(ExternalRuleId::DUMMY, SmallVec::new())];

        Self {
            registered_plugin_paths: FxHashSet::default(),
            plugins: IndexVec::default(),
            plugin_names: FxHashMap::default(),
            rules: IndexVec::default(),
            options,
            language_plugins: IndexVec::default(),
            language_plugin_paths: FxHashMap::default(),
            is_enabled,
        }
    }

    /// Returns `true` if external plugins are enabled.
    pub fn is_enabled(&self) -> bool {
        self.is_enabled
    }

    /// Returns `true` if no external plugins have been loaded: no JS plugins and no language plugins.
    ///
    /// A language plugin needs the external linter even without JS plugin rules, because it returns
    /// the markup references and comments of its files.
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty() && self.language_plugins.is_empty()
    }

    pub fn is_plugin_registered(&self, plugin_path: &Path) -> bool {
        self.registered_plugin_paths.contains(plugin_path)
    }

    /// Register plugin.
    ///
    /// # Panics
    /// Panics if:
    /// - Plugin at `plugin_path` is already registered.
    /// - `offset` does not equal the number of registered rules.
    pub fn register_plugin(
        &mut self,
        plugin_path: PathBuf,
        plugin_name: String,
        canonical_name: Option<String>,
        offset: usize,
        rule_names: Vec<String>,
    ) {
        let newly_inserted = self.registered_plugin_paths.insert(plugin_path);
        assert!(newly_inserted, "register_plugin: plugin already registered");

        let canonical_name = canonical_name.filter(|name| *name != plugin_name);
        let plugin_id = self.plugins.push(ExternalPlugin {
            name: plugin_name.clone(),
            canonical_name,
            rules: FxHashMap::default(),
        });
        self.plugin_names.insert(plugin_name, plugin_id);

        assert!(
            offset == self.rules.len(),
            "register_plugin: received offset {}, but rule table is currently {} long",
            offset,
            self.rules.len()
        );

        for rule_name in rule_names {
            let rule_id = self.rules.push(ExternalRule { name: rule_name.clone(), plugin_id });
            self.plugins[plugin_id].rules.insert(rule_name, rule_id);
        }
    }

    /// Returns the ID of the language plugin at `plugin_path`, if it is registered.
    pub fn language_plugin_id(&self, plugin_path: &Path) -> Option<LanguagePluginId> {
        self.language_plugin_paths.get(plugin_path).copied()
    }

    /// Register a language plugin that JS has loaded.
    ///
    /// # Errors
    /// Returns an error if an entry of `default_files` is not a file extension or a file name.
    ///
    /// # Panics
    /// Panics if the plugin at `plugin_path` is already registered, or if `id` is not the next ID.
    pub fn register_language_plugin(
        &mut self,
        plugin_path: PathBuf,
        id: u32,
        name: String,
        default_files: &[String],
    ) -> Result<LanguagePluginId, String> {
        assert!(
            !self.language_plugin_paths.contains_key(&plugin_path),
            "register_language_plugin: plugin already registered"
        );
        assert!(
            id as usize == self.language_plugins.len(),
            "register_language_plugin: received ID {id}, but {} language plugins are registered",
            self.language_plugins.len()
        );
        // Register the plugin also if `default_files` is not valid, so that the IDs stay the same
        // as on the JS side, which has already registered the plugin.
        let parsed_default_files = LanguagePluginDefaultFiles::parse(default_files);
        let plugin_id = self.language_plugins.push(LanguagePlugin {
            name,
            default_files: parsed_default_files.clone().unwrap_or_default(),
        });
        self.language_plugin_paths.insert(plugin_path, plugin_id);
        parsed_default_files.map(|_| plugin_id)
    }

    /// Returns the language plugin with this ID.
    pub fn language_plugin(&self, plugin_id: LanguagePluginId) -> &LanguagePlugin {
        &self.language_plugins[plugin_id]
    }

    /// # Errors
    /// Returns an error if the plugin, or rule could not be found
    pub fn lookup_rule_id(
        &self,
        plugin_name: &str,
        rule_name: &str,
    ) -> Result<ExternalRuleId, ExternalRuleLookupError> {
        let plugin_id = *self.plugin_names.get(plugin_name).ok_or_else(|| {
            ExternalRuleLookupError::PluginNotFound { plugin: plugin_name.to_string() }
        })?;

        self.plugins[plugin_id].rules.get(rule_name).copied().ok_or_else(|| {
            ExternalRuleLookupError::RuleNotFound {
                plugin: plugin_name.to_string(),
                rule: rule_name.to_string(),
            }
        })
    }

    pub fn resolve_plugin_rule_names(
        &self,
        external_rule_id: ExternalRuleId,
    ) -> (/* plugin name */ &str, /* rule name */ &str) {
        let external_rule = &self.rules[external_rule_id];
        let plugin = &self.plugins[external_rule.plugin_id];
        (&plugin.name, &external_rule.name)
    }

    /// Returns the names that disable directives can use for a rule: `plugin/rule`, and also
    /// `canonical/rule` if the plugin is loaded under an alias. For example, a directive can name
    /// the `no-v-html` rule of eslint-plugin-vue loaded as `vuejs` as `vuejs/no-v-html` or as
    /// `vue/no-v-html`, the name that ESLint configs use.
    pub fn directive_rule_names(&self, external_rule_id: ExternalRuleId) -> Vec<String> {
        let external_rule = &self.rules[external_rule_id];
        let plugin = &self.plugins[external_rule.plugin_id];
        let mut names = vec![format!("{}/{}", plugin.name, external_rule.name)];
        if let Some(canonical_name) = &plugin.canonical_name {
            names.push(format!("{canonical_name}/{}", external_rule.name));
        }
        names
    }

    /// Returns the alias of a JS plugin whose own name is `canonical_name` and which has the rule
    /// `rule_name`. For example, `vuejs` for (`vue`, `no-v-html`) if eslint-plugin-vue is loaded
    /// as `vuejs`.
    pub fn find_alias_of_rule(&self, canonical_name: &str, rule_name: &str) -> Option<&str> {
        self.plugins
            .iter()
            .find(|plugin| {
                plugin.canonical_name.as_deref() == Some(canonical_name)
                    && plugin.rules.contains_key(rule_name)
            })
            .map(|plugin| plugin.name.as_str())
    }

    /// Add options to the store and return its [`ExternalOptionsId`].
    /// If `options` is empty, returns [`ExternalOptionsId::NONE`] without adding to the store.
    pub fn add_options(
        &mut self,
        rule_id: ExternalRuleId,
        options: &SmallVec<[serde_json::Value; 1]>,
    ) -> ExternalOptionsId {
        if options.is_empty() {
            ExternalOptionsId::NONE
        } else {
            self.options.push((rule_id, options.clone()))
        }
    }

    /// Send options to JS side.
    ///
    /// # Errors
    /// Returns an error if serialization of rule options fails.
    pub fn setup_rule_configs(
        &self,
        cwd: String,
        workspace_uri: Option<&str>,
        external_linter: &ExternalLinter,
    ) -> Result<(), String> {
        let json = serde_json::to_string(&ConfigSer::new(cwd, workspace_uri, self));
        match json {
            Ok(options_json) => (external_linter.setup_rule_configs)(options_json),
            Err(err) => Err(format!("Failed to serialize external plugin options: {err}")),
        }
    }
}

/// Wrapper struct for serializing options.
///
/// Splits rule IDs and options into separate arrays, without collecting into intermediate `Vec`s.
///
/// Input (`ExternalPluginStore::options`):
/// ```ignore
/// [
///   (rule_id1, [option_1a, option_1b]),
///   (rule_id2, [option_2a, option_2b]),
///   ...
/// ]
/// ```
///
/// Output JSON:
/// ```json
/// {
///   "ruleIds": [rule_id1, rule_id2, ...],
///   "options": [
///     [option_1a, option_1b],
///     [option_2a, option_2b],
///     ...
///   ],
/// }
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigSer<'s> {
    cwd: String,
    workspace_uri: Option<&'s str>,
    rule_ids: ConfigSerRuleIds<'s>,
    options: ConfigSerOptions<'s>,
}

impl<'s> ConfigSer<'s> {
    fn new(
        cwd: String,
        workspace_uri: Option<&'s str>,
        external_plugin_store: &'s ExternalPluginStore,
    ) -> Self {
        Self {
            cwd,
            workspace_uri,
            rule_ids: ConfigSerRuleIds(external_plugin_store),
            options: ConfigSerOptions(external_plugin_store),
        }
    }
}

struct ConfigSerRuleIds<'s>(&'s ExternalPluginStore);

impl Serialize for ConfigSerRuleIds<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.options.iter().map(|(rule_id, _)| rule_id))
    }
}

struct ConfigSerOptions<'s>(&'s ExternalPluginStore);

impl Serialize for ConfigSerOptions<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(self.0.options.iter().map(|(_, options)| options))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExternalRuleLookupError {
    PluginNotFound { plugin: String },
    RuleNotFound { plugin: String, rule: String },
}

impl fmt::Display for ExternalRuleLookupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExternalRuleLookupError::PluginNotFound { plugin } => {
                write!(f, "Plugin '{plugin}' not found")
            }
            ExternalRuleLookupError::RuleNotFound { plugin, rule } => {
                write!(f, "Rule '{rule}' not found in plugin '{plugin}'")
            }
        }
    }
}

impl std::error::Error for ExternalRuleLookupError {}

#[derive(Debug)]
struct ExternalPlugin {
    name: String,
    /// The name of the plugin itself, if the plugin is loaded under an alias and the name differs
    /// from the alias. For example, `vue` for eslint-plugin-vue loaded as `vuejs`.
    /// Disable directives can name the rules of the plugin with this name too.
    canonical_name: Option<String>,
    rules: FxHashMap<String, ExternalRuleId>,
}

#[derive(Debug)]
struct ExternalRule {
    name: String,
    plugin_id: ExternalPluginId,
}

/// A loaded language plugin.
#[derive(Debug)]
pub struct LanguagePlugin {
    /// Name of the plugin: the alias from the config, or the name that the plugin defines.
    pub name: String,
    /// Files that the plugin parses if its config entry has no `pattern`.
    pub default_files: LanguagePluginDefaultFiles,
}

/// The `defaultFiles` of a language plugin.
///
/// Plugins give file extensions and file names, not other globs, because the project config
/// decides which language plugin parses a file. Entries can be:
/// - an extension: `.vue`, or the glob form `**/*.vue` or `*.vue`
/// - a file name: `Dockerfile`
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LanguagePluginDefaultFiles {
    /// Extensions with the leading dot, for example `.vue`.
    /// An entry also matches a file whose whole name is the extension, for example `.prettierrc`.
    extensions: Vec<String>,
    /// File names.
    file_names: Vec<String>,
}

impl LanguagePluginDefaultFiles {
    /// Parse the `defaultFiles` list of a plugin.
    ///
    /// # Errors
    /// Returns an error for an entry that is not an extension or a file name, and for an entry of
    /// a file type that Oxlint cannot lint yet.
    pub fn parse(entries: &[String]) -> Result<Self, String> {
        fn has_glob_or_separator(text: &str) -> bool {
            text.contains(['*', '?', '[', ']', '{', '}', '!', '/', '\\'])
        }
        // Oxlint finds and reads only files with these extensions. Other file types need changes in
        // file discovery, so they are an error for now, instead of a plugin that never runs.
        fn check_lintable(entry: &str, extension: Option<&str>) -> Result<(), String> {
            if extension.is_some_and(|extension| LINTABLE_EXTENSIONS.contains(&extension)) {
                return Ok(());
            }
            Err(format!(
                "`defaultFiles` entry `{entry}` is not supported yet. \
                 Language plugins can parse only files with these extensions: {}.",
                LINTABLE_EXTENSIONS
                    .iter()
                    .map(|extension| format!("`.{extension}`"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }

        let mut default_files = Self::default();
        for entry in entries {
            let extension =
                entry.strip_prefix("**/*").or_else(|| entry.strip_prefix('*')).unwrap_or(entry);
            if extension.starts_with('.')
                && extension.len() > 1
                && !has_glob_or_separator(extension)
            {
                check_lintable(entry, extension.rsplit('.').next())?;
                default_files.extensions.push(extension.to_string());
            } else if extension.len() == entry.len()
                && !matches!(entry.as_str(), "" | "." | "..")
                && !has_glob_or_separator(entry)
            {
                check_lintable(entry, Path::new(entry).extension().and_then(|ext| ext.to_str()))?;
                default_files.file_names.push(entry.clone());
            } else {
                return Err(format!(
                    "`defaultFiles` entry `{entry}` is not supported. \
                     Use a file extension such as `.vue` or `**/*.vue`, or a file name. \
                     To select other files, set `pattern` for the plugin in the config."
                ));
            }
        }
        Ok(default_files)
    }

    /// Returns `true` if the file name matches one of the entries.
    pub fn matches(&self, file_name: &str) -> bool {
        self.extensions.iter().any(|extension| file_name.ends_with(extension.as_str()))
            || self.file_names.iter().any(|name| name == file_name)
    }

    /// Returns `true` if there are no entries.
    pub fn is_empty(&self) -> bool {
        self.extensions.is_empty() && self.file_names.is_empty()
    }
}

#[cfg(test)]
mod test_language_plugin_default_files {
    use super::LanguagePluginDefaultFiles;

    fn parse(entries: &[&str]) -> Result<LanguagePluginDefaultFiles, String> {
        LanguagePluginDefaultFiles::parse(
            &entries.iter().map(ToString::to_string).collect::<Vec<_>>(),
        )
    }

    #[test]
    fn extensions_and_file_names() {
        let default_files = parse(&[".vue", "**/*.svelte", "*.astro", "vite.config.ts"]).unwrap();
        assert!(default_files.matches("App.vue"));
        assert!(default_files.matches("App.svelte"));
        assert!(default_files.matches("index.astro"));
        assert!(default_files.matches("vite.config.ts"));
        assert!(!default_files.matches("App.ts"));
        assert!(!default_files.matches("vue"));
        assert!(!default_files.matches("vite.config.ts.bak"));

        let default_files = parse(&[".d.ts"]).unwrap();
        assert!(default_files.matches("types.d.ts"));
        assert!(!default_files.matches("types.ts"));
    }

    #[test]
    fn file_types_that_oxlint_cannot_lint_are_errors() {
        for entry in [".json", ".md", "**/*.html", "Dockerfile", ".prettierrc", "package.json"] {
            let error = parse(&[entry]).unwrap_err();
            assert!(error.contains("is not supported yet"), "`{entry}`: {error}");
        }
    }

    #[test]
    fn other_globs_are_errors() {
        for entry in ["src/**/*.vue", "**/*.{vue,svelte}", "*", "**/*", "a?.vue", "", "."] {
            assert!(parse(&[entry]).is_err(), "`{entry}` should be an error");
        }
    }
}

#[cfg(test)]
mod test_directive_rule_names {
    use std::path::PathBuf;

    use super::ExternalPluginStore;

    #[test]
    fn an_alias_adds_the_name_of_the_plugin_itself() {
        let mut store = ExternalPluginStore::new(true);
        store.register_plugin(
            PathBuf::from("eslint-plugin-vue"),
            "vuejs".into(),
            Some("vue".into()),
            0,
            vec!["no-v-html".into()],
        );
        store.register_plugin(
            PathBuf::from("eslint-plugin-foo"),
            "foo".into(),
            Some("foo".into()),
            1,
            vec!["rule".into()],
        );

        let vue_rule = store.lookup_rule_id("vuejs", "no-v-html").unwrap();
        assert_eq!(store.directive_rule_names(vue_rule), ["vuejs/no-v-html", "vue/no-v-html"]);
        // Without an alias, the plugin has one name
        let foo_rule = store.lookup_rule_id("foo", "rule").unwrap();
        assert_eq!(store.directive_rule_names(foo_rule), ["foo/rule"]);

        assert_eq!(store.find_alias_of_rule("vue", "no-v-html"), Some("vuejs"));
        assert_eq!(store.find_alias_of_rule("vue", "no-v-for"), None);
        assert_eq!(store.find_alias_of_rule("foo", "rule"), None);
    }
}
