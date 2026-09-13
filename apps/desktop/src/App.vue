<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import AppSidebar from "./components/AppSidebar.vue";
import TopBar from "./components/TopBar.vue";
import ConnectionView from "./views/ConnectionView.vue";
import GeneralView from "./views/GeneralView.vue";
import PrivacyView from "./views/PrivacyView.vue";
import { IconListFilter, IconRadio, IconSliders } from "./components/sidebarIcons";
import GlassShell from "./design-system/components/GlassShell.vue";
import { api, errorText } from "./api";
import type { ConnectionStatusView } from "./api/types";
import { connectionStateText } from "./uiText";

const sidebarVisible = ref(true);
const activeView = ref("connection");
const status = ref<ConnectionStatusView | null>(null);
let refreshing = false;
let refreshVersion = 0;
let timer: number | undefined;

const navItems = [
    { id: "connection", label: "同步", icon: IconRadio },
    { id: "privacy", label: "规则", icon: IconListFilter },
    { id: "general", label: "通用", icon: IconSliders },
];

const viewTitle = computed(
    () => navItems.find((item) => item.id === activeView.value)?.label ?? "同步",
);

const statusSummary = computed(() => connectionStateText(status.value?.coordinator.state));

const paused = ref(false);
const syncBusy = ref(false);
const syncError = ref("");
const refreshError = ref("");
const syncActive = computed(() => !!status.value?.liveDeskEnabled && !paused.value);
const syncLabel = computed(() => {
    if (!status.value) return "正在读取状态…";
    if (!status.value.paired) return "未配对";
    if (!status.value.liveDeskEnabled) return "未开启同步";
    return paused.value ? "已暂停同步" : "同步中";
});

async function refresh() {
    const version = ++refreshVersion;
    refreshing = true;
    try {
        const [connectionStatus, settings] = await Promise.all([
            api.getConnectionStatus(),
            api.getSettings(),
        ]);
        if (version !== refreshVersion) return;
        status.value = connectionStatus;
        paused.value = settings.pauseSharing;
        refreshError.value = "";
    } catch (e) {
        if (version !== refreshVersion) return;
        refreshError.value = errorText(e);
    } finally {
        if (version === refreshVersion) refreshing = false;
    }
}

async function toggleSync() {
    if (!status.value?.paired || syncBusy.value) return;
    syncBusy.value = true;
    syncError.value = "";
    try {
        if (!status.value.liveDeskEnabled) {
            await api.setPaused(false);
            await api.enableLiveDesk();
        } else {
            await api.setPaused(!paused.value);
        }
        await refresh();
    } catch (e) {
        syncError.value = errorText(e);
    } finally {
        syncBusy.value = false;
    }
}

onMounted(() => {
    refresh();
    timer = window.setInterval(() => {
        if (!refreshing && !syncBusy.value) refresh();
    }, 1000);
});
onBeforeUnmount(() => {
    ++refreshVersion;
    window.clearInterval(timer);
});
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
        :sync-active="syncActive"
        :sync-label="syncLabel"
        :sync-disabled="!status?.paired || syncBusy"
        :sync-error="syncError || refreshError"
        @toggle-sidebar="sidebarVisible = !sidebarVisible"
        @toggle-sync="toggleSync"
      />
    </template>
    <main v-scrollbar.vertical class="min-h-0 flex-1 overflow-y-auto px-4 pb-5">
      <ConnectionView v-if="activeView === 'connection'" @connection-changed="refresh" />
      <GeneralView v-else-if="activeView === 'general'" />
      <PrivacyView v-else-if="activeView === 'privacy'" />
    </main>
  </GlassShell>
</template>
