import type { Plugin } from "#oxlint/plugins";

const plugin: Plugin = {
  meta: { name: "eslint-plugin-foo" },
  rules: {
    rule: {
      create() {
        return {};
      },
    },
  },
};

export default plugin;
