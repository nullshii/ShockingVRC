<script setup lang="ts">
import SlidersIcon from "@/assets/sliders.svg?component";
import SettingsIcon from "@/assets/settings.svg?component";

import BottomBarButton from "./BottomBarButton.vue";
import EmergencyStopButton from "./EmergencyStopButton.vue";
</script>

<template>
    <div class="flex flex-row h-8 ring-2 ring-separator justify-between">
        <BottomBarButton :icon="SlidersIcon" title="Presets" />
        <EmergencyStopButton />
        <BottomBarButton :icon="SettingsIcon" title="Settings" />
    </div>
</template>
