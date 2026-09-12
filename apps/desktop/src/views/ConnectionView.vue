<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Link2, Unplug } from "@lucide/vue";

import { api, errorText } from "../api";
import BadgeTag from "../design-system/components/BadgeTag.vue";
import PillButton from "../design-system/components/PillButton.vue";
import TextInput from "../design-system/components/TextInput.vue";
import type { ConnectionStatusView, PreviewView } from "../api/types";

const status = ref<ConnectionStatusView | null>(null);
const error = ref("");
const serverUrl = ref("https://");
const pairingCode = ref("");
const deviceName = ref("我的 Windows 设备");
const pairing = ref(false);
const preview = ref<PreviewView | null>(null);
const previewLoading = ref(false);
let timer: number | undefined;

const stateText = computed(() => {
    switch (status.value?.coordinator.state) {
        case "active":
            return "已连接";
        case "connecting":
            return "连接中…";
        case "degraded":
            return "降级";
        case "updateRequired":
            return "需要更新";
        case "serverFeatureUnavailable":
            return "服务端不可用";
        case "suspended":
            return "已暂停";
        default:
            return "未启用";
    }
});

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
    try {
        status.value = await api.getConnectionStatus();
        error.value = "";
    } catch (e) {
        error.value = errorText(e);
    }
}

async function pair() {
    pairing.value = true;
    error.value = "";
    try {
        await api.startPairing(serverUrl.value, pairingCode.value, deviceName.value);
        await refresh();
    } catch (e) {
        error.value = errorText(e);
    } finally {
        pairing.value = false;
    }
}

async function unpair() {
    await api.removePairing();
    await refresh();
}

async function loadPreview() {
    previewLoading.value = true;
    try {
        preview.value = await api.getPreview();
    } catch (e) {
        error.value = errorText(e);
    } finally {
        previewLoading.value = false;
    }
}

async function enable() {
    try {
        await api.enableLiveDesk();
        await refresh();
    } catch (e) {
        error.value = errorText(e);
    }
}

async function disable() {
    await api.disableLiveDesk();
    await refresh();
}

onMounted(() => {
    refresh();
    timer = window.setInterval(refresh, 1000);
});
onBeforeUnmount(() => window.clearInterval(timer));
</script>

<template>
  <div class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="flex min-w-0 flex-wrap items-center gap-2.5">
        <div class="mr-auto min-w-0">
          <h2 class="type-heading">连接状态</h2>
          <div class="type-meta font-semibold text-secondary">Live Desk</div>
        </div>
        <BadgeTag :kind="stateKind" class="flex-none">{{ stateText }}</BadgeTag>
      </div>
      <div
        v-if="status?.coordinator.lastSentAt"
        class="mt-3 type-body text-secondary"
      >
        最近发送：{{ new Date(status.coordinator.lastSentAt).toLocaleString() }}
      </div>
      <div
        v-if="status?.coordinator.lastErrorCode"
        class="mt-3 type-body text-danger"
      >
        最近错误：{{ status.coordinator.lastErrorCode }}
      </div>
      <div
        v-if="status?.consentStale"
        class="mt-3 type-body text-secondary"
      >
        隐私策略已变化，建议重新确认预览后关闭再开启。
      </div>
    </section>

    <section
      v-if="!status?.paired"
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">配对</h2>
        <div class="type-meta font-semibold text-secondary">
          在 Yohaku 服务端生成一次性配对码后填入
        </div>
      </div>
      <label class="mt-4 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">服务器</span>
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
          placeholder="一次性配对码"
          class="min-w-0 flex-1"
        />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">设备名</span>
        <TextInput
          v-model="deviceName"
          autocomplete="off"
          spellcheck="false"
          maxlength="120"
          class="min-w-0 flex-1"
        />
      </label>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <PillButton primary :disabled="pairing || !pairingCode" @click="pair">
          <Link2 :size="15" />
          {{ pairing ? "配对中…" : "开始配对" }}
        </PillButton>
      </div>
      <div v-if="error" class="mt-3 type-body text-danger">{{ error }}</div>
    </section>

    <section
      v-else
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">已配对设备</h2>
        <div class="type-meta font-semibold text-secondary">
          {{ status?.baseUrl }}
        </div>
      </div>
      <div class="mt-3 type-body">
        <span class="text-secondary">设备 ID：</span>
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
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">净化预览与发布同意</h2>
        <div class="type-meta font-semibold text-secondary">
          开启前必须先查看当前预览（10 分钟内有效）
        </div>
      </div>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <PillButton :disabled="previewLoading" @click="loadPreview">
          {{ previewLoading ? "获取中…" : "刷新预览" }}
        </PillButton>
        <PillButton
          v-if="!status.liveDeskEnabled"
          primary
          :disabled="!preview"
          @click="enable"
        >
          确认并开启 Live Desk
        </PillButton>
        <PillButton destructive v-else @click="disable">关闭 Live Desk</PillButton>
      </div>
      <div
        v-if="preview"
        class="mt-4 rounded-xl border border-gray-200/70 bg-white/40 p-4 dark:border-white/10 dark:bg-black/20"
      >
        <div class="type-body">应用：{{ preview.application?.displayName ?? "（无）" }}</div>
        <div v-if="preview.application?.windowTitle" class="type-body">
          标题：{{ preview.application.windowTitle }}
        </div>
        <div v-if="preview.media" class="type-body">
          媒体：{{ preview.media.title ?? "?" }} - {{ preview.media.artist ?? "?" }}（{{ preview.media.player ?? "?" }}）
        </div>
        <div class="type-meta text-secondary">可用性：{{ preview.availability }}</div>
      </div>
      <div v-if="error" class="mt-3 type-body text-danger">{{ error }}</div>
    </section>
  </div>
</template>
