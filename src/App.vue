<script setup lang="ts">
import BottomBar from './components/BottomBar.vue';
import PowerSide from './components/PowerSide.vue';
import TopBar from './components/TopBar.vue';

</script>

<template>
	<main class="flex flex-col h-screen w-full bg-bg text-text border-2 border-separator">
		<TopBar />
		<div class="h-full flex flex-row">
			<PowerSide />
			<div class="flex w-full">

			</div>
		</div>
		<BottomBar />
	</main>
</template>