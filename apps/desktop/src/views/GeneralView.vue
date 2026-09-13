<script lang="ts">
let saveQueue = Promise.resolve();
</script>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";

import { api, errorText } from "../api";
import type { Settings, SettingsPatch } from "../api/types";
import PillButton from "../design-system/components/PillButton.vue";
import TextInput from "../design-system/components/TextInput.vue";
import ToggleSwitch from "../design-system/components/ToggleSwitch.vue";

const settings = ref<Settings | null>(null);
const error = ref("");
const playersInput = ref("");
let confirmedSettings: Settings | null = null;
let requestVersion = 0;
let playersEditVersion = 0;
let disposed = false;

const emptyPatch: SettingsPatch = {
    shareApplications: null,
    shareWindowTitles: null,
    shareMedia: null,
    ignoreNullArtist: null,
    launchAtLogin: null,
    pauseSharing: null,
    preferredPlayers: null,
};

async function load(): Promise<void> {
    const version = ++requestVersion;
    error.value = "";
    try {
        await saveQueue;
        if (disposed || version !== requestVersion) return;
        const loaded = await api.getSettings();
        if (disposed || version !== requestVersion) return;
        confirmedSettings = loaded;
        settings.value = { ...loaded };
        playersInput.value = loaded.preferredPlayers.join(", ");
    } catch (e) {
        if (!disposed && version === requestVersion) error.value = errorText(e);
    }
}

async function save(patch: SettingsPatch): Promise<Settings | null> {
    const version = ++requestVersion;
    error.value = "";
    const request = saveQueue.then(() => api.updateSettings(patch));
    saveQueue = request.then(() => undefined, () => undefined);
    try {
        const saved = await request;
        if (disposed) return null;
        confirmedSettings = saved;
        if (version === requestVersion) settings.value = { ...saved };
        return saved;
    } catch (e) {
        if (disposed) return null;
        error.value = errorText(e);
        if (version === requestVersion && confirmedSettings) {
            settings.value = { ...confirmedSettings };
        }
        return null;
    }
}

async function setToggle(
    key: "shareApplications" | "shareWindowTitles" | "shareMedia" | "ignoreNullArtist" | "launchAtLogin",
    value: boolean,
): Promise<void> {
    if (!settings.value) return;
    settings.value[key] = value;
    await save({ ...emptyPatch, [key]: value });
}

function markPlayersEdited(): void {
    ++playersEditVersion;
}

async function setPreferredPlayers(value: string): Promise<void> {
    if (!settings.value) return;
    const editVersion = ++playersEditVersion;
    const preferredPlayers = value.split(",").map((name) => name.trim()).filter(Boolean);
    settings.value.preferredPlayers = preferredPlayers;
    const saved = await save({
        ...emptyPatch,
        preferredPlayers,
    });
    if (saved && editVersion === playersEditVersion) {
        playersInput.value = saved.preferredPlayers.join(", ");
    }
}

onMounted(load);
onBeforeUnmount(() => {
    disposed = true;
    ++requestVersion;
});
</script>

<template>
  <div v-if="settings || error" class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <div v-if="error" role="alert" class="flex items-center gap-2.5 type-body text-danger">
      {{ error }}
      <PillButton v-if="!settings" @click="load">重试</PillButton>
    </div>
    <template v-if="settings">
      <section class="page-card">
        <div class="min-w-0">
          <h2 class="type-heading">同步内容</h2>
          <div class="type-meta font-semibold text-secondary">选择要在 Live Desk 中显示的信息</div>
        </div>

        <label class="mt-4 flex min-h-11 items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">当前应用</span>
          <ToggleSwitch
            :checked="settings.shareApplications"
            @change="setToggle('shareApplications', $event)"
          />
          <span class="type-body text-secondary">你正在使用的应用名称</span>
        </label>
        <label class="flex min-h-11 items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">窗口标题</span>
          <ToggleSwitch
            :checked="settings.shareWindowTitles"
            @change="setToggle('shareWindowTitles', $event)"
          />
          <span class="type-body text-secondary">网页或文档标题，需在「规则」中允许同步</span>
        </label>
        <label class="flex min-h-11 items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">播放内容</span>
          <ToggleSwitch
            :checked="settings.shareMedia"
            @change="setToggle('shareMedia', $event)"
          />
          <span class="type-body text-secondary">正在播放的音乐、视频等信息</span>
        </label>
      </section>

      <section class="page-card">
        <div class="min-w-0">
          <h2 class="type-heading">优先使用的播放器</h2>
          <div class="type-meta font-semibold text-secondary">打开多个播放器时，优先使用这里指定的播放器</div>
        </div>
        <label class="mt-4 flex min-h-11 max-w-[420px] items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">播放器名称</span>
          <TextInput
            v-model="playersInput"
            autocomplete="off"
            spellcheck="false"
            placeholder="spotify, cloudmusic, qqmusic"
            class="min-w-0 flex-1"
            @input="markPlayersEdited"
            @change="setPreferredPlayers(($event.target as HTMLInputElement).value)"
          />
        </label>
        <div class="mt-2 type-meta text-secondary">
          用英文逗号分隔。Spotify 可填 spotify，网易云音乐可填 cloudmusic，QQ 音乐可填 qqmusic。
        </div>
      </section>

      <section class="page-card flex items-center justify-between gap-4">
        <div class="min-w-0">
          <h2 id="launch-at-login-title" class="type-heading">登录时启动</h2>
          <div id="launch-at-login-description" class="type-meta font-semibold text-secondary">
            登录 Windows 后自动启动 Yohaku Companion
          </div>
        </div>
        <ToggleSwitch
          :checked="settings.launchAtLogin"
          aria-labelledby="launch-at-login-title"
          aria-describedby="launch-at-login-description"
          @change="setToggle('launchAtLogin', $event)"
        />
      </section>
    </template>
  </div>
</template>
