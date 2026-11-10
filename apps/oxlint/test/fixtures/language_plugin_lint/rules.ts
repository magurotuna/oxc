import type { Plugin, Rule } from "#oxlint/plugins";

const noBadElement: Rule = {
  meta: { fixable: "code" },
  create(context) {
    return {
      "Element[name='bad']"(node: any) {
        context.report({
          node,
          message: "Do not use `<bad>`",
          fix: (fixer) => fixer.replaceText(node, "<good>"),
        });
      },
    };
  },
};

// Reports facts about the file at the end of the walk, to check `context` and the walk order
const fileInfo: Rule = {
  create(context) {
    const events: string[] = [];
    return {
      Program() {
        events.push("enter Program");
      },
      "Program > Element"(node: any) {
        events.push(`enter Element ${node.name} (parent ${node.parent.type})`);
      },
      "Element:exit"(node: any) {
        events.push(`exit Element ${node.name}`);
      },
      // `onCodePathEnd` of `Program` comes after `Program:exit`, as in ESLint
      onCodePathEnd(...args: any[]) {
        const node = args[1];
        context.report({ node, message: `code path end: ${node.type}, events: ${events.length}` });
      },
      "Program:exit"(node: any) {
        const { sourceCode, languageOptions } = context;
        context.report({
          loc: { line: 1, column: 0 },
          message: [
            `ast: ${sourceCode.ast === node}`,
            `text: ${sourceCode.text.length === node.range[1]}`,
            `services: ${JSON.stringify(sourceCode.parserServices)}`,
            `parserOptions: ${JSON.stringify(languageOptions.parserOptions)}`,
            `sourceType: ${languageOptions.sourceType}`,
            `events: ${events.join(", ")}`,
          ].join("; "),
        });
      },
    };
  },
};

const plugin: Plugin = {
  meta: { name: "markup-rules" },
  rules: { "no-bad-element": noBadElement, "file-info": fileInfo },
};

export default plugin;
