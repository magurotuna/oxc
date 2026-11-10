import type { LanguagePlugin } from "#oxlint/language-plugins";

export default {
  meta: { name: "test-language" },
  defaultFiles: [".vue"],
  visitorKeys: { nodes: {} },
  load() {
    throw new Error("unreachable");
  },
} as unknown as LanguagePlugin;
