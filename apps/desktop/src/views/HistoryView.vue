<script setup lang="ts">
import { onMounted, ref } from "vue";

import { api } from "../api";
import BadgeTag from "../design-system/components/BadgeTag.vue";
import PillButton from "../design-system/components/PillButton.vue";
import type { SyncEvent } from "../api/types";

const events = ref<SyncEvent[]>([]);

const triggerText: Record<string, string> = {
    semanticChange: "语义变化",
    heartbeat: "心跳",
    lifecycle: "生命周期",
    manual: "手动",
};

async function load() {
    events.value = await api.listHistory();
}

async function clear() {
    await api.clearHistory();
    await load();
}

onMounted(load);
</script>

<template>
  <div class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">同步历史</h2>
        <div class="type-meta font-semibold text-secondary">
          本地投递审计，最多 1000 条，裁最旧
        </div>
      </div>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <PillButton @click="load">刷新</PillButton>
        <PillButton destructive @click="clear">清空</PillButton>
      </div>
    </section>

    <section
      v-if="events.length"
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="flex flex-col gap-3">
        <div
          v-for="e in events"
          :key="e.id"
          class="flex flex-wrap items-center gap-x-4 gap-y-1.5 border-b border-gray-200/70 pb-3 last:border-b-0 last:pb-0 dark:border-white/10"
        >
          <BadgeTag :kind="e.state === 'succeeded' ? 'ok' : e.state === 'failed' ? 'bad' : 'gold'">
            {{ e.state === "succeeded" ? "成功" : e.state === "failed" ? "失败" : "跳过" }}
          </BadgeTag>
          <span class="type-body">{{ triggerText[e.trigger] ?? e.trigger }}</span>
          <span class="type-meta text-secondary tabular-nums">
            {{ new Date(e.finishedAt).toLocaleString() }}
          </span>
          <span v-if="e.errorCode" class="mono type-meta text-danger">{{ e.errorCode }}</span>
          <span v-else-if="e.outputSummary" class="type-meta text-secondary">
            {{ e.outputSummary }}
          </span>
        </div>
      </div>
    </section>
    <section
      v-else
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="type-body text-secondary">暂无记录。</div>
    </section>
  </div>
</template>
