import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode, MarkupComment, MarkupReference } from "#oxlint/language-plugins";

// A language plugin in a config without JS plugins. It returns markup references (`@read:name`)
// and markup comments (`<!-- ... -->`) outside `<script>` blocks.
export default defineLanguagePlugin({
  meta: { name: "markup" },
  defaultFiles: [".vue"],
  visitorKeys: { nodes: { Program: {} } },
  parse(filePath: string, sourceText: string): LanguageNode {
    return { type: "Program", range: [0, sourceText.length] };
  },
  load(filePath, ast, sourceText) {
    const scriptStart = sourceText.indexOf("<script");
    const markup = (index: number) => index < scriptStart;
    const markupReferences: MarkupReference[] = [];
    for (const match of sourceText.matchAll(/@read:(\w+)/g)) {
      if (!markup(match.index)) continue;
      const start = match.index + "@read:".length;
      markupReferences.push({
        name: match[1],
        kind: "read",
        range: [start, start + match[1].length],
      });
    }
    const markupComments: MarkupComment[] = [];
    for (const match of sourceText.matchAll(/<!--([\s\S]*?)-->/g)) {
      if (!markup(match.index)) continue;
      markupComments.push({
        range: [match.index, match.index + match[0].length],
        text: match[1],
      });
    }
    return {
      languageId: "markup",
      ast,
      transform: null,
      isESTree: false,
      markupReferences,
      markupComments,
    };
  },
});
