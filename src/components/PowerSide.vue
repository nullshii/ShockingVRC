<script setup lang="ts">
import BatteryBar from './BatteryBar.vue';
import PowerMeter from './PowerMeter.vue';

</script>

<template>
	<div class="flex flex-col border-r-2 border-separator">
		<BatteryBar :level="60" :is-charging=false />
		<div class="flex flex-row h-full">
			<PowerMeter />
			<PowerMeter />
		</div>
	</div>
</template>