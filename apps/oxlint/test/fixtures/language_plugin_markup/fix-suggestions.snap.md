# Exit code
1

# stdout
```
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

  ! Unused eslint-disable directive (no problems were reported).
   ,-[files/TemplateOnly.vue:5:3]
 4 |   <bad></bad>
 5 |   <!-- eslint-disable-next-line no-console -->
   :   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 6 | </template>
   `----

Found 2 warnings and 7 errors.
Finished in Xms on 6 files with 4 rules using X threads.
```

# stderr
```
```

# File altered: files/Comments.vue
```
<template>
  <!-- eslint-disable-next-line markup-rules/no-bad-element -->
  <bad></bad>
  <good></bad>
  <!-- eslint-disable -->
  <bad></bad>
</template>
<good></bad>
<!-- eslint-disable no-debugger -->
<script>
debugger;
</script>
<script setup>

</script>
<!-- eslint-disable-next-line no-console -->

```

# File altered: files/TemplateOnly.vue
```
<template>
  <!-- eslint-disable-next-line markup-rules/no-bad-element -->
  <bad></bad>
  <good></bad>
  <!-- eslint-disable-next-line no-console -->
</template>

```
