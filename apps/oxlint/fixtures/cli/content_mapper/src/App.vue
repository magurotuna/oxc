<script setup lang="ts">
async function load(): Promise<void> {}
// eslint-disable-next-line typescript/no-floating-promises
load();
// eslint-disable-next-line typescript/no-misused-promises
const value = 1;
load();
</script>

<template>
  <button @click="load">{{ value }}</button>
</template>
