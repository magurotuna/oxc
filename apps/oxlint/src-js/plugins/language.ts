/*
 * Language plugins: loading.
 *
 * A language plugin parses files of another language (for example, Vue single-file components)
 * into the syntax tree of that language. JS plugin rules then run on that syntax tree.
 */

import { setupFileContext } from "./context.ts";
import { afterHooks, resetFile, resetStateAfterError, runAfterHooks } from "./lint.ts";
import { languageFile, setupLanguageFile } from "./language_source_code.ts";
import {
  addLanguageVisitor,
  isLanguageVisitorEmpty,
  resetLanguageVisitor,
  walkLanguageAst,
} from "./language_visitor.ts";
import { registeredRules } from "./load.ts";
import { allOptions, DEFAULT_OPTIONS_ID } from "./options.ts";
import { diagnostics } from "./report.ts";
import { setSettingsForFile } from "./settings.ts";
import { setupSourceTextForLanguageFile } from "./source_code.ts";
import { setGlobalsForFile } from "./globals.ts";
import { timeCall, wrapTimedFunction } from "./timing.ts";
import { switchWorkspace } from "./workspace.ts";
import baseVisitorKeys from "../generated/keys.ts";
import { debugAssert, debugAssertIsNonNull } from "../utils/asserts.ts";
import { getErrorMessage } from "../utils/utils.ts";

import type {
  LanguageNode,
  LanguagePlugin,
  LoadedLanguageFile,
  VisitorKeysSchema,
} from "../package/language_plugin.ts";
import type { RuleTiming } from "./timing.ts";

/**
 * A loaded language plugin.
 */
export interface LanguagePluginDetails {
  plugin: LanguagePlugin;
  name: string;
  // Child field names of each node type in the plugin's schema
  keys: Record<string, readonly string[]>;
  // `keys` merged over the child field names of Oxc's ESTree nodes, for syntax trees with `isESTree: true`.
  // Created on first use.
  estreeKeys: Record<string, readonly string[]> | null;
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
  registeredLanguagePlugins.push({ plugin, name, keys, estreeKeys: null });
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
 * Reference from markup to a variable, in the form sent to Rust (UTF-16 offsets).
 */
interface MarkupReferenceReport {
  name: string;
  kind: "read" | "write" | "read-write" | "type";
  start: number;
  end: number;
}

/**
 * Comment in markup, in the form sent to Rust (UTF-16 offsets).
 */
interface MarkupCommentReport {
  start: number;
  end: number;
  text: string;
  scopeEnd: number | null;
}

const MARKUP_REFERENCE_KINDS = new Set(["read", "write", "read-write", "type"]);

// Reusable property descriptor for updating `options` value on rule context objects
const OPTIONS_DESCRIPTOR: PropertyDescriptor = { value: null };

/**
 * Lint a file with a language plugin.
 *
 * Parse the file with the plugin, load the result, and run JS plugin rules on the plugin's syntax tree.
 *
 * @param filePath - Absolute path of file being linted
 * @param pluginId - ID of the language plugin
 * @param pluginOptionsJSON - Options of the `languagePlugins` entry, as JSON string (`null` if none)
 * @param sourceText - Source text of the file, without a BOM
 * @param ruleIds - IDs of rules to run on this file
 * @param optionsIds - IDs of options to use for rules on this file, in same order as `ruleIds`
 * @param settingsJSON - Settings for this file, as JSON string
 * @param globalsJSON - Globals for this file, as JSON string
 * @param collectTimings - Whether to collect per-rule timing information
 * @param workspaceUri - Workspace URI (`null` in CLI, string in LSP)
 * @returns Diagnostics, markup references, markup comments, and optional timings, or an error, serialized to JSON
 */
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
  const start = collectTimings ? performance.now() : 0;
  const timings: RuleTiming[] | null = collectTimings ? [] : null;

  try {
    const { markupReferences, markupComments } = lintLanguageFileImpl(
      filePath,
      pluginId,
      pluginOptionsJSON,
      sourceText,
      ruleIds,
      optionsIds,
      settingsJSON,
      globalsJSON,
      workspaceUri,
      timings,
    );

    const output = {
      diagnostics,
      markupReferences,
      markupComments,
      timings:
        timings === null ? undefined : { rules: timings, runtimeMs: performance.now() - start },
    };
    const ret = JSON.stringify({ Success: output });

    diagnostics.length = 0;
    resetLanguageVisitor();
    resetFile();

    return ret;
  } catch (err) {
    const timingOutput =
      timings === null ? undefined : { rules: timings, runtimeMs: performance.now() - start };
    const message = getErrorMessage(err);

    resetStateAfterError();

    return JSON.stringify({ Failure: { message, timings: timingOutput } });
  }
}

/**
 * Parse and load a file with a language plugin, and run rules on it.
 *
 * @returns Markup references and markup comments of the file
 * @throws {*} If the plugin or any rule throws, or the plugin returns invalid values
 */
function lintLanguageFileImpl(
  filePath: string,
  pluginId: number,
  pluginOptionsJSON: string,
  sourceText: string,
  ruleIds: number[],
  optionsIds: number[],
  settingsJSON: string,
  globalsJSON: string,
  workspaceUri: string | null,
  timings: RuleTiming[] | null,
): { markupReferences: MarkupReferenceReport[] | null; markupComments: MarkupCommentReport[] } {
  debugAssert(
    ruleIds.length === optionsIds.length,
    "`ruleIds` and `optionsIds` should be same length",
  );

  if (workspaceUri !== null) switchWorkspace(workspaceUri);

  debugAssert(pluginId < registeredLanguagePlugins.length, "Language plugin ID out of bounds");
  const details = registeredLanguagePlugins[pluginId];
  const { plugin, name } = details;

  // Options of the `languagePlugins` entry. Each file gets a new object, so a plugin cannot change the options
  // of later files.
  const options = (JSON.parse(pluginOptionsJSON) ?? {}) as Record<string, unknown>;

  const loaded = parseAndLoad(plugin, name, filePath, sourceText, options);
  const markupReferences = convertMarkupReferences(loaded, name);
  const markupComments = convertMarkupComments(loaded, name);

  let { keys } = details;
  if (loaded.isESTree) {
    if (details.estreeKeys === null) {
      details.estreeKeys = Object.freeze({ ...baseVisitorKeys, ...details.keys });
    }
    keys = details.estreeKeys;
  }

  // Rules see the file's own text and syntax tree
  setupFileContext(filePath);
  setupSourceTextForLanguageFile(sourceText);
  setupLanguageFile(loaded, keys, Object.freeze(options));
  setSettingsForFile(settingsJSON);
  setGlobalsForFile(globalsJSON);

  if (ruleIds.length === 0) return { markupReferences, markupComments };

  debugAssertIsNonNull(allOptions, "`allOptions` should be initialized");

  // The order rules run in is indeterminate.
  // To make order predictable in tests, in debug builds, sort rules by ID in ascending order.
  let ruleIndexes: number[] | undefined;
  if (DEBUG) {
    const rules = ruleIds.map((ruleId, index) => ({ ruleId, optionsId: optionsIds[index], index }));
    rules.sort((rule1, rule2) => rule1.ruleId - rule2.ruleId);
    ruleIds = rules.map((rule) => rule.ruleId);
    optionsIds = rules.map((rule) => rule.optionsId);
    ruleIndexes = rules.map((rule) => rule.index);
  }

  // Call `create` of all rules before walking the syntax tree.
  // Some rules depend on this. For example, `defineTemplateBodyVisitor` of `vue-eslint-parser` walks the template
  // at `Program:exit` of the first rule that calls it, for the template visitors of all rules.
  for (let i = 0, len = ruleIds.length; i < len; i++) {
    const ruleId = ruleIds[i];
    debugAssert(ruleId < registeredRules.length, "Rule ID out of bounds");
    const ruleDetails = registeredRules[ruleId];

    const ruleIndex = DEBUG ? ruleIndexes![i] : i;
    ruleDetails.ruleIndex = ruleIndex;
    let timing: RuleTiming | undefined;
    if (timings !== null) {
      timing = { ruleIndex, durationMs: 0, calls: 0 };
      timings.push(timing);
    }

    const optionsId = optionsIds[i];
    debugAssert(optionsId < allOptions.length, "Options ID out of bounds");
    OPTIONS_DESCRIPTOR.value =
      optionsId === DEFAULT_OPTIONS_ID ? ruleDetails.defaultOptions : allOptions[optionsId];
    Object.defineProperty(ruleDetails.context, "options", OPTIONS_DESCRIPTOR);

    let visitor: unknown = ruleDetails.visitor;
    if (visitor === null) {
      // Rule defined with `create` method
      debugAssertIsNonNull(ruleDetails.rule.create);
      visitor =
        timing === undefined
          ? ruleDetails.rule.create(ruleDetails.context)
          : timeCall(timing, () => ruleDetails.rule.create!(ruleDetails.context));
    } else {
      // Rule defined with `createOnce` method
      const { beforeHook, afterHook } = ruleDetails;
      if (beforeHook !== null) {
        const shouldRun = timing === undefined ? beforeHook() : timeCall(timing, beforeHook);
        if (shouldRun === false) continue;
      }
      if (afterHook !== null) {
        afterHooks.push(timing === undefined ? afterHook : wrapTimedFunction(afterHook, timing));
      }
    }

    addLanguageVisitor(visitor as Record<string, unknown>, timing);
  }

  if (!isLanguageVisitorEmpty()) {
    debugAssertIsNonNull(languageFile);
    walkLanguageAst(loaded.ast, keys);
  }

  runAfterHooks(true);

  return { markupReferences, markupComments };
}

/**
 * Call the `parse` and `load` functions of a language plugin, and check the result of `load`.
 *
 * @throws {Error|TypeError} If the plugin throws or returns an invalid value
 */
function parseAndLoad(
  plugin: LanguagePlugin,
  pluginName: string,
  filePath: string,
  sourceText: string,
  options: Record<string, unknown>,
): LoadedLanguageFile {
  const parseResult: unknown = plugin.parse(filePath, sourceText, options);
  if (isPromise(parseResult)) {
    throw new TypeError(`\`parse\` of language plugin \`${pluginName}\` must not return a Promise`);
  }

  const loaded: unknown = plugin.load(filePath, parseResult, sourceText, options);
  if (isPromise(loaded)) {
    throw new TypeError(`\`load\` of language plugin \`${pluginName}\` must not return a Promise`);
  }

  const fail = (message: string): never => {
    throw new TypeError(`\`load\` of language plugin \`${pluginName}\` ${message}`);
  };
  if (loaded === null || typeof loaded !== "object") fail("must return an object");
  const file = loaded as LoadedLanguageFile;

  if (typeof file.languageId !== "string" || file.languageId === "") {
    fail("must return a non-empty string `languageId`");
  }
  if (!isRangedNode(file.ast)) {
    fail("must return an `ast` object with a string `type` and a `range`");
  }
  if (file.transform != null) {
    fail("must return `transform: null`: transforms are not supported yet");
  }
  if (typeof file.isESTree !== "boolean") fail("must return a boolean `isESTree`");
  if (file.parserServices != null && typeof file.parserServices !== "object") {
    fail("must return an object or `undefined` for `parserServices`");
  }

  return file;
}

/**
 * Convert `markupReferences` of a loaded file to the form sent to Rust.
 * Returns `null` if the plugin does not return `markupReferences`.
 *
 * @throws {TypeError} If `markupReferences` is invalid
 */
function convertMarkupReferences(
  loaded: LoadedLanguageFile,
  pluginName: string,
): MarkupReferenceReport[] | null {
  const { markupReferences } = loaded;
  // `null` tells Rust that the plugin does not know the markup's references
  if (markupReferences == null) return null;
  const fail = (message: string): never => {
    throw new TypeError(`\`markupReferences\` of language plugin \`${pluginName}\` ${message}`);
  };
  if (!Array.isArray(markupReferences)) fail("must be an array");

  return markupReferences.map((reference) => {
    if (reference === null || typeof reference !== "object") fail("must contain objects");
    const { name, kind, range } = reference;
    if (typeof name !== "string" || name === "") fail("must have a non-empty string `name`");
    if (!MARKUP_REFERENCE_KINDS.has(kind)) {
      fail("must have a `kind` of `read`, `write`, `read-write`, or `type`");
    }
    if (!isRange(range)) fail("must have a `range` of 2 non-negative integers");
    return { name, kind, start: range[0], end: range[1] };
  });
}

/**
 * Convert `markupComments` of a loaded file to the form sent to Rust.
 *
 * @throws {TypeError} If `markupComments` is invalid
 */
function convertMarkupComments(
  loaded: LoadedLanguageFile,
  pluginName: string,
): MarkupCommentReport[] {
  const { markupComments } = loaded;
  if (markupComments == null) return [];
  const fail = (message: string): never => {
    throw new TypeError(`\`markupComments\` of language plugin \`${pluginName}\` ${message}`);
  };
  if (!Array.isArray(markupComments)) fail("must be an array");

  return markupComments.map((comment) => {
    if (comment === null || typeof comment !== "object") fail("must contain objects");
    const { range, text, scopeEnd } = comment;
    if (!isRange(range)) fail("must have a `range` of 2 non-negative integers");
    if (typeof text !== "string") fail("must have a string `text`");
    if (scopeEnd != null && !isOffset(scopeEnd)) {
      fail("must have a non-negative integer or `undefined` for `scopeEnd`");
    }
    return { start: range[0], end: range[1], text, scopeEnd: scopeEnd ?? null };
  });
}

function isPromise(value: unknown): boolean {
  return (
    value !== null
    && typeof value === "object"
    && typeof (value as { then?: unknown }).then === "function"
  );
}

function isOffset(value: unknown): value is number {
  return typeof value === "number" && value >= 0 && (value | 0) === value;
}

function isRange(value: unknown): value is [number, number] {
  return (
    Array.isArray(value)
    && value.length === 2
    && isOffset(value[0])
    && isOffset(value[1])
    && value[0] <= value[1]
  );
}

function isRangedNode(value: unknown): value is LanguageNode {
  return (
    value !== null
    && typeof value === "object"
    && typeof (value as { type?: unknown }).type === "string"
    && isRange((value as { range?: unknown }).range)
  );
}
