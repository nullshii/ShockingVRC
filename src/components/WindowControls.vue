<script lang="ts" setup>
import CloseIcon from "@/assets/controls/close.svg?component";
import MaximizeIcon from "@/assets/controls/maximize.svg?component";
import MinimizeIcon from "@/assets/controls/minimize.svg?component";

import { getCurrentWindow } from "@tauri-apps/api/window";

const appWindow = getCurrentWindow();

const minimize = async () => {
    await appWindow.minimize();
};

const toggleMaximize = async () => {
    await appWindow.toggleMaximize();
};

const close = async () => {
    await appWindow.close();
};
</script>

<template>
    <div
        class="flex flex-row items-center ring-2 ring-separator justify-center gap-5 px-2 h-full"
    >
        <button tabindex="-1" @click="minimize">
            <component
                :is="MinimizeIcon"
                class="drag-none min-w-5 min-h-5 transition-all duration-150 text-text hover:text-base-power cursor-pointer"
            />
        </button>
        <button tabindex="-1" @click="toggleMaximize">
            <component
                :is="MaximizeIcon"
                class="drag-none min-w-5 min-h-5 transition-all duration-150 text-text hover:text-over-power cursor-pointer"
            />
        </button>
        <button tabindex="-1" @click="close">
            <component
                :is="CloseIcon"
                class="drag-none min-w-5 min-h-5 transition-all duration-150 text-text hover:text-limit-power cursor-pointer"
            />
        </button>
    </div>
</template>
