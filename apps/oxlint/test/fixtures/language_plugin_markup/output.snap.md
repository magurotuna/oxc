# Exit code
1

# stdout
```
  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/Comments.vue:4:3]
 3 |   <bad></bad>
 4 |   <bad></bad>
   :   ^^^^^
 5 |   <!-- eslint-disable -->
   `----

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/Comments.vue:8:1]
 7 | </template>
 8 | <bad></bad>
   : ^^^^^
 9 | <!-- eslint-disable no-debugger -->
   `----

  x eslint(no-debugger): `debugger` statement is not allowed
    ,-[files/Comments.vue:14:1]
 13 | <script setup>
 14 | debugger;
    : ^^^^^^^^^
 15 | // eslint-disable-next-line no-console
    `----
  help: Remove the debugger statement

  ! Unused eslint-disable directive (no problems were reported).
    ,-[files/Comments.vue:15:1]
 14 | debugger;
 15 | // eslint-disable-next-line no-console
    : ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 16 | </script>
    `----

  ! Unused eslint-disable directive (no problems were reported).
    ,-[files/Comments.vue:17:1]
 16 | </script>
 17 | <!-- eslint-disable-next-line no-console -->
    : ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    `----

  x eslint(no-unused-vars): Variable 'unusedInFrontmatter' is declared but never used. Unused variables should start with a '_'.
   ,-[files/Frontmatter.astro:3:7]
 2 | const title = "Title";
 3 | const unusedInFrontmatter = 1;
   :       ^^^^^^^^^|^^^^^^^^^
   :                `-- 'unusedInFrontmatter' is declared here
 4 | ---
   `----
  help: Consider removing this declaration.

  x eslint(no-unused-vars): Variable 'clientVariable' is declared but never used. Unused variables should start with a '_'.
   ,-[files/Frontmatter.astro:8:9]
 7 | <script>
 8 |   const clientVariable = 1;
   :         ^^^^^^^|^^^^^^
   :                `-- 'clientVariable' is declared here
 9 | </script>
   `----
  help: Consider removing this declaration.

  x eslint(no-unused-vars): Variable 'unusedInModule' is declared but never used. Unused variables should start with a '_'.
   ,-[files/Module.svelte:3:9]
 2 |   const fromModule = 1;
 3 |   const unusedInModule = 1;
   :         ^^^^^^^|^^^^^^
   :                `-- 'unusedInModule' is declared here
 4 | </script>
   `----
  help: Consider removing this declaration.

  x eslint(prefer-const): `local` is never reassigned.
   ,-[files/Module.svelte:7:7]
 6 | <script>
 7 |   let local = fromModule;
   :       ^^^^^
 8 | </script>
   `----
  help: Use `const` instead.

  x eslint(no-unused-vars): Variable 'clientOnly' is declared but never used. Unused variables should start with a '_'.
   ,-[files/NoFrontmatter.astro:3:9]
 2 | <script>
 3 |   const clientOnly = 1;
   :         ^^^^^|^^^^
   :              `-- 'clientOnly' is declared here
 4 | </script>
   `----
  help: Consider removing this declaration.

  x eslint(no-unused-vars): Interface 'Unused' is declared but never used.
   ,-[files/References.vue:6:11]
 5 | type Shape = { a: number };
 6 | interface Unused {
   :           ^^^|^^
   :              `-- 'Unused' is declared here
 7 |   b: number;
   `----
  help: Consider removing this declaration.

  x eslint(no-unused-vars): Variable 'written' is declared but never used. Unused variables should start with a '_'.
    ,-[files/References.vue:10:5]
  9 | const shown = 1;
 10 | let written = 0;
    :     ^^^|^^^
    :        `-- 'written' is declared here
 11 | let both = 0;
    `----
  help: Consider removing this declaration.

  x eslint(prefer-const): `neverReassigned` is never reassigned.
    ,-[files/References.vue:12:5]
 11 | let both = 0;
 12 | let neverReassigned = 1;
    :     ^^^^^^^^^^^^^^^
 13 | const unused = 2;
    `----
  help: Use `const` instead.

  x eslint(no-unused-vars): Variable 'unused' is declared but never used. Unused variables should start with a '_'.
    ,-[files/References.vue:13:7]
 12 | let neverReassigned = 1;
 13 | const unused = 2;
    :       ^^^|^^
    :          `-- 'unused' is declared here
 14 | console.log(neverReassigned);
    `----
  help: Consider removing this declaration.

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/TemplateOnly.vue:4:3]
 3 |   <bad></bad>
 4 |   <bad></bad>
   :   ^^^^^
 5 |   <!-- eslint-disable-next-line no-console -->
   `----

  ! Unused eslint-disable directive (no problems were reported).
   ,-[files/TemplateOnly.vue:5:3]
 4 |   <bad></bad>
 5 |   <!-- eslint-disable-next-line no-console -->
   :   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 6 | </template>
   `----

  x eslint(no-unused-vars): Variable 'reassignedInSetup' is declared but never used. Unused variables should start with a '_'.
   ,-[files/TwoScripts.vue:6:5]
 5 | const sharedConstant = 1;
 6 | let reassignedInSetup = 1;
   :     ^^^^^^^^|^^^^^^^^
   :             `-- 'reassignedInSetup' is declared here
 7 | const unusedInScript = 1;
   `----
  help: Consider removing this declaration.

  x eslint(no-unused-vars): Variable 'unusedInScript' is declared but never used. Unused variables should start with a '_'.
   ,-[files/TwoScripts.vue:7:7]
 6 | let reassignedInSetup = 1;
 7 | const unusedInScript = 1;
   :       ^^^^^^^|^^^^^^
   :              `-- 'unusedInScript' is declared here
 8 | </script>
   `----
  help: Consider removing this declaration.

  x eslint(no-unused-vars): Variable 'unusedInSetup' is declared but never used. Unused variables should start with a '_'.
    ,-[files/TwoScripts.vue:12:7]
 11 | reassignedInSetup = 2;
 12 | const unusedInSetup = 1;
    :       ^^^^^^|^^^^^^
    :             `-- 'unusedInSetup' is declared here
 13 | </script>
    `----
  help: Consider removing this declaration.

Found 3 warnings and 16 errors.
Finished in Xms on 8 files with 4 rules using X threads.
```

# stderr
```
```
