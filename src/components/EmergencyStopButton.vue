<script setup lang="ts">
const text = "EMERGENCY STOP";
</script>

<template>
	<button
		class="bg-limit-power text-text italic font-semibold w-full cursor-pointer ring-2 ring-separator flex flex-row justify-center items-center gap-1 hover:gap-8 px-5 duration-500 transition-all ease-in-out">
		<span v-for="(char, charIndex) in text" :key="charIndex" :class="char === ' ' ? 'w-1' : ''">
			{{ char === ' ' ? '\u00A0' : char }}
		</span>
	</button>
</template>