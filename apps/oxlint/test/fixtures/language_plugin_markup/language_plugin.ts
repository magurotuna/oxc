import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode, MarkupComment, MarkupReference } from "#oxlint/language-plugins";

// A small markup language plugin for tests of `markupReferences` and `markupComments`.
//
// - `<bad>` tags outside `<script>` blocks are `Element` nodes.
// - `@read:name`, `@write:name`, `@read-write:name`, and `@type:name` outside `<script>` blocks
//   are markup references.
// - `<!-- ... -->` outside `<script>` blocks are markup comments. As in eslint-plugin-vue, a block
//   directive in `<template>` stops at the end of `<template>`, and a top-level one stops at the end
//   of the next top-level block.
// - Files whose name contains `NoReferences` have no `markupReferences`.
export default defineLanguagePlugin({
  meta: { name: "markup" },
  defaultFiles: [".vue", ".svelte", ".astro"],
  visitorKeys: {
    nodes: { Program: { children: ["Element"] }, Element: {} },
  },
  parse(filePath: string, sourceText: string): LanguageNode {
    const scripts = [...sourceText.matchAll(/<script[^>]*>[\s\S]*?<\/script>/g)].map(
      (match) => [match.index, match.index + match[0].length] as [number, number],
    );
    const inScript = (offset: number) =>
      scripts.some(([start, end]) => offset >= start && offset < end);
    const template = /<template>[\s\S]*<\/template>/.exec(sourceText);
    const templateRange =
      template === null ? null : [template.index, template.index + template[0].length];
    // Ends of top-level blocks, for `scopeEnd` of top-level comments
    const blockEnds = [...sourceText.matchAll(/<\/(script|template)>/g)].map(
      (match) => match.index + match[0].length,
    );

    const children: LanguageNode[] = [];
    for (const match of sourceText.matchAll(/<bad>/g)) {
      if (inScript(match.index)) continue;
      children.push({ type: "Element", range: [match.index, match.index + match[0].length] });
    }

    const references: MarkupReference[] = [];
    for (const match of sourceText.matchAll(/@(read-write|read|write|type):(\w+)/g)) {
      if (inScript(match.index)) continue;
      const start = match.index + match[1].length + 2;
      references.push({
        name: match[2],
        kind: match[1] as MarkupReference["kind"],
        range: [start, start + match[2].length],
      });
    }

    const comments: MarkupComment[] = [];
    for (const match of sourceText.matchAll(/<!--([\s\S]*?)-->/g)) {
      const start = match.index;
      if (inScript(start)) continue;
      const inTemplate =
        templateRange !== null && start > templateRange[0] && start < templateRange[1];
      const scopeEnd = inTemplate ? templateRange[1] : blockEnds.find((end) => end > start);
      comments.push({ range: [start, start + match[0].length], text: match[1], scopeEnd });
    }

    return {
      type: "Program",
      range: [0, sourceText.length],
      children,
      references,
      comments,
    };
  },
  load(filePath, parseResult) {
    const { references, comments } = parseResult as unknown as {
      references: MarkupReference[];
      comments: MarkupComment[];
    };
    return {
      languageId: "markup",
      ast: parseResult,
      transform: null,
      isESTree: false,
      markupReferences: filePath.includes("NoReferences") ? undefined : references,
      markupComments: comments,
    };
  },
});
