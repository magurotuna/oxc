/*
 * Visitors and traversal for files that a language plugin parsed.
 *
 * The normal visitor (`visitor.ts`) compiles visitors to arrays indexed by the IDs of Oxc's node types.
 * A language plugin's syntax tree has other node types, so this module dispatches by node type names,
 * and matches other selectors with esquery, in the same order as ESLint:
 * selectors with lower specificity first, and the listeners of one selector in the order that rules registered them.
 */

import esquery from "esquery";
// @ts-expect-error - internal module of ESLint with no types
import CodePathAnalyzer from "../../node_modules/eslint/lib/linter/code-path-analysis/code-path-analyzer.js";
import { getChildKeys } from "./language_source_code.ts";
import { wrapTimedFunction } from "./timing.ts";

import type { ESQueryOptions, Selector as EsquerySelector } from "esquery";
import type { Node as EsqueryNode } from "estree";
import type { LanguageNode } from "../package/language_plugin.ts";
import type { RuleTiming } from "./timing.ts";

type Listener = (node: LanguageNode) => void;
type CodePathListener = (...args: unknown[]) => void;

// Code path events of ESLint. They are not node selectors.
const CODE_PATH_EVENTS = new Set([
  "onCodePathStart",
  "onCodePathEnd",
  "onCodePathSegmentStart",
  "onCodePathSegmentEnd",
  "onUnreachableCodePathSegmentStart",
  "onUnreachableCodePathSegmentEnd",
  "onCodePathSegmentLoop",
]);

/**
 * A selector of a visitor, with its listeners.
 */
interface SelectorGroup {
  rawSelector: string;
  selector: EsquerySelector;
  // `true` if the selector is only a node type, so a type check is enough to match it
  isTypeOnly: boolean;
  // Node types that the selector can match, or `null` for any type
  nodeTypes: string[] | null;
  attributeCount: number;
  identifierCount: number;
  listeners: Listener[];
}

const enterGroups = new Map<string, SelectorGroup>();
const exitGroups = new Map<string, SelectorGroup>();

// Listeners of code path events, keyed by event name
const codePathListeners = new Map<string, CodePathListener[]>();

// Sorted groups for each node type, created on first use
const enterGroupsByType = new Map<string, SelectorGroup[]>();
const exitGroupsByType = new Map<string, SelectorGroup[]>();

// Parsed selectors, cached across files
const parsedSelectors = new Map<string, EsquerySelector>();

/**
 * Add the listeners of a rule's visitor.
 *
 * @param visitor - Visitor object that a rule returned
 * @param timing - Timing accumulator of the rule, or `undefined` if timings are not being collected
 * @throws {TypeError} If a value of the visitor is not a function
 */
export function addLanguageVisitor(
  visitor: Record<string, unknown>,
  timing: RuleTiming | undefined,
): void {
  if (visitor === null || typeof visitor !== "object") {
    throw new TypeError("Visitor returned from `create` method must be an object");
  }
  for (const key of Object.keys(visitor)) {
    const listener = visitor[key];
    if (listener == null) continue;
    if (typeof listener !== "function") {
      throw new TypeError(`'${key}' property of visitor object is not a function`);
    }
    if (CODE_PATH_EVENTS.has(key)) {
      let listeners = codePathListeners.get(key);
      if (listeners === undefined) {
        listeners = [];
        codePathListeners.set(key, listeners);
      }
      listeners.push(
        timing === undefined
          ? (listener as CodePathListener)
          : wrapTimedFunction(listener as CodePathListener, timing),
      );
      continue;
    }

    const isExit = key.endsWith(":exit");
    const rawSelector = isExit ? key.slice(0, -":exit".length) : key;
    const groups = isExit ? exitGroups : enterGroups;
    let group = groups.get(rawSelector);
    if (group === undefined) {
      group = createGroup(rawSelector);
      groups.set(rawSelector, group);
    }
    group.listeners.push(
      timing === undefined
        ? (listener as Listener)
        : wrapTimedFunction(listener as Listener, timing),
    );
  }
}

/**
 * Returns `true` if no listeners were added.
 */
export function isLanguageVisitorEmpty(): boolean {
  return enterGroups.size === 0 && exitGroups.size === 0 && codePathListeners.size === 0;
}

/**
 * Remove all listeners, ready for the next file.
 */
export function resetLanguageVisitor(): void {
  enterGroups.clear();
  exitGroups.clear();
  enterGroupsByType.clear();
  exitGroupsByType.clear();
  codePathListeners.clear();
}

function createGroup(rawSelector: string): SelectorGroup {
  let selector = parsedSelectors.get(rawSelector);
  if (selector === undefined) {
    selector = esquery.parse(rawSelector);
    parsedSelectors.set(rawSelector, selector);
  }
  return {
    rawSelector,
    selector,
    isTypeOnly: selector.type === "identifier",
    nodeTypes: getPossibleTypes(selector),
    attributeCount: countAttributes(selector),
    identifierCount: countIdentifiers(selector),
    listeners: [],
  };
}

/**
 * Get the node types that a selector can match, or `null` if it can match any type.
 */
function getPossibleTypes(selector: EsquerySelector): string[] | null {
  switch (selector.type) {
    case "identifier":
      return [selector.value];
    case "matches": {
      const types: string[] = [];
      for (const child of selector.selectors) {
        const childTypes = getPossibleTypes(child);
        if (childTypes === null) return null;
        types.push(...childTypes);
      }
      return types;
    }
    case "compound": {
      let types: string[] | null = null;
      for (const child of selector.selectors) {
        const childTypes = getPossibleTypes(child);
        if (childTypes === null) continue;
        types = types === null ? childTypes : types.filter((type) => childTypes.includes(type));
      }
      return types;
    }
    case "child":
    case "descendant":
    case "sibling":
    case "adjacent":
      return getPossibleTypes(selector.right);
    default:
      return null;
  }
}

function countAttributes(selector: EsquerySelector): number {
  switch (selector.type) {
    case "child":
    case "descendant":
    case "sibling":
    case "adjacent":
      return countAttributes(selector.left) + countAttributes(selector.right);
    case "compound":
    case "not":
    case "matches":
      return selector.selectors.reduce((sum, child) => sum + countAttributes(child), 0);
    case "attribute":
    case "field":
    case "nth-child":
    case "nth-last-child":
      return 1;
    default:
      return 0;
  }
}

function countIdentifiers(selector: EsquerySelector): number {
  switch (selector.type) {
    case "child":
    case "descendant":
    case "sibling":
    case "adjacent":
      return countIdentifiers(selector.left) + countIdentifiers(selector.right);
    case "compound":
    case "not":
    case "matches":
      return selector.selectors.reduce((sum, child) => sum + countIdentifiers(child), 0);
    case "identifier":
      return 1;
    default:
      return 0;
  }
}

function compareGroups(a: SelectorGroup, b: SelectorGroup): number {
  return (
    a.attributeCount - b.attributeCount
    || a.identifierCount - b.identifierCount
    || (a.rawSelector <= b.rawSelector ? -1 : 1)
  );
}

function groupsForType(type: string, isExit: boolean): SelectorGroup[] {
  const cache = isExit ? exitGroupsByType : enterGroupsByType;
  let groups = cache.get(type);
  if (groups === undefined) {
    groups = [];
    for (const group of (isExit ? exitGroups : enterGroups).values()) {
      if (group.nodeTypes === null || group.nodeTypes.includes(type)) groups.push(group);
    }
    groups.sort(compareGroups);
    cache.set(type, groups);
  }
  return groups;
}

// Kinds of steps of a walk
const STEP_ENTER = 0;
const STEP_EXIT = 1;
const STEP_CODE_PATH_EVENT = 2;

/**
 * Traverse the syntax tree and call the listeners.
 *
 * As ESLint does, first set the `parent` property of all nodes and record the steps of the walk,
 * then call the listeners. So all nodes have `parent` before any listener runs.
 * If a rule listens to code path events, ESLint's `CodePathAnalyzer` adds them to the steps,
 * in the same order as in ESLint.
 *
 * @param ast - Root node
 * @param keys - Child field names, keyed by node type
 */
export function walkLanguageAst(ast: LanguageNode, keys: Record<string, readonly string[]>): void {
  const stepKinds: number[] = [];
  const stepData: (LanguageNode | [string, unknown[]])[] = [];

  let enterNode = (node: LanguageNode) => {
    stepKinds.push(STEP_ENTER);
    stepData.push(node);
  };
  let leaveNode = (node: LanguageNode) => {
    stepKinds.push(STEP_EXIT);
    stepData.push(node);
  };
  if (codePathListeners.size !== 0) {
    const analyzer = new CodePathAnalyzer({
      enterNode,
      leaveNode,
      emit(eventName: string, args: unknown[]) {
        stepKinds.push(STEP_CODE_PATH_EVENT);
        stepData.push([eventName, args]);
      },
    });
    enterNode = analyzer.enterNode.bind(analyzer);
    leaveNode = analyzer.leaveNode.bind(analyzer);
  }

  const visit = (node: LanguageNode, parent: LanguageNode | null) => {
    node.parent = parent;
    enterNode(node);
    for (const key of getChildKeys(node, keys)) {
      const child = (node as Record<string, unknown>)[key];
      if (Array.isArray(child)) {
        for (const entry of child) {
          if (isNode(entry)) visit(entry, node);
        }
      } else if (isNode(child)) {
        visit(child, node);
      }
    }
    leaveNode(node);
  };
  visit(ast, (ast.parent as LanguageNode | undefined) ?? null);

  // No `nodeTypeKey`: with it, esquery matches no class selector such as `:function`
  const esqueryOptions: ESQueryOptions = {
    visitorKeys: keys as Record<string, string[]>,
    fallback: (node) => [...getChildKeys(node as unknown as LanguageNode, keys)],
  };

  for (let i = 0, len = stepKinds.length; i < len; i++) {
    const kind = stepKinds[i];
    if (kind === STEP_CODE_PATH_EVENT) {
      const [eventName, args] = stepData[i] as [string, unknown[]];
      const listeners = codePathListeners.get(eventName);
      if (listeners !== undefined) {
        for (const listener of listeners) listener(...args);
      }
      continue;
    }

    const node = stepData[i] as LanguageNode;
    const groups = groupsForType(node.type, kind === STEP_EXIT);
    if (groups.length === 0) continue;
    let ancestry: LanguageNode[] | null = null;
    for (const group of groups) {
      if (!group.isTypeOnly) {
        if (ancestry === null) ancestry = getAncestry(node);
        if (
          !esquery.matches(
            node as unknown as EsqueryNode,
            group.selector,
            ancestry as unknown as EsqueryNode[],
            esqueryOptions,
          )
        ) {
          continue;
        }
      }
      for (const listener of group.listeners) listener(node);
    }
  }
}

function isNode(value: unknown): value is LanguageNode {
  return (
    value !== null
    && typeof value === "object"
    && typeof (value as { type: unknown }).type === "string"
  );
}

/**
 * Get the ancestors of a node, from its parent to the root, as esquery expects.
 */
function getAncestry(node: LanguageNode): LanguageNode[] {
  const ancestry: LanguageNode[] = [];
  for (
    let parent = node.parent as LanguageNode | null | undefined;
    parent;
    parent = parent.parent as LanguageNode | null | undefined
  ) {
    ancestry.push(parent);
  }
  return ancestry;
}
