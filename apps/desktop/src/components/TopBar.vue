<script setup lang="ts">
import { Pause, Play } from "@lucide/vue";

import RoundIconButton from "../design-system/components/RoundIconButton.vue";
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
    <div class="mr-auto min-w-0">
      <h1 class="truncate type-heading">{{ viewTitle }}</h1>
      <div class="truncate type-compact text-secondary">
        {{ statusSummary }}
      </div>
    </div>

    <div class="flex flex-none items-center gap-2">
      <RoundIconButton
        title="显示/隐藏侧边栏"
        aria-label="显示/隐藏侧边栏"
        :aria-pressed="false"
        @click="$emit('toggleSidebar')"
      >
        <IconSidebarToggle />
      </RoundIconButton>

      <RoundIconButton
        primary
        :title="paused ? '恢复上报' : '暂停上报'"
        :aria-label="paused ? '恢复上报' : '暂停上报'"
        @click="$emit('togglePause')"
      >
        <Play v-if="paused" :size="15" />
        <Pause v-else :size="15" />
      </RoundIconButton>
    </div>
  </div>
</template>
