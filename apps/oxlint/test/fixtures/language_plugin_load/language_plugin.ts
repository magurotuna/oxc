import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

export default defineLanguagePlugin({
  meta: { name: "test-language" },
  defaultFiles: [".vue", "**/*.svelte"],
  visitorKeys: { nodes: { Root: { children: ["Node"] } }, unions: { Node: ["Root"] } },
  parse(filePath: string, sourceText: string): LanguageNode {
    return { type: "Root", range: [0, sourceText.length], children: [] };
  },
  load(filePath, parseResult) {
    return { languageId: "test", ast: parseResult, transform: null, isESTree: false };
  },
});
