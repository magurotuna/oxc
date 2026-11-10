# Exit code
1

# stdout
```
  x Error running language plugin `throwing`.
  | File path: <fixture>/files/Broken.vue
  | SyntaxError: Unexpected `<<`
  |     at Object.parse (<fixture>/throwing_plugin.ts:10:42)

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/Broken.vue:3:1]
 2 | <script>
 3 | debugger;
   : ^^^^^^^^^
 4 | </script>
   `----
  help: Remove the debugger statement

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/Fine.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

  x Parsing error: Unexpected character
   ,-[files/InsideEmoji.vue:2:6]
 1 | <template>
 2 |   <p>💩</p>
   :      ^
 3 | </template>
   `----
  help: Language plugin `throwing` could not parse this file.

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/InsideEmoji.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

  x Parsing error: Unexpected token `)`
   ,-[files/LineColumn.vue:2:14]
 1 | <template>
 2 |   <p>😀 {{ ) }}</p>
   :            ^
 3 | </template>
   `----
  help: Language plugin `throwing` could not parse this file.

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/LineColumn.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

  x Parsing error: Unexpected token `(`
   ,-[files/Positioned.vue:2:19]
 1 | <template>
 2 |   <p>日本語 {{ ( }}</p>
   :                ^
 3 | </template>
   `----
  help: Language plugin `throwing` could not parse this file.

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/Positioned.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

  x Error running language plugin `throwing`.
  | File path: <fixture>/files/Transform.vue
  | TypeError: `load` of language plugin `throwing` must return `transform: null`: transforms are not supported yet

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/Transform.vue:3:1]
 2 | <script>
 3 | debugger;
   : ^^^^^^^^^
 4 | </script>
   `----
  help: Remove the debugger statement

Found 0 warnings and 11 errors.
Finished in Xms on 6 files with 2 rules using X threads.
```

# stderr
```
```
