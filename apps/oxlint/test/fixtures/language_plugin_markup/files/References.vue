<template>
  <p>@read:shown @write:written @read-write:both @type:Shape</p>
</template>
<script setup lang="ts">
type Shape = { a: number };
interface Unused {
  b: number;
}
const shown = 1;
let written = 0;
let both = 0;
let neverReassigned = 1;
const unused = 2;
console.log(neverReassigned);
</script>
