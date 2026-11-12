# Exit code
1

# stdout
```
Failed to parse oxlint configuration file.

  x Plugin name 'import' is reserved, and cannot be used for JS plugins.
  | 
  | The 'import' plugin is already implemented natively in Rust within oxlint.
  | Using both the native and JS versions would create ambiguity about which rules to use.
  | 
  | To use an external 'import' plugin instead, provide a custom alias:
  | 
  | "jsPlugins": [{ "name": "import-js", "specifier": "eslint-plugin-import" }]
  | 
  | Then reference rules using your alias:
  | 
  | "rules": {
  |   "import-js/rule-name": "error"
  | }
  | 
  | Disable comments can name the rules with the alias or with the plugin's own name.
  | If the plugin's own name is 'import', then `// eslint-disable-next-line import/rule-name`
  | also disables 'import-js/rule-name'.
  | 
  | See: https://oxc.rs/docs/guide/usage/linter/js-plugins.html
```

# stderr
```
```
