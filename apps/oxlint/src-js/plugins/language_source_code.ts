/*
 * `SourceCode` for files that a language plugin parsed.
 *
 * The normal `SourceCode` (`source_code.ts`) reads the Oxc AST, tokens, and comments from the buffer that Rust sends.
 * A language plugin returns its own syntax tree, tokens, and comments as plain JS objects,
 * so this `SourceCode` works on plain arrays.
 *
 * Its methods follow ESLint's `SourceCode` and `TokenStore`.
 */

import * as locationMethods from "./location.ts";
import { initLines, lines, lineStartIndices } from "./location.ts";
import { sourceText } from "./source_code.ts";
import { debugAssertIsNonNull } from "../utils/asserts.ts";

import type {
  LanguageComment,
  LanguageNode,
  LanguageToken,
  LoadedLanguageFile,
} from "../package/language_plugin.ts";

type Ranged = { range: [number, number] };
type TokenOrComment = LanguageToken | LanguageComment;
type FilterFn = (tokenOrComment: TokenOrComment) => boolean;

/**
 * Options of the token methods: a number (`skip` for single-token methods, `count` for multi-token methods),
 * a filter function, or an object.
 */
type TokenOptions =
  | number
  | FilterFn
  | {
      includeComments?: boolean;
      filter?: FilterFn | null;
      skip?: number;
      count?: number;
    }
  | null
  | undefined;

/**
 * State of the file that a language plugin parsed. `null` when no such file is being linted.
 */
interface LanguageFileState {
  loaded: LoadedLanguageFile;
  // Child field names, keyed by node type
  keys: Record<string, readonly string[]>;
  tokens: LanguageToken[];
  comments: LanguageComment[];
  // Tokens and comments, sorted by position. Created on first use.
  tokensAndComments: TokenOrComment[] | null;
  // Set of comments, to tell comments from tokens. Created on first use.
  commentSet: Set<TokenOrComment> | null;
  parserOptions: Readonly<Record<string, unknown>>;
  // `true` if the file starts with a BOM. The source text that rules see has no BOM.
  hasBOM: boolean;
}

export let languageFile: LanguageFileState | null = null;

/**
 * Set up the file that a language plugin parsed, before rules run on it.
 *
 * @param loaded - Result of the plugin's `load` function
 * @param keys - Child field names of the syntax tree, keyed by node type
 * @param parserOptions - Options of the `languagePlugins` entry
 * @param hasBOM - `true` if the file starts with a BOM
 */
export function setupLanguageFile(
  loaded: LoadedLanguageFile,
  keys: Record<string, readonly string[]>,
  parserOptions: Readonly<Record<string, unknown>>,
  hasBOM: boolean,
): void {
  const { ast } = loaded;
  const tokens = Array.isArray(loaded.tokens)
    ? loaded.tokens
    : Array.isArray(ast.tokens)
      ? (ast.tokens as LanguageToken[])
      : [];
  const comments = Array.isArray(ast.comments) ? (ast.comments as LanguageComment[]) : [];
  languageFile = {
    loaded,
    keys,
    tokens,
    comments,
    tokensAndComments: null,
    commentSet: null,
    parserOptions,
    hasBOM,
  };
}

/**
 * Reset the state of the file that a language plugin parsed.
 */
export function resetLanguageFile(): void {
  languageFile = null;
}

/**
 * Get the child field names of a node.
 * For a node type that the schema does not list, use the properties that contain nodes, as ESLint does.
 *
 * @param node - Node
 * @param keys - Child field names, keyed by node type
 * @returns Child field names
 */
export function getChildKeys(
  node: LanguageNode,
  keys: Record<string, readonly string[]>,
): readonly string[] {
  const nodeKeys = keys[node.type];
  if (nodeKeys !== undefined) return nodeKeys;
  return Object.keys(node).filter(
    (key) =>
      !FALLBACK_SKIPPED_KEYS.has(key)
      && key[0] !== "_"
      && isNodeOrNodeArray((node as Record<string, unknown>)[key]),
  );
}

const FALLBACK_SKIPPED_KEYS = new Set([
  "parent",
  "leadingComments",
  "trailingComments",
  "tokens",
  "comments",
  "loc",
  "range",
]);

function isNode(value: unknown): value is LanguageNode {
  return (
    value !== null
    && typeof value === "object"
    && typeof (value as Ranged & { type: unknown }).type === "string"
  );
}

function isNodeOrNodeArray(value: unknown): boolean {
  return isNode(value) || (Array.isArray(value) && value.some(isNode));
}

function getState(): LanguageFileState {
  debugAssertIsNonNull(languageFile, "No file of a language plugin is being linted");
  return languageFile;
}

function getText(): string {
  debugAssertIsNonNull(sourceText, "`sourceText` should be set for a file of a language plugin");
  return sourceText;
}

function getTokensAndComments(): TokenOrComment[] {
  const state = getState();
  if (state.tokensAndComments === null) {
    state.tokensAndComments = [...state.tokens, ...state.comments].sort(
      (a, b) => a.range[0] - b.range[0],
    );
  }
  return state.tokensAndComments;
}

/**
 * Get the index of the first item whose start is at or after `offset`.
 */
function firstIndexStartingAtOrAfter(items: TokenOrComment[], offset: number): number {
  let low = 0,
    high = items.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (items[mid].range[0] < offset) low = mid + 1;
    else high = mid;
  }
  return low;
}

/**
 * Get the index of the first item whose end is after `offset`.
 */
function firstIndexEndingAfter(items: TokenOrComment[], offset: number): number {
  let low = 0,
    high = items.length;
  while (low < high) {
    const mid = (low + high) >>> 1;
    if (items[mid].range[1] <= offset) low = mid + 1;
    else high = mid;
  }
  return low;
}

interface NormalizedOptions {
  includeComments: boolean;
  filter: FilterFn | null;
  skip: number;
  // Maximum number of tokens, or -1 for no limit. As in ESLint, a given `count` of 0 returns no tokens.
  count: number;
}

/**
 * Normalize the options of a token method.
 * @param options - Options as given
 * @param isMulti - `true` for methods that return many tokens (then a number is `count`, not `skip`)
 */
function normalizeOptions(options: TokenOptions, isMulti: boolean): NormalizedOptions {
  if (typeof options === "number") {
    return isMulti
      ? { includeComments: false, filter: null, skip: 0, count: options }
      : { includeComments: false, filter: null, skip: options, count: -1 };
  }
  if (typeof options === "function") {
    return { includeComments: false, filter: options, skip: 0, count: -1 };
  }
  if (options == null) return { includeComments: false, filter: null, skip: 0, count: -1 };
  return {
    includeComments: options.includeComments === true,
    filter: typeof options.filter === "function" ? options.filter : null,
    skip: typeof options.skip === "number" ? options.skip : 0,
    count: typeof options.count === "number" ? options.count : -1,
  };
}

function itemsFor(options: NormalizedOptions): TokenOrComment[] {
  return options.includeComments ? getTokensAndComments() : getState().tokens;
}

/**
 * Collect tokens from `startIndex` in direction `step`, while `isInside` holds.
 * Applies `filter` and `skip`. Stops after `limit` tokens (-1 = no limit).
 */
function collect(
  items: TokenOrComment[],
  startIndex: number,
  step: 1 | -1,
  isInside: (item: TokenOrComment) => boolean,
  options: NormalizedOptions,
  limit: number,
): TokenOrComment[] {
  const result: TokenOrComment[] = [];
  if (limit === 0) return result;
  let toSkip = options.skip;
  for (let i = startIndex; i >= 0 && i < items.length; i += step) {
    const item = items[i];
    if (!isInside(item)) break;
    if (options.filter !== null && !options.filter(item)) continue;
    if (toSkip > 0) {
      toSkip--;
      continue;
    }
    result.push(item);
    if (limit > 0 && result.length === limit) break;
  }
  return result;
}

const ALWAYS = () => true;

function getFirstToken(node: Ranged, options?: TokenOptions): TokenOrComment | null {
  const opts = normalizeOptions(options, false);
  const items = itemsFor(opts);
  const [start, end] = node.range;
  return (
    collect(
      items,
      firstIndexStartingAtOrAfter(items, start),
      1,
      (item) => item.range[1] <= end,
      opts,
      1,
    )[0] ?? null
  );
}

function getFirstTokens(node: Ranged, options?: TokenOptions): TokenOrComment[] {
  const opts = normalizeOptions(options, true);
  const items = itemsFor(opts);
  const [start, end] = node.range;
  return collect(
    items,
    firstIndexStartingAtOrAfter(items, start),
    1,
    (item) => item.range[1] <= end,
    opts,
    opts.count,
  );
}

function getLastToken(node: Ranged, options?: TokenOptions): TokenOrComment | null {
  const opts = normalizeOptions(options, false);
  const items = itemsFor(opts);
  const [start, end] = node.range;
  return (
    collect(
      items,
      firstIndexEndingAfter(items, end) - 1,
      -1,
      (item) => item.range[0] >= start,
      opts,
      1,
    )[0] ?? null
  );
}

function getLastTokens(node: Ranged, options?: TokenOptions): TokenOrComment[] {
  const opts = normalizeOptions(options, true);
  const items = itemsFor(opts);
  const [start, end] = node.range;
  return collect(
    items,
    firstIndexEndingAfter(items, end) - 1,
    -1,
    (item) => item.range[0] >= start,
    opts,
    opts.count,
  ).reverse();
}

function getTokenBefore(nodeOrToken: Ranged, options?: TokenOptions): TokenOrComment | null {
  const opts = normalizeOptions(options, false);
  const items = itemsFor(opts);
  const start = nodeOrToken.range[0];
  return collect(items, firstIndexEndingAfter(items, start) - 1, -1, ALWAYS, opts, 1)[0] ?? null;
}

function getTokensBefore(nodeOrToken: Ranged, options?: TokenOptions): TokenOrComment[] {
  const opts = normalizeOptions(options, true);
  const items = itemsFor(opts);
  const start = nodeOrToken.range[0];
  return collect(
    items,
    firstIndexEndingAfter(items, start) - 1,
    -1,
    ALWAYS,
    opts,
    opts.count,
  ).reverse();
}

function getTokenAfter(nodeOrToken: Ranged, options?: TokenOptions): TokenOrComment | null {
  const opts = normalizeOptions(options, false);
  const items = itemsFor(opts);
  const end = nodeOrToken.range[1];
  return collect(items, firstIndexStartingAtOrAfter(items, end), 1, ALWAYS, opts, 1)[0] ?? null;
}

function getTokensAfter(nodeOrToken: Ranged, options?: TokenOptions): TokenOrComment[] {
  const opts = normalizeOptions(options, true);
  const items = itemsFor(opts);
  const end = nodeOrToken.range[1];
  return collect(items, firstIndexStartingAtOrAfter(items, end), 1, ALWAYS, opts, opts.count);
}

/**
 * Get the tokens of a node, with optional padding tokens before and after it.
 * Accepts `(node, beforeCount, afterCount)` or `(node, options)`.
 */
function getTokens(
  node: Ranged,
  beforeCountOrOptions?: TokenOptions,
  afterCount?: number,
): TokenOrComment[] {
  if (typeof beforeCountOrOptions === "number" || typeof afterCount === "number") {
    const beforeCount = typeof beforeCountOrOptions === "number" ? beforeCountOrOptions : 0;
    const items = getState().tokens;
    const [start, end] = node.range;
    const first = Math.max(firstIndexStartingAtOrAfter(items, start) - beforeCount, 0);
    const last = Math.min(firstIndexEndingAfter(items, end) + (afterCount ?? 0), items.length);
    return items.slice(first, last);
  }
  const opts = normalizeOptions(beforeCountOrOptions, true);
  const items = itemsFor(opts);
  const [start, end] = node.range;
  return collect(
    items,
    firstIndexStartingAtOrAfter(items, start),
    1,
    (item) => item.range[1] <= end,
    opts,
    opts.count,
  );
}

/**
 * Get the tokens between two nodes or tokens.
 * Accepts `(left, right, padding)` or `(left, right, options)`.
 */
function getTokensBetween(
  left: Ranged,
  right: Ranged,
  paddingOrOptions?: TokenOptions,
): TokenOrComment[] {
  if (typeof paddingOrOptions === "number") {
    const items = getState().tokens;
    const first = Math.max(firstIndexStartingAtOrAfter(items, left.range[1]) - paddingOrOptions, 0);
    const last = Math.min(
      firstIndexEndingAfter(items, right.range[0]) + paddingOrOptions,
      items.length,
    );
    return items.slice(first, last);
  }
  const opts = normalizeOptions(paddingOrOptions, true);
  const items = itemsFor(opts);
  const end = right.range[0];
  return collect(
    items,
    firstIndexStartingAtOrAfter(items, left.range[1]),
    1,
    (item) => item.range[1] <= end,
    opts,
    opts.count,
  );
}

function getFirstTokenBetween(
  left: Ranged,
  right: Ranged,
  options?: TokenOptions,
): TokenOrComment | null {
  const opts = normalizeOptions(options, false);
  const items = itemsFor(opts);
  const end = right.range[0];
  return (
    collect(
      items,
      firstIndexStartingAtOrAfter(items, left.range[1]),
      1,
      (item) => item.range[1] <= end,
      opts,
      1,
    )[0] ?? null
  );
}

function getFirstTokensBetween(
  left: Ranged,
  right: Ranged,
  options?: TokenOptions,
): TokenOrComment[] {
  const opts = normalizeOptions(options, true);
  const items = itemsFor(opts);
  const end = right.range[0];
  return collect(
    items,
    firstIndexStartingAtOrAfter(items, left.range[1]),
    1,
    (item) => item.range[1] <= end,
    opts,
    opts.count,
  );
}

function getLastTokenBetween(
  left: Ranged,
  right: Ranged,
  options?: TokenOptions,
): TokenOrComment | null {
  const opts = normalizeOptions(options, false);
  const items = itemsFor(opts);
  const start = left.range[1];
  return (
    collect(
      items,
      firstIndexEndingAfter(items, right.range[0]) - 1,
      -1,
      (item) => item.range[0] >= start,
      opts,
      1,
    )[0] ?? null
  );
}

function getLastTokensBetween(
  left: Ranged,
  right: Ranged,
  options?: TokenOptions,
): TokenOrComment[] {
  const opts = normalizeOptions(options, true);
  const items = itemsFor(opts);
  const start = left.range[1];
  return collect(
    items,
    firstIndexEndingAfter(items, right.range[0]) - 1,
    -1,
    (item) => item.range[0] >= start,
    opts,
    opts.count,
  ).reverse();
}

function getTokenByRangeStart(
  offset: number,
  options?: { includeComments?: boolean } | null,
): TokenOrComment | null {
  const items = options?.includeComments === true ? getTokensAndComments() : getState().tokens;
  const index = firstIndexStartingAtOrAfter(items, offset);
  const item = items[index];
  return item !== undefined && item.range[0] === offset ? item : null;
}

function getCommentsBefore(nodeOrToken: Ranged): LanguageComment[] {
  const items = getTokensAndComments();
  const comments: LanguageComment[] = [];
  for (let i = firstIndexEndingAfter(items, nodeOrToken.range[0]) - 1; i >= 0; i--) {
    const item = items[i];
    if (!isComment(item)) break;
    comments.push(item);
  }
  return comments.reverse();
}

function getCommentsAfter(nodeOrToken: Ranged): LanguageComment[] {
  const items = getTokensAndComments();
  const comments: LanguageComment[] = [];
  for (let i = firstIndexStartingAtOrAfter(items, nodeOrToken.range[1]); i < items.length; i++) {
    const item = items[i];
    if (!isComment(item)) break;
    comments.push(item);
  }
  return comments;
}

function getCommentsInside(node: Ranged): LanguageComment[] {
  const { comments } = getState();
  const [start, end] = node.range;
  return comments.filter((comment) => comment.range[0] >= start && comment.range[1] <= end);
}

function commentsExistBetween(left: Ranged, right: Ranged): boolean {
  const { comments } = getState();
  const start = left.range[1],
    end = right.range[0];
  return comments.some((comment) => comment.range[0] >= start && comment.range[1] <= end);
}

function isComment(item: TokenOrComment): item is LanguageComment {
  const state = getState();
  if (state.commentSet === null) state.commentSet = new Set(state.comments);
  return state.commentSet.has(item);
}

function isSpaceBetween(first: Ranged, second: Ranged): boolean {
  const [left, right] = first.range[1] <= second.range[0] ? [first, second] : [second, first];
  const text = getText().slice(left.range[1], right.range[0]);
  return /\s/u.test(text.replace(/\/\*.*?\*\//gsu, ""));
}

function getNodeByRangeIndex(index: number): LanguageNode | null {
  const { loaded, keys } = getState();
  const { ast } = loaded;
  if (index < ast.range[0] || index >= ast.range[1]) return null;
  let node = ast;
  while (true) {
    let deeper: LanguageNode | null = null;
    for (const key of getChildKeys(node, keys)) {
      const child = (node as Record<string, unknown>)[key];
      const children = Array.isArray(child) ? child : [child];
      for (const entry of children) {
        if (isNode(entry) && entry.range[0] <= index && index < entry.range[1]) {
          deeper = entry;
          break;
        }
      }
      if (deeper !== null) break;
    }
    if (deeper === null) return node;
    node = deeper;
  }
}

interface ScopeManagerLike {
  scopes: unknown[];
  acquire(node: unknown, inner?: boolean): { type: string; childScopes: unknown[] } | null;
  getDeclaredVariables(node: unknown): unknown[];
}

function getScopeManager(): ScopeManagerLike | null {
  return (getState().loaded.scopeManager as ScopeManagerLike | undefined) ?? null;
}

function requireScopeManager(method: string): ScopeManagerLike {
  const scopeManager = getScopeManager();
  if (scopeManager === null) {
    throw new Error(
      `\`sourceCode.${method}\` needs a scope manager, but the language plugin did not return \`scopeManager\``,
    );
  }
  return scopeManager;
}

function getScope(node: LanguageNode): unknown {
  const scopeManager = requireScopeManager("getScope");
  // Same algorithm as ESLint
  const inner = node.type !== "Program";
  for (
    let current: LanguageNode | null = node;
    current;
    current = (current.parent as LanguageNode) ?? null
  ) {
    const scope = scopeManager.acquire(current, inner);
    if (scope) {
      if (scope.type === "function-expression-name") return scope.childScopes[0];
      return scope;
    }
  }
  return scopeManager.scopes[0];
}

function getDeclaredVariables(node: LanguageNode): unknown[] {
  return requireScopeManager("getDeclaredVariables").getDeclaredVariables(node);
}

interface VariableLike {
  name: string;
  eslintUsed?: boolean;
}

interface ScopeLike {
  type: string;
  block: unknown;
  variables: VariableLike[];
  childScopes: ScopeLike[];
  upper: ScopeLike | null;
}

function markVariableAsUsed(name: string, refNode?: LanguageNode): boolean {
  // Same as ESLint's `SourceCode#markVariableAsUsed`
  const scopeManager = getScopeManager();
  if (scopeManager === null) return false;
  const { ast } = getState().loaded;
  let scope: ScopeLike | null = getScope(refNode ?? ast) as ScopeLike;
  // In a module or a CommonJS file, top-level variables are in the child scope of the global scope
  // whose block is the `Program` node
  if (
    scope.type === "global"
    && scope.childScopes.length !== 0
    && scope.childScopes[0].block === ast
  ) {
    scope = scope.childScopes[0];
  }
  while (scope) {
    const variable = scope.variables.find((v) => v.name === name);
    if (variable) {
      variable.eslintUsed = true;
      return true;
    }
    scope = scope.upper;
  }
  return false;
}

interface GlobalVariableLike {
  defs: unknown[];
  references: { identifier: unknown }[];
}

function isGlobalReference(node: LanguageNode): boolean {
  // Same as ESLint's `SourceCode#isGlobalReference`
  if (!node) throw new TypeError("Missing required argument: `node`");
  if (node.type !== "Identifier") return false;
  const scopeManager = getScopeManager() as {
    scopes: { set: Map<string, GlobalVariableLike> }[];
  } | null;
  if (scopeManager === null || scopeManager.scopes.length === 0) return false;
  const variable = scopeManager.scopes[0].set.get(node.name as string);
  if (variable === undefined || variable.defs.length > 0) return false;
  return variable.references.some(({ identifier }) => identifier === node);
}

// `SourceCode` object for files that a language plugin parsed.
// Only one file is linted at a time, so this single object is reused.
export const LANGUAGE_SOURCE_CODE = Object.freeze({
  get text(): string {
    return getText();
  },
  get hasBOM(): boolean {
    return getState().hasBOM;
  },
  get ast(): LanguageNode {
    return getState().loaded.ast;
  },
  get isESTree(): boolean {
    return getState().loaded.isESTree;
  },
  get scopeManager(): unknown {
    return getScopeManager();
  },
  get visitorKeys(): Readonly<Record<string, readonly string[]>> {
    return getState().keys;
  },
  get parserServices(): Record<string, unknown> {
    return getState().loaded.parserServices ?? {};
  },
  get lines(): string[] {
    if (lines.length === 0) initLines();
    return lines;
  },
  get lineStartIndices(): number[] {
    if (lines.length === 0) initLines();
    return lineStartIndices;
  },
  get tokensAndComments(): TokenOrComment[] {
    return getTokensAndComments();
  },
  getText(node?: Ranged | null, beforeCount?: number | null, afterCount?: number | null): string {
    const text = getText();
    if (!node) return text;
    let [start, end] = node.range;
    if (beforeCount) start = Math.max(start - beforeCount, 0);
    if (afterCount) end += afterCount;
    return text.slice(start, end);
  },
  getAncestors(node: LanguageNode): LanguageNode[] {
    const ancestors: LanguageNode[] = [];
    for (
      let parent = node.parent as LanguageNode | null | undefined;
      parent;
      parent = parent.parent as LanguageNode | null | undefined
    ) {
      ancestors.push(parent);
    }
    return ancestors.reverse();
  },
  getLines(): string[] {
    if (lines.length === 0) initLines();
    return lines;
  },
  getRange(nodeOrToken: Ranged): [number, number] {
    return nodeOrToken.range;
  },
  getLoc(nodeOrToken: Ranged & { loc?: unknown }): unknown {
    return (
      nodeOrToken.loc ?? locationMethods.computeLoc(nodeOrToken.range[0], nodeOrToken.range[1])
    );
  },
  getNodeByRangeIndex,
  getLocFromIndex: locationMethods.getLineColumnFromOffset,
  getIndexFromLoc: locationMethods.getOffsetFromLineColumn,
  getAllComments(): LanguageComment[] {
    return getState().comments;
  },
  getCommentsBefore,
  getCommentsAfter,
  getCommentsInside,
  commentsExistBetween,
  getScope,
  getDeclaredVariables,
  markVariableAsUsed,
  isGlobalReference,
  getTokens,
  getFirstToken,
  getFirstTokens,
  getLastToken,
  getLastTokens,
  getTokenBefore,
  getTokenOrCommentBefore(nodeOrToken: Ranged, skip?: number): TokenOrComment | null {
    return getTokenBefore(nodeOrToken, { includeComments: true, skip });
  },
  getTokensBefore,
  getTokenAfter,
  getTokenOrCommentAfter(nodeOrToken: Ranged, skip?: number): TokenOrComment | null {
    return getTokenAfter(nodeOrToken, { includeComments: true, skip });
  },
  getTokensAfter,
  getTokensBetween,
  getFirstTokenBetween,
  getFirstTokensBetween,
  getLastTokenBetween,
  getLastTokensBetween,
  getTokenByRangeStart,
  isSpaceBetween,
  isSpaceBetweenTokens: isSpaceBetween,
});

export type LanguageSourceCode = typeof LANGUAGE_SOURCE_CODE;
