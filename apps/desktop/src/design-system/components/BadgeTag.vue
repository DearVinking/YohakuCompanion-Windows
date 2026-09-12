<script setup lang="ts">
defineProps<{ kind: "bad" | "gold" | "info" | "ok" }>();

const KIND = {
  bad: "bg-danger-soft text-danger",
  gold: "bg-gold-soft text-gold",
  info: "bg-accent-soft text-accent",
  ok: "bg-success-soft text-success",
} as const;
</script>

<template>
  <div
    class="flex max-w-[520px] items-center gap-1 overflow-hidden rounded-lg px-2.5 py-1.5 type-body leading-none whitespace-nowrap select-none"
    :class="KIND[kind]"
  >
    <slot />
  </div>
</template>
