<script setup lang="ts">
defineOptions({ inheritAttrs: false });

defineProps<{
  checked?: boolean;
  disabled?: boolean;
}>();

const emit = defineEmits<{ change: [value: boolean] }>();
</script>

<template>
  <button
    v-bind="$attrs"
    type="button"
    role="switch"
    :aria-checked="checked ? 'true' : 'false'"
    :data-checked="checked ? 'true' : 'false'"
    :disabled="disabled"
    class="group relative h-6 w-14 flex-none rounded-full bg-[rgba(0,0,0,.098)] transition-colors duration-200 outline-none focus-visible:ring-2 focus-visible:ring-focus-ring focus-visible:ring-offset-1 focus-visible:ring-offset-transparent disabled:cursor-not-allowed disabled:opacity-50 data-[checked=true]:bg-[#0088FF] dark:bg-[rgba(255,255,255,.098)] dark:data-[checked=true]:bg-[#0091FF]"
    @click="emit('change', !checked)"
  >
    <span
      aria-hidden="true"
      class="absolute top-0.5 left-0.5 h-5 w-8 rounded-full bg-white/85 shadow-[0_1px_3px_rgba(0,0,0,.25)] transition-all duration-400 group-active:scale-150 group-active:bg-white/5 group-active:ring-1 group-active:ring-black/10 group-data-[checked=true]:left-5.5 group-active:dark:ring-white/30"
    ></span>
  </button>
</template>
