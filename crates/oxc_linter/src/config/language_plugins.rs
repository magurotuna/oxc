use std::path::PathBuf;

use schemars::{
    JsonSchema, SchemaGenerator,
    schema::{
        ArrayValidation, InstanceType, Metadata, ObjectValidation, Schema, SchemaObject,
        SingleOrVec, SubschemaValidation,
    },
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use oxc_config::validate_glob_pattern;

/// Entry of the `languagePlugins` list.
///
/// A language plugin parses files of one language (for example, Vue single-file components)
/// into the syntax tree of that language, so that JS plugin rules can lint the whole file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguagePluginEntry {
    /// Directory that contains the config file that specified this plugin.
    pub config_dir: PathBuf,
    /// Plugin specifier (path or package name).
    pub specifier: String,
    /// Optional name of the plugin, used instead of the name that the plugin defines.
    pub name: Option<String>,
    /// Glob patterns of the files that the plugin parses, relative to `config_dir`.
    /// If `None`, the plugin's `defaultFiles` are used.
    pub pattern: Option<Vec<String>>,
    /// Options passed to the plugin's `parse` and `load` functions.
    pub options: Option<serde_json::Value>,
}

/// Custom deserializer for `LanguagePluginEntry`.
/// Supports:
/// * String: `"vue-language-plugin"`
/// * Object: `{ "name": "vue-language-plugin", "pattern": "src/**/*.vue", "options": {} }`.
///   If the object also has `specifier`, then `name` is an alias for the plugin at `specifier`.
impl<'de> Deserialize<'de> for LanguagePluginEntry {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Pattern {
            One(String),
            Many(Vec<String>),
        }

        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PluginObject {
            name: String,
            specifier: Option<String>,
            pattern: Option<Pattern>,
            options: Option<serde_json::Value>,
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum PluginEntry {
            String(String),
            Object(PluginObject),
        }

        let entry = PluginEntry::deserialize(deserializer).map_err(|_| {
            serde::de::Error::custom(
                "a language plugin entry must be a string, or an object with `name` and optional `specifier`, `pattern`, and `options`",
            )
        })?;

        let entry = match entry {
            PluginEntry::String(specifier) => LanguagePluginEntry {
                config_dir: PathBuf::default(),
                specifier,
                name: None,
                pattern: None,
                options: None,
            },
            PluginEntry::Object(obj) => {
                let pattern = obj.pattern.map(|pattern| match pattern {
                    Pattern::One(pattern) => vec![pattern],
                    Pattern::Many(patterns) => patterns,
                });
                if let Some(patterns) = &pattern {
                    if patterns.is_empty() {
                        return Err(serde::de::Error::custom(
                            "`pattern` of a language plugin must not be empty",
                        ));
                    }
                    for pattern in patterns {
                        validate_glob_pattern(pattern).map_err(serde::de::Error::custom)?;
                    }
                }
                let (specifier, name) = match obj.specifier {
                    Some(specifier) => (specifier, Some(obj.name)),
                    None => (obj.name, None),
                };
                LanguagePluginEntry {
                    config_dir: PathBuf::default(),
                    specifier,
                    name,
                    pattern,
                    options: obj.options,
                }
            }
        };
        Ok(entry)
    }
}

/// Custom serializer for `LanguagePluginEntry`.
/// * Serializes an entry with only a specifier as the specifier, for example `"vue-language-plugin"`.
/// * Serializes other entries as an object.
impl Serialize for LanguagePluginEntry {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct PluginObject<'e> {
            name: &'e str,
            #[serde(skip_serializing_if = "Option::is_none")]
            specifier: Option<&'e str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            pattern: Option<&'e [String]>,
            #[serde(skip_serializing_if = "Option::is_none")]
            options: Option<&'e serde_json::Value>,
        }

        if self.name.is_none() && self.pattern.is_none() && self.options.is_none() {
            return self.specifier.serialize(serializer);
        }

        let (name, specifier) = match &self.name {
            Some(name) => (name.as_str(), Some(self.specifier.as_str())),
            None => (self.specifier.as_str(), None),
        };
        PluginObject {
            name,
            specifier,
            pattern: self.pattern.as_deref(),
            options: self.options.as_ref(),
        }
        .serialize(serializer)
    }
}

/// JSON schema of the `languagePlugins` field: `null`, or an array of entries.
pub fn language_plugins_schema(generator: &mut SchemaGenerator) -> Schema {
    let entry_schema = generator.subschema_for::<LanguagePluginEntry>();

    let array_schema = SchemaObject {
        instance_type: Some(InstanceType::Array.into()),
        array: Some(Box::new(ArrayValidation {
            items: Some(entry_schema.into()),
            ..Default::default()
        })),
        ..Default::default()
    };

    SchemaObject {
        subschemas: Some(Box::new(SubschemaValidation {
            any_of: Some(vec![
                SchemaObject {
                    instance_type: Some(InstanceType::Null.into()),
                    ..Default::default()
                }
                .into(),
                array_schema.into(),
            ]),
            ..Default::default()
        })),
        ..Default::default()
    }
    .into()
}

impl JsonSchema for LanguagePluginEntry {
    fn schema_name() -> String {
        "LanguagePluginEntry".to_string()
    }

    fn json_schema(_gen: &mut SchemaGenerator) -> Schema {
        fn string_schema(description: &str) -> SchemaObject {
            SchemaObject {
                instance_type: Some(InstanceType::String.into()),
                metadata: Some(Box::new(Metadata {
                    description: Some(description.to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            }
        }

        let string_entry_schema = string_schema("Path or package name of the language plugin");

        let pattern_schema = SchemaObject {
            metadata: Some(Box::new(Metadata {
                description: Some(
                    "Glob patterns of the files that this plugin parses, relative to the directory of the config file.\n\n\
                     If you do not set `pattern`, the plugin parses the files in its `defaultFiles` list."
                        .to_string(),
                ),
                ..Default::default()
            })),
            subschemas: Some(Box::new(SubschemaValidation {
                any_of: Some(vec![
                    SchemaObject {
                        instance_type: Some(InstanceType::String.into()),
                        ..Default::default()
                    }
                    .into(),
                    SchemaObject {
                        instance_type: Some(InstanceType::Array.into()),
                        array: Some(Box::new(ArrayValidation {
                            items: Some(SingleOrVec::Single(Box::new(
                                SchemaObject {
                                    instance_type: Some(InstanceType::String.into()),
                                    ..Default::default()
                                }
                                .into(),
                            ))),
                            ..Default::default()
                        })),
                        ..Default::default()
                    }
                    .into(),
                ]),
                ..Default::default()
            })),
            ..Default::default()
        };

        let options_schema = SchemaObject {
            metadata: Some(Box::new(Metadata {
                description: Some(
                    "Options passed to the `parse` and `load` functions of the plugin.".to_string(),
                ),
                ..Default::default()
            })),
            ..Default::default()
        };

        let mut object_properties = schemars::Map::new();
        object_properties.insert(
            "name".to_string(),
            string_schema(
                "Package name or path of the language plugin.\n\n\
                 If you also set `specifier`, then `name` is a custom name for the plugin at `specifier`.",
            )
            .into(),
        );
        object_properties.insert(
            "specifier".to_string(),
            string_schema("Path or package name of the language plugin").into(),
        );
        object_properties.insert("pattern".to_string(), pattern_schema.into());
        object_properties.insert("options".to_string(), options_schema.into());

        let object_schema = SchemaObject {
            instance_type: Some(InstanceType::Object.into()),
            object: Some(Box::new(ObjectValidation {
                properties: object_properties,
                required: std::iter::once("name".to_string()).collect(),
                additional_properties: Some(Box::new(false.into())),
                ..Default::default()
            })),
            ..Default::default()
        };

        SchemaObject {
            subschemas: Some(Box::new(SubschemaValidation {
                any_of: Some(vec![string_entry_schema.into(), object_schema.into()]),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
    }
}

#[cfg(test)]
mod test {
    use serde_json::json;

    use super::LanguagePluginEntry;

    #[test]
    fn test_deserialize_string_entry() {
        let entry: LanguagePluginEntry = serde_json::from_value(json!("vue-plugin")).unwrap();
        assert_eq!(entry.specifier, "vue-plugin");
        assert_eq!(entry.name, None);
        assert_eq!(entry.pattern, None);
        assert_eq!(entry.options, None);
    }

    #[test]
    fn test_deserialize_object_entry() {
        let entry: LanguagePluginEntry = serde_json::from_value(json!({
            "name": "vue-plugin",
            "pattern": "packages/app2/*.vue",
            "options": { "a": 1 }
        }))
        .unwrap();
        assert_eq!(entry.specifier, "vue-plugin");
        assert_eq!(entry.name, None);
        assert_eq!(entry.pattern, Some(vec!["packages/app2/*.vue".to_string()]));
        assert_eq!(entry.options, Some(json!({ "a": 1 })));

        let entry: LanguagePluginEntry = serde_json::from_value(json!({
            "name": "vue",
            "specifier": "./vue-plugin.js",
            "pattern": ["a/*.vue", "b/*.vue"]
        }))
        .unwrap();
        assert_eq!(entry.specifier, "./vue-plugin.js");
        assert_eq!(entry.name.as_deref(), Some("vue"));
        assert_eq!(entry.pattern, Some(vec!["a/*.vue".to_string(), "b/*.vue".to_string()]));
    }

    #[test]
    fn test_deserialize_invalid_entries() {
        for value in [
            json!(1),
            json!({ "specifier": "vue-plugin" }),
            json!({ "name": "vue-plugin", "files": ["*.vue"] }),
            json!({ "name": "vue-plugin", "pattern": [] }),
            json!({ "name": "vue-plugin", "pattern": "a/[" }),
        ] {
            assert!(
                serde_json::from_value::<LanguagePluginEntry>(value.clone()).is_err(),
                "{value} should not deserialize"
            );
        }
    }

    #[test]
    fn test_serialize_round_trip() {
        for value in [
            json!("vue-plugin"),
            json!({ "name": "vue-plugin", "pattern": ["*.vue"] }),
            json!({ "name": "vue", "specifier": "./vue-plugin.js", "options": { "a": 1 } }),
        ] {
            let entry: LanguagePluginEntry = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&entry).unwrap(), value);
        }
    }
}
