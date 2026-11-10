/*
 * `defineLanguagePlugin` function and types of language plugins.
 *
 * A language plugin parses files of another language, such as Vue single-file components,
 * into the syntax tree of that language. JS plugin rules then run on that syntax tree.
 *
 * See RFC https://github.com/oxc-project/oxc/discussions/21936 (language plugins), and
 * RFC https://github.com/oxc-project/oxc/discussions/29142 (`markupReferences` and `markupComments`).
 */

/**
 * Range of a node, token, or comment: offsets of its start and end in the source text,
 * in UTF-16 code units.
 */
export type LanguageRange = [number, number];

/**
 * Line and column of a position. Lines start at 1, columns start at 0.
 */
export interface LanguagePosition {
  line: number;
  column: number;
}

/**
 * Location of a node, token, or comment.
 */
export interface LanguageLocation {
  start: LanguagePosition;
  end: LanguagePosition;
}

/**
 * A node of the syntax tree of a language.
 *
 * `range` is required. `loc` is optional: Oxlint computes it from `range` if it is missing.
 */
export interface LanguageNode {
  type: string;
  range: LanguageRange;
  loc?: LanguageLocation;
  [key: string]: unknown;
}

/**
 * A token of a language.
 */
export interface LanguageToken {
  type: string;
  value: string;
  range: LanguageRange;
  loc?: LanguageLocation;
}

/**
 * A comment of a language.
 */
export interface LanguageComment {
  type: string;
  value: string;
  range: LanguageRange;
  loc?: LanguageLocation;
}

/**
 * Static schema of the syntax tree, used to traverse it.
 *
 * `nodes` lists the child fields of each node type. The value of a field is the node type
 * or union of the child (`"VStartTag"`), or a one-element array for a list of children (`["VNode"]`).
 * `unions` lists the node types of each union.
 *
 * If `LoadedLanguageFile.isESTree` is `true`, the ESTree node types (including TypeScript nodes)
 * are already known, and `nodes` adds node types or replaces the fields of ESTree node types.
 */
export interface VisitorKeysSchema {
  nodes: Record<string, Record<string, string | [string]>>;
  unions?: Record<string, string[]>;
}

/**
 * Transformed virtual JS/TS source of a file, for Rust rules and type-aware linting.
 *
 * Not supported yet: `LoadedLanguageFile.transform` must be `null`.
 */
export interface TransformResult {
  sourceText: string;
  scriptKind: "js" | "jsx" | "ts" | "tsx";
  mappings: LanguageMapping[];
}

/**
 * A mapping between a range of the original file and a range of the virtual source.
 */
export interface LanguageMapping {
  virtualStart: number;
  virtualEnd: number;
  originalStart: number;
  originalEnd: number;
}

/**
 * A reference from the markup of a file to a top-level variable of its script.
 *
 * Oxlint adds these references to its scope analysis of the script, so that rules such as
 * `no-unused-vars` and `prefer-const` see the uses in the markup.
 */
export interface MarkupReference {
  /** Name of the variable. */
  name: string;
  /** How the markup uses the variable. */
  kind: "read" | "write" | "read-write" | "type";
  /** Range of the reference in the file. */
  range: LanguageRange;
}

/**
 * A comment in the markup of a file, for example `<!-- eslint-disable-next-line vue/no-v-html -->`.
 *
 * Oxlint adds the directives in these comments to the same table as the directives in
 * `// eslint-disable` comments in the script.
 */
export interface MarkupComment {
  /** Range of the whole comment in the file, including its delimiters. */
  range: LanguageRange;
  /** Text of the comment, without its delimiters. */
  text: string;
  /**
   * Offset where a block directive in this comment (`eslint-disable` without `-line`) stops applying.
   * If omitted, the directive applies until a matching `eslint-enable` comment, or until the end of the file.
   *
   * For example, a Vue plugin can set this to the end of the `<template>` element, as `eslint-plugin-vue` does.
   */
  scopeEnd?: number;
}

/**
 * Result of loading a file with a language plugin.
 */
export interface LoadedLanguageFile {
  /** ID of the language, for example `"vue"`. Rules see it as `context.languageOptions.language`. */
  languageId: string;
  /**
   * Root node of the syntax tree. Its `range` must cover the whole file.
   * If `ast.tokens` and `ast.comments` are arrays, they are the tokens and comments of the file,
   * as with ESLint parsers.
   */
  ast: LanguageNode;
  /** Tokens of the file. If omitted, `ast.tokens` is used. */
  tokens?: LanguageToken[];
  /** Virtual JS/TS source. Not supported yet: must be `null`. */
  transform: TransformResult | null;
  /** `true` if the syntax tree extends ESTree. */
  isESTree: boolean;
  /** Services for rules, as `context.sourceCode.parserServices`. */
  parserServices?: Record<string, unknown>;
  /**
   * ESLint-compatible scope manager, for `context.sourceCode.scopeManager` and `getScope`.
   * Optional, as RFC #21936 leaves scope analysis to future work.
   */
  scopeManager?: unknown;
  /** References from the markup to top-level script variables. */
  markupReferences?: MarkupReference[];
  /** Comments in the markup. */
  markupComments?: MarkupComment[];
}

/**
 * A language plugin.
 */
export interface LanguagePlugin<ParseResult = unknown, Options = unknown> {
  meta: {
    /** Unique name of the plugin. */
    name: string;
  };
  /**
   * Files that the plugin parses if the config does not set `pattern` for it.
   * Each entry is a file extension (`".vue"`, or `"**\/*.vue"`) or a file name (`"Dockerfile"`).
   * Other globs are not supported: the project config decides which plugin parses a file.
   */
  defaultFiles?: string[];
  /** Static schema of the syntax tree. */
  visitorKeys: VisitorKeysSchema;
  /**
   * Parse a file.
   * @param filePath - Absolute path of the file
   * @param sourceText - Source text of the file
   * @param options - `options` of the `languagePlugins` entry in the config, or `null`
   */
  parse(filePath: string, sourceText: string, options: Options): ParseResult;
  /**
   * Turn the result of `parse` into a `LoadedLanguageFile`.
   * @param filePath - Absolute path of the file
   * @param parseResult - Result of `parse`
   * @param sourceText - Source text of the file
   * @param options - `options` of the `languagePlugins` entry in the config, or `null`
   */
  load(
    filePath: string,
    parseResult: ParseResult,
    sourceText: string,
    options: Options,
  ): LoadedLanguageFile;
}

/**
 * Define a language plugin.
 *
 * No-op function, only to provide type safety. Input is passed through unchanged.
 *
 * @param plugin - Language plugin to define
 * @returns Same plugin as passed in
 */
export function defineLanguagePlugin<ParseResult, Options>(
  plugin: LanguagePlugin<ParseResult, Options>,
): LanguagePlugin<ParseResult, Options> {
  return plugin;
}
