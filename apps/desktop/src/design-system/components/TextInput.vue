<script setup lang="ts">
withDefaults(defineProps<{ disabled?: boolean; type?: "number" | "text" | "password" }>(), {
  type: "text",
});
const value = defineModel<number | string>({ required: true });
</script>

<template>
  <input
    v-model="value"
    :disabled="disabled"
    :type="type"
    spellcheck="false"
    class="h-8 rounded-lg border border-control-border bg-transparent px-2 type-body text-label outline-none focus:border-focus-border focus:ring-[3px] focus:ring-focus-ring disabled:cursor-not-allowed disabled:opacity-55"
  />
</template>
