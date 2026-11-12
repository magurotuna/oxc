# Exit code
1

# stdout
```
Failed to parse oxlint configuration file.

  x Plugin name 'jsdoc' is reserved, and cannot be used for JS plugins.
  | 
  | The 'jsdoc' plugin is already implemented natively in Rust within oxlint.
  | Using both the native and JS versions would create ambiguity about which rules to use.
  | 
  | To use an external 'jsdoc' plugin instead, provide a custom alias:
  | 
  | "jsPlugins": [{ "name": "jsdoc-js", "specifier": "eslint-plugin-jsdoc" }]
  | 
  | Then reference rules using your alias:
  | 
  | "rules": {
  |   "jsdoc-js/rule-name": "error"
  | }
  | 
  | Disable comments can name the rules with the alias or with the plugin's own name.
  | If the plugin's own name is 'jsdoc', then `// eslint-disable-next-line jsdoc/rule-name`
  | also disables 'jsdoc-js/rule-name'.
  | 
  | See: https://oxc.rs/docs/guide/usage/linter/js-plugins.html
```

# stderr
```
```
