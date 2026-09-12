<script setup lang="ts">
import { onMounted, ref } from "vue";

import { api } from "../api";
import type { Settings } from "../api/types";
import TextInput from "../design-system/components/TextInput.vue";
import ToggleSwitch from "../design-system/components/ToggleSwitch.vue";

const settings = ref<Settings | null>(null);

async function load() {
    settings.value = await api.getSettings();
}

async function setToggle(
    key: "shareApplications" | "shareWindowTitles" | "shareMedia" | "ignoreNullArtist" | "launchAtLogin",
    value: boolean,
) {
    settings.value = await api.updateSettings({ [key]: value } as never);
}

async function setPaused(value: boolean) {
    settings.value = await api.setPaused(value);
}

async function setPreferredPlayers(value: string) {
    settings.value = await api.updateSettings({
        preferredPlayers: value.split(",").map((v) => v.trim()).filter(Boolean),
    } as never);
}

onMounted(load);
</script>

<template>
  <div v-if="settings" class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">上报内容</h2>
        <div class="type-meta font-semibold text-secondary">数据来源</div>
      </div>

      <label class="mt-4 flex min-h-11 items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">应用</span>
        <ToggleSwitch
          :checked="settings.shareApplications"
          @change="setToggle('shareApplications', $event)"
        />
        <span class="type-body text-secondary">正在使用的应用</span>
      </label>
      <label class="flex min-h-11 items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">窗口标题</span>
        <ToggleSwitch
          :checked="settings.shareWindowTitles"
          @change="setToggle('shareWindowTitles', $event)"
        />
        <span class="type-body text-secondary">需同时被隐私规则允许</span>
      </label>
      <label class="flex min-h-11 items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">媒体</span>
        <ToggleSwitch
          :checked="settings.shareMedia"
          @change="setToggle('shareMedia', $event)"
        />
        <span class="type-body text-secondary">正在播放的音乐</span>
      </label>
    </section>

    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">媒体会话仲裁</h2>
        <div class="type-meta font-semibold text-secondary">优先播放器</div>
      </div>
      <label class="mt-4 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">关键词</span>
        <TextInput
          :model-value="settings.preferredPlayers.join(', ')"
          autocomplete="off"
          spellcheck="false"
          placeholder="spotify, cloudmusic, qqmusic"
          class="min-w-0 flex-1"
          @change="setPreferredPlayers(($event.target as HTMLInputElement).value)"
        />
      </label>
      <div class="mt-2 type-meta text-secondary">
        命中播放器标识（不区分大小写）时优先上报，逗号分隔。
      </div>
    </section>

    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">运行</h2>
        <div class="type-meta font-semibold text-secondary">开机与暂停</div>
      </div>

      <label class="mt-4 flex min-h-11 items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">自启</span>
        <ToggleSwitch
          :checked="settings.launchAtLogin"
          @change="setToggle('launchAtLogin', $event)"
        />
        <span class="type-body text-secondary">开机自动启动</span>
      </label>
      <label class="flex min-h-11 items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">暂停</span>
        <ToggleSwitch
          :checked="settings.pauseSharing"
          @change="setPaused($event)"
        />
        <span class="type-body text-secondary">暂停上报并向服务端清除当前状态</span>
      </label>
    </section>
  </div>
</template>
