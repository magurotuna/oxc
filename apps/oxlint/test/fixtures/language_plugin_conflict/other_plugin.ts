import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

export default defineLanguagePlugin({
  meta: { name: "other" },
  defaultFiles: [".vue"],
  visitorKeys: { nodes: { Program: {} } },
  parse(filePath: string, sourceText: string): LanguageNode {
    return { type: "Program", range: [0, sourceText.length] };
  },
  load(filePath, parseResult) {
    return { languageId: "other", ast: parseResult, transform: null, isESTree: false };
  },
});
