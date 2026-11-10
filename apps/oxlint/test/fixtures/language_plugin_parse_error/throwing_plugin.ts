import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

export default defineLanguagePlugin({
  meta: { name: "throwing" },
  defaultFiles: [".vue"],
  visitorKeys: { nodes: { Program: {} } },
  parse(filePath: string, sourceText: string): LanguageNode {
    if (sourceText.includes("<<")) throw new SyntaxError("Unexpected `<<`");
    // Syntax errors with a position, as ESLint parsers throw them: `index`, or 1-based `lineNumber` and `column`
    const index = sourceText.indexOf("{{ (");
    if (index !== -1) {
      throw Object.assign(new SyntaxError("Unexpected token `(`"), {
        index: index + 3,
        lineNumber: 0,
        column: 0,
      });
    }
    const lines = sourceText.split("\n");
    const lineIndex = lines.findIndex((line) => line.includes("{{ ) }}"));
    if (lineIndex !== -1) {
      const column = lines[lineIndex].indexOf(")") + 1;
      throw Object.assign(new SyntaxError("Unexpected token `)`"), {
        lineNumber: lineIndex + 1,
        column,
      });
    }
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
