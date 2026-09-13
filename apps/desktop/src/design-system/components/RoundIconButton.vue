<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{ grouped?: boolean; primary?: boolean; outlined?: boolean }>();

const BASE =
  "flex size-9 cursor-pointer items-center justify-center rounded-full type-body backdrop-blur-md transition-colors duration-200 disabled:cursor-not-allowed";
const PRIMARY =
  "border border-[#B2DCFF] bg-[#0088FF] text-white shadow-[0_0_35px_rgba(0,136,255,0.2)] enabled:hover:bg-[#007FF6] disabled:text-white/50 dark:border-white/10 dark:bg-[#0091FF] dark:enabled:hover:border-white/10 dark:enabled:hover:bg-[#0C9BFF]";
const PRIMARY_OUTLINED =
  "border border-[#B2DCFF] bg-gray-100/70 text-accent shadow-[0_0_35px_rgba(0,136,255,0.2)] enabled:hover:bg-gray-200/70 disabled:text-accent/50 dark:border-white/10 dark:bg-black/30 dark:enabled:hover:bg-white/5";
const NORMAL =
  "border border-white bg-gray-100/70 text-label shadow-[0_0_35px_rgba(0,0,0,0.15)] enabled:hover:border-gray-100/70 enabled:hover:bg-gray-200/70 disabled:text-disabled-label dark:border-white/20 dark:bg-black/30 dark:enabled:hover:border-white/20 dark:enabled:hover:bg-white/5";
const GROUPED =
  "bg-transparent text-label enabled:hover:bg-gray-200/70 disabled:text-disabled-label dark:enabled:hover:bg-white/5";

const classes = computed(() => {
  if (props.primary) return [BASE, props.outlined ? PRIMARY_OUTLINED : PRIMARY];
  return [BASE, props.grouped ? GROUPED : NORMAL];
});
</script>

<template>
  <button type="button" :class="classes"><slot /></button>
</template>
