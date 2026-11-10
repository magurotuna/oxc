# Exit code
1

# stdout
```
  ! Unused eslint-disable directive (no problems were reported).
   ,-[files/App.vue:3:3]
 2 |   <p>@read:used</p>
 3 |   <!-- eslint-disable-next-line no-alert -->
   :   ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
 4 | </template>
   `----

  x eslint(no-unused-vars): Variable 'unused' is declared but never used. Unused variables should start with a '_'.
   ,-[files/App.vue:8:7]
 7 | const used = 1;
 8 | const unused = 2;
   :       ^^^|^^
   :          `-- 'unused' is declared here
 9 | debugger;
   `----
  help: Consider removing this declaration.

Found 1 warning and 1 error.
Finished in Xms on 1 file with 2 rules using X threads.
```

# stderr
```
```
