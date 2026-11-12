import type { Plugin } from "#oxlint/plugins";

const plugin: Plugin = {
  meta: { name: "eslint-plugin-foo" },
  rules: {
    "no-debugger": {
      create(context) {
        return {
          DebuggerStatement(node) {
            context.report({ message: "Do not use `debugger`", node });
          },
        };
      },
    },
  },
};

export default plugin;
