<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Link2, Unplug } from "@lucide/vue";

import { api, errorText } from "../api";
import BadgeTag from "../design-system/components/BadgeTag.vue";
import PillButton from "../design-system/components/PillButton.vue";
import TextInput from "../design-system/components/TextInput.vue";
import type { ConnectionStatusView, PreviewView } from "../api/types";
import { connectionStateText, errorCodeText } from "../uiText";

const emit = defineEmits<{ connectionChanged: [] }>();

const status = ref<ConnectionStatusView | null>(null);
const error = ref("");
const refreshError = ref("");
const serverUrl = ref("https://");
const pairingCode = ref("");
const deviceName = ref("我的 Windows 电脑");
const pairing = ref(false);
const preview = ref<PreviewView | null>(null);
let refreshing = false;
let refreshVersion = 0;
let timer: number | undefined;

const stateText = computed(() => connectionStateText(status.value?.coordinator.state));

const stateKind = computed<"ok" | "bad" | "gold" | "info">(() => {
    switch (status.value?.coordinator.state) {
        case "active":
            return "ok";
        case "connecting":
            return "info";
        case "suspended":
            return "gold";
        default:
            return "bad";
    }
});

async function refresh() {
    const version = ++refreshVersion;
    refreshing = true;
    try {
        const connectionStatus = await api.getConnectionStatus();
        if (version !== refreshVersion) return;
        status.value = connectionStatus;
        const currentPreview = connectionStatus.paired ? await api.getPreview() : null;
        if (version !== refreshVersion) return;
        preview.value = currentPreview;
        refreshError.value = "";
    } catch (e) {
        if (version !== refreshVersion) return;
        preview.value = null;
        refreshError.value = errorText(e);
    } finally {
        if (version === refreshVersion) refreshing = false;
    }
}

async function pair() {
    pairing.value = true;
    error.value = "";
    try {
        await api.startPairing(serverUrl.value, pairingCode.value, deviceName.value);
        emit("connectionChanged");
        await refresh();
    } catch (e) {
        error.value = errorText(e);
    } finally {
        pairing.value = false;
    }
}

async function unpair() {
    error.value = "";
    try {
        await api.removePairing();
        preview.value = null;
        emit("connectionChanged");
        await refresh();
    } catch (e) {
        error.value = errorText(e);
    }
}

onMounted(() => {
    refresh();
    timer = window.setInterval(() => {
        if (!refreshing) refresh();
    }, 1000);
});
onBeforeUnmount(() => {
    ++refreshVersion;
    window.clearInterval(timer);
});
</script>

<template>
  <div class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <section class="page-card">
      <div class="flex min-w-0 flex-wrap items-center gap-2.5">
        <div class="mr-auto min-w-0">
          <h2 class="type-heading">连接状态</h2>
          <div class="type-meta font-semibold text-secondary">同步到 Yohaku 的 Live Desk</div>
        </div>
        <BadgeTag :kind="stateKind" class="flex-none">{{ stateText }}</BadgeTag>
      </div>
      <div
        v-if="status?.coordinator.lastErrorCode"
        class="mt-3 type-body text-danger"
        :title="`错误代码：${status.coordinator.lastErrorCode}`"
      >
        同步提示：{{ errorCodeText(status.coordinator.lastErrorCode) }}
      </div>
      <div v-if="error || refreshError" role="alert" class="mt-3 type-body text-danger">
        {{ error || refreshError }}
      </div>
    </section>

    <section
      v-if="!status?.paired"
      class="page-card"
    >
      <div class="min-w-0">
        <h2 class="type-heading">连接这台电脑</h2>
        <div class="type-meta font-semibold text-secondary">
          在 Yohaku 中生成配对码，再填写服务器地址和配对码。
        </div>
      </div>
      <label class="mt-4 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">服务器地址</span>
        <TextInput
          v-model="serverUrl"
          autocomplete="off"
          spellcheck="false"
          placeholder="https://core.example.com"
          class="min-w-0 flex-1"
        />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">配对码</span>
        <TextInput
          v-model="pairingCode"
          autocomplete="off"
          spellcheck="false"
          maxlength="32"
          placeholder="粘贴配对码"
          class="min-w-0 flex-1"
        />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">设备名称</span>
        <TextInput
          v-model="deviceName"
          autocomplete="off"
          spellcheck="false"
          maxlength="120"
          placeholder="我的 Windows 电脑"
          class="min-w-0 flex-1"
        />
      </label>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <PillButton primary :disabled="pairing || !pairingCode" @click="pair">
          <Link2 :size="15" />
          {{ pairing ? "正在连接…" : "连接" }}
        </PillButton>
      </div>
    </section>

    <section
      v-else
      class="page-card"
    >
      <div class="min-w-0">
        <h2 class="type-heading">这台电脑已配对</h2>
        <div class="type-meta font-semibold text-secondary">
          {{ status?.baseUrl }}
        </div>
      </div>
      <div class="mt-3 type-body">
        <span class="text-secondary">设备编号：</span>
        <span class="mono">{{ status?.deviceId }}</span>
      </div>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <PillButton destructive @click="unpair">
          <Unplug :size="15" />
          解除配对
        </PillButton>
      </div>
    </section>

    <section
      v-if="status?.paired"
      class="page-card"
    >
      <div class="min-w-0">
        <h2 class="type-heading">同步内容</h2>
        <div class="type-meta font-semibold text-secondary">
          当前设置允许同步到 Live Desk 的信息，内容自动更新。
        </div>
      </div>
      <div
        class="mt-4 rounded-xl border border-gray-200/70 bg-white/40 p-4 dark:border-white/10 dark:bg-black/20"
      >
        <template v-if="preview">
          <div class="type-body">应用名称：{{ preview.application?.displayName ?? "不显示" }}</div>
          <div class="type-body">窗口标题：{{ preview.application?.windowTitle ?? "不显示" }}</div>
          <div v-if="preview.media" class="type-body">
            播放内容：{{ preview.media.title ?? "未提供标题" }}
            <span v-if="preview.media.artist"> · {{ preview.media.artist }}</span>
            <span v-if="preview.media.player">（{{ preview.media.player }}）</span>
          </div>
          <div v-else class="type-body">播放内容：不显示</div>
        </template>
        <div v-else class="type-body text-secondary">
          {{ refreshError ? "暂时无法读取同步内容。" : "正在读取同步内容…" }}
        </div>
        <hr class="my-3 border-gray-200/70 dark:border-white/10" />
        <div class="type-body text-secondary">
          最近同步时间：{{ status.coordinator.lastSentAt ? new Date(status.coordinator.lastSentAt).toLocaleString() : "尚未同步" }}
        </div>
      </div>
    </section>
  </div>
</template>
