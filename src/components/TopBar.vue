<script setup lang="ts">
import TopBarDrag from "./TopBarDrag.vue";
import TopBarLogo from "./TopBarLogo.vue";
import WindowControls from "./WindowControls.vue";
</script>

<!-- TODO: Add fonts -->
<template>
    <div
        class="flex flex-row ring-2 ring-separator justify-between items-center select-none"
    >
        <TopBarLogo />
        <TopBarDrag />
        <WindowControls />
    </div>
</template>
