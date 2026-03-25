<script setup lang="ts">
import GithubIcon from '@/assets/github.svg?component';
</script>

<template>
	<div class="w-full h-full drag-bar relative flex flex-row items-center justify-between px-2 overflow-hidden"
		data-tauri-drag-region>
		<div class="flex flex-row items-center justify-center gap-2 z-10">
			<p class="text-text">
				Shocking VRC
			</p>
			<p class="text-text">
				//
			</p>
			<button class="text-accent cursor-pointer hover:underline z-10">
				v 0.0.0
			</button>
		</div>
		<button tabindex="-1" class="flex flex-row items-center justify-center cursor-pointer z-10">
			<component :is="GithubIcon"
				class="min-w-3.5 aspect-auto text-text transition-all duration-150 hover:text-accent" />
		</button>
	</div>
</template>

<style lang="css" scoped>
.drag-bar::before {
	content: "";
	position: absolute;

	top: 0%;
	left: 0%;
	width: 100%;
	height: 100%;

	background-image: repeating-linear-gradient(315deg,
			transparent,
			transparent 5px,
			#44444450 5px,
			#44444450 10px);

	z-index: 0;
	pointer-events: none;
}
</style>