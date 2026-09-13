<script setup lang="ts">
import { Radio, RadioOff } from "@lucide/vue";

import RoundIconButton from "../design-system/components/RoundIconButton.vue";
import { IconSidebarToggle } from "../design-system/icons";

defineProps<{
    viewTitle: string;
    statusSummary: string;
    syncActive: boolean;
    syncLabel: string;
    syncDisabled: boolean;
    syncError: string;
}>();

defineEmits<{
    toggleSidebar: [];
    toggleSync: [];
}>();
</script>

<template>
  <div class="flex min-w-0 flex-1 items-center gap-2">
    <div class="mr-auto min-w-0">
      <h1 class="truncate type-heading">{{ viewTitle }}</h1>
      <div v-if="syncError" role="alert" class="truncate type-compact text-danger" :title="syncError">
        {{ syncError }}
      </div>
      <div v-else class="truncate type-compact text-secondary">
        {{ statusSummary }}
      </div>
    </div>

    <div class="flex flex-none items-center gap-2">
      <RoundIconButton
        title="显示或隐藏导航栏"
        aria-label="显示或隐藏导航栏"
        :aria-pressed="false"
        @click="$emit('toggleSidebar')"
      >
        <IconSidebarToggle />
      </RoundIconButton>

      <RoundIconButton
        primary
        :outlined="syncActive"
        :title="syncLabel"
        :aria-label="syncLabel"
        :aria-pressed="syncActive"
        :disabled="syncDisabled"
        @click="$emit('toggleSync')"
      >
        <Radio v-if="syncActive" :size="15" />
        <RadioOff v-else :size="15" />
      </RoundIconButton>
    </div>
  </div>
</template>
