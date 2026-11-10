import { parseForESLint } from "@typescript-eslint/parser";
import { defineLanguagePlugin } from "#oxlint/language-plugins";

import type { LanguageNode } from "#oxlint/language-plugins";

// A language plugin that parses the `<script>` block of a file with `@typescript-eslint/parser`,
// at its offsets in the whole file. The text outside the block is replaced with spaces.
export default defineLanguagePlugin({
  meta: { name: "script-only" },
  defaultFiles: [".vue"],
  visitorKeys: { nodes: {} },
  parse(filePath: string, sourceText: string) {
    const match = /<script>([\s\S]*?)<\/script>/.exec(sourceText);
    let code = sourceText.replace(/[^\n]/g, " ");
    if (match !== null) {
      const start = match.index + "<script>".length;
      code = code.slice(0, start) + match[1] + code.slice(start + match[1].length);
    }
    return parseForESLint(code, {
      range: true,
      loc: true,
      tokens: true,
      comment: true,
      sourceType: "module",
      ecmaVersion: "latest",
    });
  },
  load(filePath, result) {
    const { ast, scopeManager } = result as ReturnType<typeof parseForESLint>;
    return {
      languageId: "script-only",
      ast: ast as unknown as LanguageNode,
      transform: null,
      isESTree: true,
      scopeManager,
    };
  },
});
