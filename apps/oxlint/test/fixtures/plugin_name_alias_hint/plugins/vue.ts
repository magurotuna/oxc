import type { Plugin } from "#oxlint/plugins";

const plugin: Plugin = {
  meta: { name: "eslint-plugin-vue" },
  rules: {
    "no-v-html": {
      create() {
        return {};
      },
    },
  },
};

export default plugin;
