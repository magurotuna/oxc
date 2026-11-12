# Exit code
1

# stdout
```
  ! Unused eslint-disable directive (no problems were reported).
   ,-[files/Markup.vue:6:3]
 5 |   <bad></bad>
 6 |   <!-- eslint-disable-next-line bar/no-bad-element -->
   :   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 7 |   <bad></bad>
   `----

  x foojs(no-bad-element): Do not use `<bad>`
   ,-[files/Markup.vue:7:3]
 6 |   <!-- eslint-disable-next-line bar/no-bad-element -->
 7 |   <bad></bad>
   :   ^^^^^
 8 |   <!-- eslint-disable foo/no-bad-element -->
   `----

  x foojs(no-bad-element): Do not use `<bad>`
    ,-[files/Markup.vue:11:1]
 10 | </template>
 11 | <bad></bad>
    : ^^^^^
 12 | <script>
    `----

  x foojs(no-todo): Do not use `TODO`
    ,-[files/Markup.vue:15:12]
 14 | const a = "TODO";
 15 | const b = "TODO";
    :            ^^^^
 16 | </script>
    `----

  x foojs(no-todo): Do not use `TODO`
    ,-[files/Markup.vue:22:12]
 21 | <script>
 22 | const d = "TODO";
    :            ^^^^
 23 | </script>
    `----

  x foojs(no-var): Use `let` or `const`
    ,-[files/index.js:10:1]
  9 | // Not disabled
 10 | var c = 3;
    : ^^^^^^^^^^
 11 | 
    `----

  ! Unused eslint-disable directive (no problems were reported).
    ,-[files/index.js:17:1]
 16 | // Unused, because the next line has no `var`
 17 | // eslint-disable-next-line foo/no-var
    : ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 18 | let d = 4;
    `----

  ! Unused eslint-disable directive (no problems were reported).
    ,-[files/index.js:21:1]
 20 | // Unused, because the `bar` plugin has no `no-var` rule
 21 | // eslint-disable-next-line bar/no-var
    : ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 22 | var e = 5;
    `----

  x foojs(no-var): Use `let` or `const`
    ,-[files/index.js:22:1]
 21 | // eslint-disable-next-line bar/no-var
 22 | var e = 5;
    : ^^^^^^^^^^
 23 | 
    `----

  x foojs(no-var): Use `let` or `const`
    ,-[files/index.js:27:1]
 26 | /* eslint-enable foo/no-var */
 27 | var g = 7;
    : ^^^^^^^^^^
    `----

Found 3 warnings and 7 errors.
Finished in Xms on 2 files with 4 rules using X threads.
```

# stderr
```
```
