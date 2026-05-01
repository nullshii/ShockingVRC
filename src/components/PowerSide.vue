<script setup lang="ts">
import BatteryBar from "./BatteryBar.vue";
import PowerMeter from "./PowerMeter.vue";
</script>

<template>
    <div class="flex flex-col border-r-2 border-separator pb-2">
        <BatteryBar :level="60" :is-charging="false" />
        <div
            class="flex flex-row h-full items-center justify-between px-2 gap-2"
        >
            <PowerMeter :value="150" />
            <PowerMeter :value="50" />
        </div>
    </div>
</template>
