# Exit code
1

# stdout
```
  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/NotParsed.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

  x markup-rules(no-bad-element): Do not use `<bad>`
   ,-[files/src/Parsed.vue:2:3]
 1 | <template>
 2 |   <bad></bad>
   :   ^^^^^
 3 | </template>
   `----

  x eslint(no-debugger): `debugger` statement is not allowed
   ,-[files/src/Parsed.vue:5:1]
 4 | <script>
 5 | debugger;
   : ^^^^^^^^^
 6 | </script>
   `----
  help: Remove the debugger statement

Found 0 warnings and 3 errors.
Finished in Xms on 2 files with 2 rules using X threads.
```

# stderr
```
```
