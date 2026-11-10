import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

export default defineLanguagePlugin({
  meta: { name: "throwing" },
  defaultFiles: [".vue"],
  visitorKeys: { nodes: { Program: {} } },
  parse(filePath: string, sourceText: string): LanguageNode {
    if (sourceText.includes("<<")) throw new SyntaxError("Unexpected `<<`");
    return { type: "Program", range: [0, sourceText.length] };
  },
  load(filePath, parseResult) {
    // `transform` must be `null`
    if (filePath.endsWith("Transform.vue")) {
      return { languageId: "x", ast: parseResult, transform: {}, isESTree: false } as never;
    }
    return { languageId: "x", ast: parseResult, transform: null, isESTree: false };
  },
});
