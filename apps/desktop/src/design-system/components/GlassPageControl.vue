<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{
  currentPage: number;
  label?: string;
  pageCount: number;
}>();

const count = computed(() =>
  Number.isFinite(props.pageCount)
    ? Math.max(0, Math.trunc(props.pageCount))
    : 0,
);
const selection = computed(() => {
  const page = Number.isFinite(props.currentPage)
    ? Math.trunc(props.currentPage)
    : 0;
  return Math.min(Math.max(page, 0), Math.max(count.value - 1, 0));
});
</script>

<template>
  <div
    v-if="count > 0"
    class="glass-page-control"
    role="img"
    :aria-label="label ?? `第 ${selection + 1} 页，共 ${count} 页`"
  >
    <span
      v-for="page in count"
      :key="page"
      class="page-slot"
      aria-hidden="true"
    >
      <span class="page-dot" :class="{ selected: page - 1 === selection }" />
    </span>
  </div>
</template>

<style scoped>
.glass-page-control {
  --page-control-surface: rgb(243 243 243 / 70%);
  --page-control-sheen: linear-gradient(
    180deg,
    rgb(255 255 255 / 60%),
    transparent 55%
  );
  --page-control-rim: rgb(255 255 255 / 90%);
  --page-control-highlight: rgb(255 255 255 / 60%);
  --page-control-bottom-inset: rgb(0 0 0 / 5%);

  display: inline-flex;
  flex: none;
  align-items: center;
  justify-content: center;
  gap: 4px;
  height: 28px;
  padding: 0 12px;
  border-radius: 9999px;
  background: var(--page-control-surface);
  background-image: var(--page-control-sheen);
  backdrop-filter: blur(16px) saturate(150%);
  box-shadow:
    inset 0 0 0 1px var(--page-control-rim),
    inset 0 1px 0 var(--page-control-highlight),
    inset 0 -1px 0 var(--page-control-bottom-inset);
}

.page-slot,
.page-dot {
  display: block;
  flex: none;
  width: 8px;
  height: 8px;
}

.page-dot {
  border-radius: 50%;
  background: var(--label);
  opacity: 0.5;
  transform: scale(0.75);
  transition:
    transform 240ms cubic-bezier(0.2, 0.8, 0.2, 1),
    opacity 240ms ease;
}

.page-dot.selected {
  opacity: 1;
  transform: scale(1);
}

@media (prefers-color-scheme: dark) {
  .glass-page-control {
    --page-control-surface: rgb(0 0 0 / 50%);
    --page-control-sheen: linear-gradient(
      180deg,
      rgb(255 255 255 / 8%),
      transparent 55%
    );
    --page-control-rim: rgb(255 255 255 / 16%);
    --page-control-highlight: rgb(255 255 255 / 24%);
    --page-control-bottom-inset: rgb(255 255 255 / 10%);
  }
}

@media (prefers-reduced-motion: reduce) {
  .page-dot {
    transition: none;
  }
}

@media (prefers-reduced-transparency: reduce) {
  .glass-page-control {
    background: #f3f3f3;
    background-image: none;
    backdrop-filter: none;
  }
}

@media (prefers-reduced-transparency: reduce) and (prefers-color-scheme: dark) {
  .glass-page-control {
    background: #121214;
  }
}

@media (forced-colors: active) {
  .glass-page-control {
    background: Canvas;
    outline: 1px solid CanvasText;
    outline-offset: -1px;
    backdrop-filter: none;
    box-shadow: none;
  }

  .page-dot {
    forced-color-adjust: none;
    background: CanvasText;
    opacity: 1;
  }

  .page-dot.selected {
    background: Highlight;
    outline: 1px solid Highlight;
    outline-offset: 1px;
  }
}
</style>
