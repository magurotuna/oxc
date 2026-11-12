import type { Plugin } from "#oxlint/plugins";

const plugin: Plugin = {
  meta: { name: "eslint-plugin-foo" },
  rules: {
    "no-var": {
      create(context) {
        return {
          VariableDeclaration(node) {
            if (node.kind === "var") context.report({ message: "Use `let` or `const`", node });
          },
        };
      },
    },
    // Reports each `TODO` in the text, also in `<script>` blocks of files of a language plugin
    "no-todo": {
      create(context) {
        return {
          "Program:exit"() {
            const { sourceCode } = context;
            for (const match of sourceCode.text.matchAll(/TODO/g)) {
              context.report({
                message: "Do not use `TODO`",
                loc: {
                  start: sourceCode.getLocFromIndex(match.index),
                  end: sourceCode.getLocFromIndex(match.index + match[0].length),
                },
              });
            }
          },
        };
      },
    },
    "no-bad-element": {
      create(context) {
        return {
          Element(node: any) {
            context.report({ message: "Do not use `<bad>`", node });
          },
        };
      },
    },
  },
};

export default plugin;
