import type { Plugin, Rule } from "#oxlint/plugins";

// Reports identifiers that are references to globals (`sourceCode.isGlobalReference`), and the
// references that resolve to no variable (as `no-undef` sees them).
const globalReferences: Rule = {
  create(context) {
    const { sourceCode } = context;
    return {
      Identifier(node: any) {
        if (sourceCode.isGlobalReference(node)) {
          context.report({ node, message: `global reference: ${node.name}` });
        }
      },
      "Program:exit"(node: any) {
        const through = (sourceCode.scopeManager as any).globalScope.through.map(
          (reference: any) => reference.identifier.name,
        );
        context.report({ node, message: `unresolved: ${through.join(", ")}` });
      },
    };
  },
};

// Reports facts about `SourceCode` methods
const sourceCodeInfo: Rule = {
  create(context) {
    const { sourceCode } = context;
    return {
      Program(node: any) {
        const markedWithoutNode = sourceCode.markVariableAsUsed("topLevel");
        const markedWithProgram = sourceCode.markVariableAsUsed("topLevel", node);
        const markedMissing = sourceCode.markVariableAsUsed("missing", node);
        context.report({
          node,
          message: [
            `hasBOM: ${sourceCode.hasBOM}`,
            `markVariableAsUsed: ${markedWithoutNode}, ${markedWithProgram}, ${markedMissing}`,
            `getFirstTokens(0): ${sourceCode.getFirstTokens(node, 0).length}`,
            `getLastTokens({ count: 0 }): ${sourceCode.getLastTokens(node, { count: 0 }).length}`,
            `getFirstTokens(2): ${sourceCode.getFirstTokens(node, 2).length}`,
            `getTokens: ${sourceCode.getTokens(node).length}`,
          ].join("; "),
        });
      },
    };
  },
};

const plugin: Plugin = {
  meta: { name: "source-code" },
  rules: { "global-references": globalReferences, "source-code-info": sourceCodeInfo },
};

export default plugin;
