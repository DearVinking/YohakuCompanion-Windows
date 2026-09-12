<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import AppSidebar from "./components/AppSidebar.vue";
import TopBar from "./components/TopBar.vue";
import ConnectionView from "./views/ConnectionView.vue";
import GeneralView from "./views/GeneralView.vue";
import HistoryView from "./views/HistoryView.vue";
import PrivacyView from "./views/PrivacyView.vue";
import AdvancedView from "./views/AdvancedView.vue";
import { IconHistory, IconPlug, IconShield, IconSliders, IconWrench } from "./components/sidebarIcons";
import GlassShell from "./design-system/components/GlassShell.vue";
import { api, errorText } from "./api";
import type { ConnectionStatusView } from "./api/types";

const sidebarVisible = ref(true);
const activeView = ref("connection");
const status = ref<ConnectionStatusView | null>(null);
let timer: number | undefined;

const navItems = [
    { id: "connection", label: "连接", icon: IconPlug },
    { id: "general", label: "通用", icon: IconSliders },
    { id: "privacy", label: "隐私", icon: IconShield },
    { id: "history", label: "历史", icon: IconHistory },
    { id: "advanced", label: "高级", icon: IconWrench },
];

const viewTitle = computed(
    () => navItems.find((item) => item.id === activeView.value)?.label ?? "连接",
);

const statusSummary = computed(() => {
    const c = status.value?.coordinator;
    if (!c) return "就绪";
    switch (c.state) {
        case "active":
            return "Live Desk 已连接";
        case "connecting":
            return "连接中…";
        case "degraded":
            return "降级（最近发送失败）";
        case "updateRequired":
            return "需要更新客户端";
        case "serverFeatureUnavailable":
            return "服务端功能不可用";
        case "suspended":
            return "已暂停";
        default:
            return "未启用";
    }
});

const paused = ref(false);

async function refresh() {
    try {
        status.value = await api.getConnectionStatus();
        paused.value = (await api.getSettings()).pauseSharing;
    } catch (e) {
        errorText(e);
    }
}

async function togglePause() {
    await api.setPaused(!paused.value);
    await refresh();
}

onMounted(() => {
    refresh();
    timer = window.setInterval(refresh, 1000);
});
onBeforeUnmount(() => window.clearInterval(timer));
</script>

<template>
  <GlassShell v-model:sidebar-visible="sidebarVisible">
    <template #sidebar>
      <AppSidebar
        :nav-items="navItems"
        :active-view="activeView"
        app-version="1.7.3"
        @navigate="activeView = $event"
      />
    </template>
    <template #toolbar>
      <TopBar
        :view-title="viewTitle"
        :status-summary="statusSummary"
        :paused="paused"
        @toggle-sidebar="sidebarVisible = !sidebarVisible"
        @toggle-pause="togglePause"
      />
    </template>
    <main v-scrollbar.vertical class="min-h-0 flex-1 overflow-y-auto px-4 pb-5">
      <ConnectionView v-if="activeView === 'connection'" />
      <GeneralView v-else-if="activeView === 'general'" />
      <PrivacyView v-else-if="activeView === 'privacy'" />
      <HistoryView v-else-if="activeView === 'history'" />
      <AdvancedView v-else />
    </main>
  </GlassShell>
</template>
