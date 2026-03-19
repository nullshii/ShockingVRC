<!-- TODO: Add fonts -->
<!-- TODO: Add lines to drag area -->
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
	<div class="flex flex-row ring-2 ring-separator justify-between items-center select-none">
		<div class="flex-row items-center ring-2 ring-separator justify-center min-w-8 min-h-8 flex overflow-hidden">
			<img class="drag-none w-full h-full p-1" src="../assets/logo.svg" alt="logo" />
		</div>
		<div class="w-full h-full flex flex-row items-center justify-between px-2" data-tauri-drag-region>
			<div class="flex flex-row items-center justify-center gap-2">
				<p class="text-text">
					Shocking VRC
				</p>
				<p class="text-text">
					//
				</p>
				<p class="text-accent underline">
					v 0.0.0
				</p>
			</div>
			<button tabindex="-1" class="flex flex-row items-center justify-center">
				<img class="drag-none" src="../assets/github.svg" alt="github" />
			</button>
		</div>
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
	</div>
</template>