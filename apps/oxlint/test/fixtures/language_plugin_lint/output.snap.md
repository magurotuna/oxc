# Exit code
1

# stdout
```
  ! markup-rules(file-info): ast: true; text: true; services: {"kind":"markup","options":{"flag":true}}; parserOptions: {"flag":true}; sourceType: module; events: enter Program, enter Element p
  | (parent Program), exit Element p, enter Element bad (parent Program), exit Element bad, enter Element bad (parent Program), exit Element bad, enter Element bad (parent Program), exit Element bad
   ,-[files/App.vue:1:1]
 1 | <template>
   : ^
 2 |   <p>日本語 😀</p>
   `----

  ! markup-rules(file-info): code path end: Program, events: 9
    ,-[files/App.vue:1:1]
  1 | ,-> <template>
  2 | |     <p>日本語 😀</p>
  3 | |     <bad></bad>
  4 | |   </template>
  5 | |   <script>
  6 | |   // eslint-disable-next-line markup-rules/no-bad-element
  7 | |   const a = "<bad>";
  8 | |   const b = "<bad>";
  9 | |   debugger;
 10 | `-> </script>
    `----

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/App.vue:3:3]
 2 |   <p>日本語 😀</p>
 3 |   <bad></bad>
   :   ^^^^^
 4 | </template>
   `----

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/App.vue:8:12]
 7 | const a = "<bad>";
 8 | const b = "<bad>";
   :            ^^^^^
 9 | debugger;
   `----

  x eslint(no-debugger): `debugger` statement is not allowed
    ,-[files/App.vue:9:1]
  8 | const b = "<bad>";
  9 | debugger;
    : ^^^^^^^^^
 10 | </script>
    `----
  help: Remove the debugger statement

  ! markup-rules(file-info): ast: true; text: true; services: {"kind":"markup","options":{"flag":true}}; parserOptions: {"flag":true}; sourceType: module; events: enter Program, enter Element bad
  | (parent Program), exit Element bad
   ,-[files/Bom.vue:1:4]
 1 | ﻿<template>
   : ^
 2 |   <bad></bad>
   `----

  ! markup-rules(file-info): code path end: Program, events: 3
   ,-[files/Bom.vue:1:4]
 1 | ,-> ﻿<template>
 2 | |     <bad></bad>
 3 | |   </template>
 4 | |   <script>
 5 | |   debugger;
 6 | `-> </script>
   `----

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/Bom.vue:2:3]
 1 | ﻿<template>
 2 |   <bad></bad>
   :   ^^^^^
 3 | </template>
   `----

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/Bom.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

  ! markup-rules(file-info): ast: true; text: true; services: {"kind":"markup","options":{"flag":true}}; parserOptions: {"flag":true}; sourceType: module; events: enter Program, enter Element bad
  | (parent Program), exit Element bad
   ,-[files/TemplateOnly.vue:1:1]
 1 | <template>
   : ^
 2 |   <bad></bad>
   `----

  ! markup-rules(file-info): code path end: Program, events: 3
   ,-[files/TemplateOnly.vue:1:1]
 1 | ,-> <template>
 2 | |     <bad></bad>
 3 | `-> </template>
   `----

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/TemplateOnly.vue:2:3]
 1 | <template>
 2 |   <bad></bad>
   :   ^^^^^
 3 | </template>
   `----

  ! markup-rules(file-info): ast: true; text: true; services: {}; parserOptions: {"sourceType":"script","ecmaFeatures":{"jsx":true,"globalReturn":false,"impliedStrict":false}}; sourceType: script;
  | events: enter Program
   ,-[files/index.js:1:1]
 1 | debugger;
   : ^
 2 | const c = "<bad>";
   `----

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/index.js:1:1]
 1 | debugger;
   : ^^^^^^^^^
 2 | const c = "<bad>";
   `----
  help: Remove the debugger statement

  ! markup-rules(file-info): code path end: Program, events: 1
   ,-[files/index.js:1:1]
 1 | ,-> debugger;
 2 | `-> const c = "<bad>";
   `----

Found 8 warnings and 7 errors.
Finished in Xms on 4 files with 3 rules using X threads.
```

# stderr
```
```
