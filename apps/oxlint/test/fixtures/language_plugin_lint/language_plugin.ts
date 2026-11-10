import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

// A small markup language plugin for tests.
// It finds opening tags such as `<bad>` anywhere in the file, and `<script>` blocks.
export default defineLanguagePlugin({
  meta: { name: "markup" },
  defaultFiles: [".vue"],
  visitorKeys: {
    nodes: { Program: { children: ["Child"] }, Element: {}, Script: {} },
    unions: { Child: ["Element", "Script"] },
  },
  parse(filePath: string, sourceText: string): LanguageNode {
    const children: LanguageNode[] = [];
    for (const match of sourceText.matchAll(/<(\w+)[^>]*>/g)) {
      const start = match.index;
      const end = start + match[0].length;
      const name = match[1];
      if (name === "template") continue;
      if (name === "script") {
        const close = sourceText.indexOf("</script>", end);
        children.push({ type: "Script", range: [start, close + "</script>".length], name });
      } else {
        children.push({ type: "Element", range: [start, end], name });
      }
    }
    return { type: "Program", range: [0, sourceText.length], children, sourceType: "module" };
  },
  load(filePath, parseResult, sourceText, options) {
    return {
      languageId: "markup",
      ast: parseResult,
      transform: null,
      isESTree: false,
      parserServices: { kind: "markup", options },
    };
  },
});
