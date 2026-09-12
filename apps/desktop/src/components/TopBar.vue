<script setup lang="ts">
import { Pause, Play } from "@lucide/vue";

import PillButton from "../design-system/components/PillButton.vue";
import { IconSidebarToggle } from "../design-system/icons";

defineProps<{
    viewTitle: string;
    statusSummary: string;
    paused: boolean;
}>();

defineEmits<{
    toggleSidebar: [];
    togglePause: [];
}>();
</script>

<template>
  <div class="flex min-w-0 flex-1 items-center gap-2">
    <button
      type="button"
      aria-label="切换侧边栏"
      class="flex size-8 flex-none cursor-pointer items-center justify-center rounded-lg hover:bg-[rgba(0,0,0,.035)] dark:hover:bg-[rgba(255,255,255,.05)]"
      @click="$emit('toggleSidebar')"
    >
      <IconSidebarToggle />
    </button>
    <div class="mr-auto min-w-0">
      <h1 class="truncate type-heading">{{ viewTitle }}</h1>
      <div class="truncate type-compact text-secondary">
        {{ statusSummary }}
      </div>
    </div>

    <div class="flex flex-none items-center gap-2">
      <PillButton @click="$emit('togglePause')">
        <component :is="paused ? Play : Pause" :size="15" />
        {{ paused ? "恢复上报" : "暂停上报" }}
      </PillButton>
    </div>
  </div>
</template>
