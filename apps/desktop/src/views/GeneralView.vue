<script setup lang="ts">
import { onMounted, ref } from "vue";
import { api } from "../api";
import type { Settings, SettingsPatch } from "../api/types";

const settings = ref<Settings | null>(null);

async function load() {
    settings.value = await api.getSettings();
}

async function toggle(key: "shareApplications" | "shareWindowTitles" | "shareMedia" | "ignoreNullArtist" | "launchAtLogin", value: boolean) {
    settings.value = await api.updateSettings({ [key]: value } as unknown as SettingsPatch);
}

onMounted(load);
</script>

<template>
    <div v-if="settings">
        <div class="card">
            <h3>上报内容</h3>
            <label class="switch row">
                <input
                    type="checkbox"
                    :checked="settings.shareApplications"
                    @change="toggle('shareApplications', !settings.shareApplications)"
                />
                <span class="grow">正在使用的应用</span>
            </label>
            <label class="switch row">
                <input
                    type="checkbox"
                    :checked="settings.shareWindowTitles"
                    @change="toggle('shareWindowTitles', !settings.shareWindowTitles)"
                />
                <span class="grow">窗口标题（同时需要隐私规则允许）</span>
            </label>
            <label class="switch row">
                <input
                    type="checkbox"
                    :checked="settings.shareMedia"
                    @change="toggle('shareMedia', !settings.shareMedia)"
                />
                <span class="grow">正在播放的音乐</span>
            </label>
        </div>
        <div class="card">
            <h3>媒体会话仲裁</h3>
            <div class="row"><span class="muted">以下关键词命中播放器时优先上报（逗号分隔，不区分大小写）。</span></div>
            <div class="row">
                <input
                    type="text"
                    :value="settings.preferredPlayers.join(', ')"
                    @change="api.updateSettings({ preferredPlayers: ($event.target as HTMLInputElement).value.split(',').map((s) => s.trim()).filter(Boolean) } as unknown as SettingsPatch).then((s) => (settings = s))"
                />
            </div>
        </div>
        <div class="card">
            <h3>运行</h3>
            <label class="switch row">
                <input
                    type="checkbox"
                    :checked="settings.launchAtLogin"
                    @change="toggle('launchAtLogin', !settings.launchAtLogin)"
                />
                <span class="grow">开机自启</span>
            </label>
            <label class="switch row">
                <input
                    type="checkbox"
                    :checked="settings.pauseSharing"
                    @change="api.setPaused(!settings.pauseSharing).then((s) => (settings = s))"
                />
                <span class="grow">暂停上报（向服务端清除当前状态）</span>
            </label>
        </div>
    </div>
</template>
