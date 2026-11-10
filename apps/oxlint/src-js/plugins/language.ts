/*
 * Language plugins: loading.
 *
 * A language plugin parses files of another language (for example, Vue single-file components)
 * into the syntax tree of that language. JS plugin rules then run on that syntax tree.
 */

import { switchWorkspace } from "./workspace.ts";
import { getErrorMessage } from "../utils/utils.ts";

import type { LanguagePlugin, VisitorKeysSchema } from "../package/language_plugin.ts";

/**
 * A loaded language plugin.
 */
export interface LanguagePluginDetails {
  plugin: LanguagePlugin;
  name: string;
  // Child field names of each node type in the plugin's schema
  keys: Record<string, readonly string[]>;
}

// Loaded language plugins, indexed by the ID that is sent to Rust.
// May be changed when switching workspaces.
export let registeredLanguagePlugins: LanguagePluginDetails[] = [];

/**
 * Set `registeredLanguagePlugins`. Used when switching workspaces.
 * @param plugins - Loaded language plugins of the workspace
 */
export function setRegisteredLanguagePlugins(plugins: LanguagePluginDetails[]): void {
  registeredLanguagePlugins = plugins;
}

/**
 * Load a language plugin.
 *
 * @param url - Absolute path of plugin file as a `file://...` URL
 * @param alias - Name from the config, or `null`
 * @param workspaceUri - Workspace URI (`null` in CLI, string in LSP)
 * @returns Plugin details or error, serialized to JSON string
 */
export async function loadLanguagePlugin(
  url: string,
  alias: string | null,
  workspaceUri: string | null,
): Promise<string> {
  try {
    const plugin = (await import(url)).default as LanguagePlugin;
    // Switch workspace after `await`, as another workspace may have been switched to in the meantime
    if (workspaceUri !== null) switchWorkspace(workspaceUri);
    const details = registerLanguagePlugin(plugin, alias);
    return JSON.stringify({ Success: details });
  } catch (err) {
    return JSON.stringify({ Failure: getErrorMessage(err) });
  }
}

/**
 * Validate and register a language plugin.
 *
 * @param plugin - Default export of the plugin module
 * @param alias - Name from the config, or `null`
 * @returns ID, name, and `defaultFiles` of the plugin
 * @throws {TypeError} If the plugin is not valid
 */
function registerLanguagePlugin(
  plugin: LanguagePlugin,
  alias: string | null,
): { id: number; name: string; defaultFiles: string[] } {
  if (plugin === null || typeof plugin !== "object") {
    throw new TypeError("A language plugin must be an object (the default export of the module)");
  }

  const { meta } = plugin;
  if (meta === null || typeof meta !== "object") {
    throw new TypeError("A language plugin must have a `meta` object");
  }
  const pluginName = meta.name;
  if (typeof pluginName !== "string" || pluginName === "") {
    throw new TypeError("`meta.name` of a language plugin must be a non-empty string");
  }

  if (typeof plugin.parse !== "function") {
    throw new TypeError(`Language plugin \`${pluginName}\` must have a \`parse\` function`);
  }
  if (typeof plugin.load !== "function") {
    throw new TypeError(`Language plugin \`${pluginName}\` must have a \`load\` function`);
  }

  let defaultFiles: string[] = [];
  if (plugin.defaultFiles != null) {
    if (
      !Array.isArray(plugin.defaultFiles)
      || plugin.defaultFiles.some((entry) => typeof entry !== "string")
    ) {
      throw new TypeError(
        `\`defaultFiles\` of language plugin \`${pluginName}\` must be an array of strings`,
      );
    }
    defaultFiles = plugin.defaultFiles;
  }

  const keys = compileVisitorKeys(plugin.visitorKeys, pluginName);

  const id = registeredLanguagePlugins.length;
  const name = alias ?? pluginName;
  registeredLanguagePlugins.push({ plugin, name, keys });
  return { id, name, defaultFiles };
}

/**
 * Validate the visitor keys schema of a language plugin, and get the child field names of each node type.
 *
 * @param schema - `visitorKeys` of the plugin
 * @param pluginName - Name of the plugin, for error messages
 * @returns Child field names, keyed by node type
 * @throws {TypeError} If the schema is not valid
 */
function compileVisitorKeys(
  schema: VisitorKeysSchema,
  pluginName: string,
): Record<string, readonly string[]> {
  const fail = (message: string): never => {
    throw new TypeError(`\`visitorKeys\` of language plugin \`${pluginName}\` ${message}`);
  };

  if (schema === null || typeof schema !== "object") fail("must be an object");
  const { nodes, unions } = schema;
  if (nodes === null || typeof nodes !== "object" || Array.isArray(nodes)) {
    fail("must have a `nodes` object");
  }

  const isTypeName = (value: unknown) => typeof value === "string" && value !== "";

  const keys: Record<string, readonly string[]> = Object.create(null);
  for (const [type, fields] of Object.entries(nodes)) {
    if (fields === null || typeof fields !== "object" || Array.isArray(fields)) {
      fail(`must have an object of child fields for node type \`${type}\``);
    }
    for (const [field, childType] of Object.entries(fields)) {
      const isValid = Array.isArray(childType)
        ? childType.length === 1 && isTypeName(childType[0])
        : isTypeName(childType);
      if (!isValid) {
        fail(
          `has an invalid type for field \`${type}.${field}\`: use a type name, or an array with one type name`,
        );
      }
    }
    keys[type] = Object.freeze(Object.keys(fields));
  }

  if (unions != null) {
    if (typeof unions !== "object" || Array.isArray(unions)) fail("`unions` must be an object");
    for (const [union, members] of Object.entries(unions)) {
      if (!Array.isArray(members) || !members.every(isTypeName)) {
        fail(`must have an array of type names for union \`${union}\``);
      }
    }
  }

  return Object.freeze(keys);
}

/**
 * Lint a file with a language plugin.
 *
 * Not implemented yet: Rust does not call this function yet.
 *
 * @returns Failure, serialized to JSON string
 */
/* oxlint-disable no-unused-vars */
export function lintLanguageFile(
  filePath: string,
  pluginId: number,
  pluginOptionsJSON: string,
  sourceText: string,
  ruleIds: number[],
  optionsIds: number[],
  settingsJSON: string,
  globalsJSON: string,
  collectTimings: boolean,
  workspaceUri: string | null,
): string {
  /* oxlint-enable no-unused-vars */
  return JSON.stringify({
    Failure: { message: "Linting with language plugins is not supported yet" },
  });
}
