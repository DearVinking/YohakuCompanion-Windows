<script setup lang="ts">
defineOptions({ inheritAttrs: false });

defineProps<{
  checked?: boolean;
  disabled?: boolean;
}>();
</script>

<template>
  <span class="relative inline-flex size-5 flex-none">
    <input
      v-bind="$attrs"
      type="checkbox"
      :checked="checked"
      :disabled="disabled"
      class="peer absolute inset-0 m-0 size-5 cursor-pointer appearance-none rounded-[6px] bg-[rgba(0,0,0,.098)] transition-colors outline-none checked:bg-[#0088FF] focus-visible:ring-2 focus-visible:ring-focus-ring focus-visible:ring-offset-1 focus-visible:ring-offset-transparent disabled:cursor-not-allowed dark:bg-[rgba(255,255,255,.098)] dark:checked:bg-[#0091FF]"
    />
    <svg
      aria-hidden="true"
      class="pointer-events-none absolute top-[3px] left-[3px] size-3.5 text-white opacity-0 transition-opacity peer-checked:opacity-100"
      viewBox="0 0 14 14"
      fill="none"
    >
      <path
        d="M3 7.5L6 10.5L11 4.5"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      />
    </svg>
  </span>
</template>
