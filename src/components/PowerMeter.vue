<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from 'vue';

const props = defineProps({
	value: { type: Number, default: 200 },
});

const containerRef = ref<HTMLElement | null>(null);
const containerHeight = ref(0);

const ITEM_HEIGHT = 8;
const GAP = 4;

const itemCount = computed(() => {
	if (containerHeight.value <= 0) return 0;
	return Math.floor((containerHeight.value + GAP) / (ITEM_HEIGHT + GAP));
});

const observer = new ResizeObserver((entries) => {
	if (!entries.length) return;
	containerHeight.value = entries[0].contentBoxSize[0].blockSize;
});

onMounted(() => {
	if (containerRef.value) observer.observe(containerRef.value);
});

onUnmounted(() => observer.disconnect());
</script>

<template>
	<div class="flex flex-col gap-2 h-full items-center">
		<span class="shrink-0">{{ value }}</span>
		<div ref="containerRef" class="relative flex-1 w-8 overflow-hidden">
			<div class="absolute inset-0 flex flex-col gap-1">
				<div v-for="i in itemCount" :key="i" class="w-full bg-separator rounded-xs shrink-0"
					:style="{ height: `${ITEM_HEIGHT}px` }"></div>
			</div>
		</div>
	</div>
</template>