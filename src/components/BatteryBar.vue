<script setup lang="ts">
import BatteryIcon from '@/assets/battery.svg?component';
import ChargingBatteryIcon from '@/assets/battery_charging.svg?component';
import { computed } from 'vue';

const props = defineProps({
	level: { type: Number, default: 100 },
	isCharging: { type: Boolean, default: false }
});

const clipStyle = computed(() => {
	return { clipPath: `inset(0 ${120 - props.level}% 0 0)` };
});
</script>

<template>
	<div class="flex justify-center items-center gap-2 py-1 px-2 min-w-10 border-b-2 border-separator">
		<component v-if="isCharging" :is="ChargingBatteryIcon" class="min-w-3.5 aspect-auto text-accent" />
		<div v-if="!isCharging" class="relative inline-block">
			<component :is="BatteryIcon" class="min-w-3.5 aspect-auto text-accent" />
			<div :style="clipStyle"
				class="absolute inset-0 min-w-3.5 aspect-auto bg-accent rounded-sm transition-all duration-500 ease-in-out">
			</div>
		</div>
		<span>{{ level }}%</span>
	</div>
</template>