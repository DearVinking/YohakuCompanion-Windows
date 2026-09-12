<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { api, errorText } from "../api";
import type { ConnectionStatusView, PreviewView } from "../api/types";

const status = ref<ConnectionStatusView | null>(null);
const error = ref("");
const serverUrl = ref("https://");
const pairingCode = ref("");
const deviceName = ref("我的 Windows 设备");
const pairing = ref(false);
const pairingOk = ref(false);
const preview = ref<PreviewView | null>(null);
const previewLoading = ref(false);
let timer: number | undefined;

const stateText = computed(() => {
    if (!status.value) return "";
    switch (status.value.coordinator.state) {
        case "active":
            return "已连接";
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

const stateColor = computed(() => {
    switch (status.value?.coordinator.state) {
        case "active":
            return "var(--ok)";
        case "connecting":
            return "var(--warn)";
        case "degraded":
        case "updateRequired":
        case "serverFeatureUnavailable":
            return "var(--bad)";
        default:
            return "var(--text-dim)";
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
        pairingOk.value = true;
        await refresh();
    } catch (e) {
        error.value = errorText(e);
    } finally {
        pairing.value = false;
    }
}

async function unpair() {
    await api.removePairing();
    pairingOk.value = false;
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
    <div v-if="status">
        <div class="card">
            <h3>连接状态</h3>
            <div class="row">
                <span class="status-dot" :style="{ background: stateColor }" />
                <span class="grow">{{ stateText }}</span>
                <span v-if="status.liveDeskEnabled" class="ok-text">Live Desk 已开启</span>
            </div>
            <div class="row" v-if="status.consentStale">
                <span class="error-text">隐私策略已变化，建议重新确认预览后关闭再开启。</span>
            </div>
            <div class="row" v-if="status.coordinator.lastSentAt">
                <span class="muted grow">最近发送：{{ new Date(status.coordinator.lastSentAt).toLocaleString() }}</span>
            </div>
            <div class="row" v-if="status.coordinator.lastErrorCode">
                <span class="error-text grow">最近错误：{{ status.coordinator.lastErrorCode }}</span>
            </div>
        </div>

        <div class="card" v-if="!status.paired">
            <h3>配对</h3>
            <div class="row"><span class="muted">在 Yohaku 服务端生成一次性配对码后填入以下信息。</span></div>
            <div class="row">
                <input v-model="serverUrl" type="text" placeholder="服务器地址，如 https://core.example.com" />
            </div>
            <div class="row">
                <input v-model="pairingCode" type="text" placeholder="一次性配对码" />
            </div>
            <div class="row">
                <input v-model="deviceName" type="text" placeholder="设备名称" />
            </div>
            <div class="row">
                <button class="primary" :disabled="pairing || !pairingCode" @click="pair">
                    {{ pairing ? "配对中…" : "开始配对" }}
                </button>
            </div>
        </div>

        <div class="card" v-else>
            <h3>已配对设备</h3>
            <div class="row"><span class="muted grow">服务器：{{ status.baseUrl }}</span></div>
            <div class="row"><span class="muted mono grow">设备 ID：{{ status.deviceId }}</span></div>
            <div class="row">
                <button class="ghost" @click="unpair">解除配对</button>
            </div>
        </div>

        <div class="card" v-if="status.paired">
            <h3>净化预览与发布同意</h3>
            <div class="row">
                <button class="ghost" :disabled="previewLoading" @click="loadPreview">
                    {{ previewLoading ? "获取中…" : "刷新预览" }}
                </button>
                <span class="muted">开启 Live Desk 前必须先查看当前预览（10 分钟内有效）。</span>
            </div>
            <div class="preview-box" v-if="preview">
                <div>应用：{{ preview.application?.displayName ?? "（无）" }}</div>
                <div v-if="preview.application?.windowTitle">标题：{{ preview.application.windowTitle }}</div>
                <div v-if="preview.media">
                    媒体：{{ preview.media.title ?? "?" }} - {{ preview.media.artist ?? "?" }}
                    （{{ preview.media.player ?? "?" }}）
                </div>
                <div class="muted">可用性：{{ preview.availability }}</div>
            </div>
            <div class="row">
                <button
                    class="primary"
                    v-if="!status.liveDeskEnabled"
                    :disabled="!preview"
                    @click="enable"
                >
                    确认并开启 Live Desk
                </button>
                <button class="ghost" v-else @click="disable">关闭 Live Desk</button>
            </div>
        </div>

        <div class="error-text" v-if="error">{{ error }}</div>
    </div>
</template>
