import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode, MarkupComment } from "#oxlint/language-plugins";

// Returns the HTML comments in the file as markup comments, as a plugin for HTML in template literals can
export default defineLanguagePlugin({
  meta: { name: "markup" },
  defaultFiles: [".js"],
  visitorKeys: { nodes: { Program: {} } },
  parse(filePath: string, sourceText: string): LanguageNode {
    return { type: "Program", range: [0, sourceText.length] };
  },
  load(filePath, parseResult, sourceText) {
    const markupComments: MarkupComment[] = [];
    for (const match of sourceText.matchAll(/<!--([\s\S]*?)-->/g)) {
      markupComments.push({
        range: [match.index, match.index + match[0].length],
        text: match[1],
      });
    }
    return {
      languageId: "markup",
      ast: parseResult,
      transform: null,
      isESTree: false,
      markupComments,
    };
  },
});
