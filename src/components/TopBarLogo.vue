<script setup lang="ts">
import Logo from "@/assets/logo.svg?component";
</script>

<template>
    <div
        class="flex-row items-center ring-2 ring-separator justify-center min-w-8 min-h-8 flex overflow-hidden"
    >
        <component :is="Logo" class="min-w-3.5 aspect-auto text-accent" />
    </div>
</template>
