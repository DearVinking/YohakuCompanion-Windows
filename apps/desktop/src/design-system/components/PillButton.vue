<script setup lang="ts">
import { computed } from "vue";

const props = defineProps<{ primary?: boolean; destructive?: boolean }>();

const BASE =
  "inline-flex cursor-pointer items-center justify-center gap-2 whitespace-nowrap rounded-full px-4 py-1 type-body backdrop-blur-md transition-colors duration-200 disabled:cursor-not-allowed";

const PRIMARY =
  "border border-[#B2DCFF] bg-[#0088FF] text-white shadow-[0_0_35px_rgba(0,136,255,0.2)] enabled:hover:bg-[#007FF6] disabled:text-white/50 dark:border-white/10 dark:bg-[#0091FF] dark:enabled:hover:border-white/10 dark:enabled:hover:bg-[#0C9BFF]";

const NORMAL =
  "border border-white bg-gray-100/70 text-label shadow-[0_0_35px_rgba(0,0,0,0.15)] enabled:hover:bg-gray-200/70 disabled:text-disabled-label dark:border-white/20 dark:bg-black/30 dark:enabled:hover:border-white/20 dark:enabled:hover:bg-white/5";

const DESTRUCTIVE =
  "border border-white bg-gray-100/70 text-danger shadow-[0_0_35px_rgba(0,0,0,0.15)] enabled:hover:bg-gray-200/70 disabled:text-danger-disabled dark:border-white/20 dark:bg-black/30 dark:enabled:hover:border-white/20 dark:enabled:hover:bg-white/5";

const PRIMARY_DESTRUCTIVE =
  "border border-[#FFC3C4] bg-[rgba(255,56,60,.25)] text-danger shadow-[0_0_35px_rgba(0,0,0,0.15)] enabled:hover:bg-[rgba(255,56,60,.3)] disabled:text-white/50 dark:border-[rgba(255,66,69,.2)] dark:bg-[rgba(255,66,69,.25)] dark:enabled:hover:border-[rgba(255,66,69,.2)] dark:enabled:hover:bg-[rgba(255,66,69,.35)]";

const classes = computed(() => {
  if (props.primary && props.destructive) return [BASE, PRIMARY_DESTRUCTIVE];
  if (props.primary) return [BASE, PRIMARY];
  if (props.destructive) return [BASE, DESTRUCTIVE];
  return [BASE, NORMAL];
});
</script>

<template>
  <button type="button" :class="classes"><slot /></button>
</template>
