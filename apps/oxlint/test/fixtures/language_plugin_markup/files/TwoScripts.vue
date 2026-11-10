<template>
  <p>@read:fromSetup</p>
</template>
<script lang="ts">
const sharedConstant = 1;
let reassignedInSetup = 1;
const unusedInScript = 1;
</script>
<script setup lang="ts">
const fromSetup = sharedConstant;
reassignedInSetup = 2;
const unusedInSetup = 1;
</script>
