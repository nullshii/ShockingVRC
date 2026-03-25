<script setup lang="ts">
import { computed } from 'vue';

const props = defineProps<{
  iconName?: String,
  title?: String
}>();

const iconPath = computed(() => {
  if (!props.iconName) return '';
  return new URL(`../assets/${props.iconName}.svg`, import.meta.url).href;
});
</script>

<template>
  <button class="flex flex-row items-center justify-center gap-2 px-2 text-text cursor-pointer">
    <img v-if="iconName" class="drag-none min-w-3.5 aspect-auto" :src="iconPath" />
    <p>
      {{ title }}
    </p>
  </button>
</template>