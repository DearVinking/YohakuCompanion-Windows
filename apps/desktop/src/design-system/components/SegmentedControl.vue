<script setup lang="ts" generic="T">
import type { FunctionalComponent } from "vue";

import {
  computed,
  onBeforeUnmount,
  onMounted,
  ref,
  useId,
  useTemplateRef,
  watch,
} from "vue";

const props = defineProps<{
  disabled?: boolean;
  items: Array<{
    value: T;
    label: string;
    ariaLabel?: string;
    color?: string;
    icon?: FunctionalComponent;
  }>;
}>();

const selected = defineModel<T>({ required: true });
const group = useTemplateRef<HTMLElement>("group");
const groupName = useId();
const indicator = ref<{ left: number; width: number } | null>(null);
const indicatorAnimated = ref(false);
let resizeObserver: null | ResizeObserver = null;

const indicatorStyle = computed(() => {
  if (!indicator.value) return undefined;
  return {
    transform: `translateX(${indicator.value.left}px)`,
    width: `${indicator.value.width}px`,
  };
});

function measureIndicator(animate: boolean) {
  const selectedOption = group.value?.querySelector<HTMLElement>(
    '[data-segment-selected="true"]',
  );
  if (!selectedOption) {
    indicator.value = null;
    indicatorAnimated.value = false;
    return;
  }

  const nextIndicator = {
    left: selectedOption.offsetLeft,
    width: selectedOption.offsetWidth,
  };
  if (
    indicator.value?.left === nextIndicator.left &&
    indicator.value.width === nextIndicator.width
  ) {
    return;
  }

  indicatorAnimated.value = animate && indicator.value !== null;
  indicator.value = nextIndicator;
}

function observeSegments() {
  resizeObserver?.disconnect();
  if (!group.value || !resizeObserver) return;
  resizeObserver.observe(group.value);
  for (const option of group.value.querySelectorAll<HTMLElement>(
    "[data-segment-option]",
  )) {
    resizeObserver.observe(option);
  }
}

onMounted(() => {
  measureIndicator(false);
  resizeObserver = new ResizeObserver(() => measureIndicator(false));
  observeSegments();
});

onBeforeUnmount(() => resizeObserver?.disconnect());

watch(selected, () => measureIndicator(true), { flush: "post" });
watch(
  () =>
    props.items.map((item) => [
      item.value,
      item.label,
      item.ariaLabel,
      item.icon,
    ]),
  () => {
    observeSegments();
    measureIndicator(false);
  },
  { flush: "post" },
);
</script>

<template>
  <div
    ref="group"
    class="relative flex items-center gap-1"
    role="radiogroup"
    :aria-disabled="disabled || undefined"
  >
    <div
      v-if="indicator"
      class="pointer-events-none absolute inset-y-0 left-0 rounded-lg border border-white/60 bg-[rgba(0,0,0,0.098)] shadow-[0_0_35px_rgba(0,0,0,0.1)] dark:border-white/20 dark:bg-[rgba(255,255,255,0.098)]"
      :class="[
        disabled ? 'opacity-45' : '',
        indicatorAnimated
          ? 'transition-[transform,width] duration-200 ease-[cubic-bezier(.2,.8,.2,1)] motion-reduce:transition-none'
          : '',
      ]"
      :style="indicatorStyle"
      data-testid="segmented-control-indicator"
      aria-hidden="true"
    />
    <label
      v-for="item in items"
      :key="String(item.value)"
      class="relative z-10 flex cursor-pointer items-center gap-1 rounded-lg border-none px-2.5 py-1.5 type-body leading-none text-secondary"
      :class="[
        item.value === selected && !item.color
          ? 'font-semibold text-label'
          : '',
        disabled ? 'cursor-not-allowed opacity-45' : '',
      ]"
      :style="
        item.value === selected && item.color
          ? { color: item.color }
          : undefined
      "
      data-segment-option
      :data-segment-selected="item.value === selected ? 'true' : undefined"
    >
      <input
        v-model="selected"
        type="radio"
        :name="groupName"
        :value="item.value"
        :disabled="disabled"
        :aria-label="item.ariaLabel"
        class="absolute inset-0 m-0 size-full cursor-pointer appearance-none rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-focus-ring focus-visible:ring-offset-1 focus-visible:ring-offset-transparent disabled:cursor-not-allowed"
      />
      <span>{{ item.label }}</span>
      <component :is="item.icon" v-if="item.icon" aria-hidden="true" />
    </label>
  </div>
</template>
