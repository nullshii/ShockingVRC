<script setup lang="ts">
import BottomBarButton from './BottomBarButton.vue';
import EmergencyStopButton from './EmergencyStopButton.vue';

</script>

<template>
	<div class="flex flex-row h-8 ring-2 ring-separator justify-between">
		<BottomBarButton iconName="sliders" title="Presets" />
		<EmergencyStopButton />
		<BottomBarButton iconName="settings" title="Settings" />
	</div>
</template>