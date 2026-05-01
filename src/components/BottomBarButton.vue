<script setup lang="ts">
import type { Component } from "vue";

defineProps<{
    icon?: Component;
    title?: String;
}>();
</script>

<template>
    <button
        class="flex flex-row items-center justify-center gap-2 px-2 cursor-pointer duration-200 text-text stroke-text hover:stroke-accent hover:text-accent"
    >
        <component :is="icon" class="min-w-3.5 aspect-auto stroke-current" />
        <p>
            {{ title }}
        </p>
    </button>
</template>
