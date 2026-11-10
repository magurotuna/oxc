<script setup lang="ts">
import { ref } from "vue";

const items = ref<string[]>([]);
async function load(): Promise<void> {
  items.value = ["a"];
}
const count: number = 1;
</script>

<template>
  <p v-if="items">Has items</p>
  <button @click="load()">Load</button>
  <button @click="load">Load</button>
  <p>{{ count }}</p>
</template>
