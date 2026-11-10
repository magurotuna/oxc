# Exit code
1

# stdout
```
  x source-code(global-references): unresolved: value, offGlobal
   ,-[files/App.vue:1:1]
 1 | ,-> <template></template>
 2 | |   <script>
 3 | |   const topLevel = 1;
 4 | |   if (value === NaN) {
 5 | |   }
 6 | |   customGlobal;
 7 | |   offGlobal;
 8 | `-> </script>
   `----

  x source-code(source-code-info): hasBOM: false; markVariableAsUsed: true, true, false; getFirstTokens(0): 0; getLastTokens({ count: 0 }): 0; getFirstTokens(2): 2; getTokens: 17
   ,-[files/App.vue:1:1]
 1 | ,-> <template></template>
 2 | |   <script>
 3 | |   const topLevel = 1;
 4 | |   if (value === NaN) {
 5 | |   }
 6 | |   customGlobal;
 7 | |   offGlobal;
 8 | `-> </script>
   `----

  x source-code(global-references): global reference: NaN
   ,-[files/App.vue:4:15]
 3 | const topLevel = 1;
 4 | if (value === NaN) {
   :               ^^^
 5 | }
   `----

  x source-code(global-references): global reference: customGlobal
   ,-[files/App.vue:6:1]
 5 | }
 6 | customGlobal;
   : ^^^^^^^^^^^^
 7 | offGlobal;
   `----

  x source-code(global-references): unresolved:
   ,-[files/Bom.vue:1:4]
 1 | ,-> ﻿<script>
 2 | |   const topLevel = 1;
 3 | `-> </script>
   `----

  x source-code(source-code-info): hasBOM: true; markVariableAsUsed: true, true, false; getFirstTokens(0): 0; getLastTokens({ count: 0 }): 0; getFirstTokens(2): 2; getTokens: 5
   ,-[files/Bom.vue:1:4]
 1 | ,-> ﻿<script>
 2 | |   const topLevel = 1;
 3 | `-> </script>
   `----

  x source-code(class-selectors): :function: enter FunctionDeclaration, exit FunctionDeclaration, enter ArrowFunctionExpression, exit ArrowFunctionExpression; :statement: 5; :expression: 11
   ,-[files/Functions.vue:1:1]
 1 | ,-> <template></template>
 2 | |   <script>
 3 | |   async function load(item) {
 4 | |     await item;
 5 | |   }
 6 | |   const run = () => load(1);
 7 | |   run();
 8 | `-> </script>
   `----

  x source-code(global-references): unresolved:
   ,-[files/Functions.vue:1:1]
 1 | ,-> <template></template>
 2 | |   <script>
 3 | |   async function load(item) {
 4 | |     await item;
 5 | |   }
 6 | |   const run = () => load(1);
 7 | |   run();
 8 | `-> </script>
   `----

  x source-code(source-code-info): hasBOM: false; markVariableAsUsed: false, false, false; getFirstTokens(0): 0; getLastTokens({ count: 0 }): 0; getFirstTokens(2): 2; getTokens: 26
   ,-[files/Functions.vue:1:1]
 1 | ,-> <template></template>
 2 | |   <script>
 3 | |   async function load(item) {
 4 | |     await item;
 5 | |   }
 6 | |   const run = () => load(1);
 7 | |   run();
 8 | `-> </script>
   `----

Found 0 warnings and 9 errors.
Finished in Xms on 3 files with 3 rules using X threads.
```

# stderr
```
```
