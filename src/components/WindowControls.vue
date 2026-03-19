<script lang="ts" setup>
import { getCurrentWindow } from '@tauri-apps/api/window';

const appWindow = getCurrentWindow();

const minimize = async () => {
	await appWindow.minimize();
}

const toggleMaximize = async () => {
	await appWindow.toggleMaximize();
}

const close = async () => {
	await appWindow.close();
}
</script>

<template>
	<div class="flex flex-row items-center ring-2 ring-separator justify-center gap-5 px-2 h-full">
		<button tabindex="-1" @click="minimize">
			<img class="drag-none min-w-5 min-h-5" src="../assets/controls/minimize.svg" alt="minimize" />
		</button>
		<button tabindex="-1" @click="toggleMaximize">
			<img class="drag-none min-w-5 min-h-5" src="../assets/controls/maximize.svg" alt="maximize" />
		</button>
		<button tabindex="-1" @click="close">
			<img class="drag-none min-w-5 min-h-5" src="../assets/controls/close.svg" alt="close" />
		</button>
	</div>
</template>