import type { Plugin, Rule } from "#oxlint/plugins";

const noBadElement: Rule = {
  meta: { fixable: "code" },
  create(context) {
    return {
      Element(node: any) {
        context.report({
          node,
          message: "Do not use `<bad>`",
          fix: (fixer) => fixer.replaceText(node, "<good>"),
        });
      },
    };
  },
};

const plugin: Plugin = {
  meta: { name: "markup-rules" },
  rules: { "no-bad-element": noBadElement },
};

export default plugin;
