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

Found 0 warnings and 6 errors.
Finished in Xms on 2 files with 2 rules using X threads.
```

# stderr
```
```
