import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

// Throws a syntax error with an `index` between the two UTF-16 units of the emoji
export default defineLanguagePlugin({
  meta: { name: "inside-emoji" },
  defaultFiles: [".js"],
  visitorKeys: { nodes: { Program: {} } },
  parse(filePath: string, sourceText: string): LanguageNode {
    throw Object.assign(new SyntaxError("Unexpected character"), {
      index: sourceText.indexOf("💩") + 1,
    });
  },
  load(filePath, parseResult) {
    return { languageId: "x", ast: parseResult, transform: null, isESTree: false };
  },
});
