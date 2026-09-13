<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";

const visible = defineModel<boolean>("sidebarVisible", { default: true });
const MIN_W = 260;
const MAX_W = 360;
const STEP = 10;
const sidebarWidth = ref(MIN_W);
const dragging = ref(false);
let dragStartX = 0;
let dragStartWidth = MIN_W;

function clamp(width: number) {
  return Math.min(MAX_W, Math.max(MIN_W, width));
}

function onDragMove(event: MouseEvent) {
  sidebarWidth.value = clamp(dragStartWidth + event.clientX - dragStartX);
}

function stopDrag() {
  dragging.value = false;
  window.removeEventListener("mousemove", onDragMove);
  window.removeEventListener("mouseup", stopDrag);
}

function startDrag(event: MouseEvent) {
  event.preventDefault();
  dragStartX = event.clientX;
  dragStartWidth = sidebarWidth.value;
  dragging.value = true;
  window.addEventListener("mousemove", onDragMove);
  window.addEventListener("mouseup", stopDrag);
}

function resizeWithKeyboard(event: KeyboardEvent) {
  if (event.key === "ArrowLeft")
    sidebarWidth.value = clamp(sidebarWidth.value - STEP);
  else if (event.key === "ArrowRight")
    sidebarWidth.value = clamp(sidebarWidth.value + STEP);
  else if (event.key === "Home") sidebarWidth.value = MIN_W;
  else if (event.key === "End") sidebarWidth.value = MAX_W;
  else return;
  event.preventDefault();
}

onBeforeUnmount(stopDrag);
</script>

<template>
  <div class="relative h-full bg-(--content-bg)">
    <aside
      class="absolute inset-y-3 left-3 z-15 flex flex-col overflow-hidden rounded-2xl border border-[rgba(255,255,255,.247)] bg-(--sidebar-bg) shadow-[0_6px_22px_rgba(0,0,0,0.1),0_1px_2px_rgba(0,0,0,0.05)] backdrop-blur-lg dark:border-[rgba(255,255,255,.247)]"
      :class="[
        dragging ? '' : 'transition-[translate,opacity] duration-220 ease-out',
        visible ? '' : '-translate-x-[calc(100%+12px)] opacity-0',
      ]"
      :style="{ width: `${sidebarWidth}px` }"
    >
      <div
        v-scrollbar.vertical
        class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto px-2.5 pt-3 pb-3.5"
      >
        <slot name="sidebar" />
      </div>
    </aside>

    <div
      v-if="visible"
      data-testid="shell-sidebar-resizer"
      class="shell-sidebar-resizer absolute inset-y-3 z-16 w-6 cursor-col-resize"
      :style="{ left: `${sidebarWidth}px` }"
      role="region"
      aria-label="调整侧边栏宽度"
    >
      <div
        class="absolute inset-0"
        role="separator"
        aria-label="调整侧边栏宽度"
        aria-orientation="vertical"
        :aria-valuemin="MIN_W"
        :aria-valuemax="MAX_W"
        :aria-valuenow="sidebarWidth"
        tabindex="0"
        @mousedown="startDrag"
        @keydown="resizeWithKeyboard"
      />
    </div>

    <div
      data-testid="shell-content"
      class="shell-content absolute inset-0 flex flex-col"
      :class="dragging ? '' : 'transition-[padding-left] duration-220 ease-out'"
      :style="{ paddingLeft: visible ? `${sidebarWidth + 24}px` : '0' }"
    >
      <header
        class="flex h-14 flex-none items-center gap-2 pr-3 pl-4"
        role="banner"
      >
        <slot name="toolbar" />
      </header>
      <slot />
    </div>
  </div>
</template>

<style scoped>
@media (max-width: 767px) {
  .shell-content {
    padding-left: 0 !important;
  }

  .shell-sidebar-resizer {
    display: none;
  }
}
</style>
