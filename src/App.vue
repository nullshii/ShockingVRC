<script setup lang="ts">
import BottomBar from './components/BottomBar.vue';
import TopBar from './components/TopBar.vue';

</script>

<template>
	<main class="flex flex-col h-screen w-full bg-bg text-text border-2 border-separator">
		<TopBar />
		<div class="h-full">
		</div>
		<BottomBar />
	</main>
</template>