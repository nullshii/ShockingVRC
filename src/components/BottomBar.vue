<script setup lang="ts">
import EmergencyStopButton from './EmergencyStopButton.vue';
import PresetsButton from './PresetsButton.vue';
import SettingsButton from './SettingsButton.vue';

</script>

<template>
	<div class="flex flex-row h-8 ring-2 ring-separator justify-between">
		<PresetsButton />
		<EmergencyStopButton />
		<SettingsButton />
	</div>
</template>