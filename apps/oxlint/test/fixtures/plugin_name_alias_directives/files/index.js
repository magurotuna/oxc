// The name of the plugin itself
// eslint-disable-next-line foo/no-var
var a = 1;

// The alias
// eslint-disable-next-line foojs/no-var
var b = 2;

// Not disabled
var c = 3;

// The package name of a plugin without `meta.name`
// eslint-disable-next-line bar/no-debugger
debugger;

// Unused, because the next line has no `var`
// eslint-disable-next-line foo/no-var
let d = 4;

// Unused, because the `bar` plugin has no `no-var` rule
// eslint-disable-next-line bar/no-var
var e = 5;

/* eslint-disable foo/no-var */
var f = 6;
/* eslint-enable foo/no-var */
var g = 7;
