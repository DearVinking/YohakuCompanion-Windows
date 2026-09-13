<script lang="ts">
let saveQueue = Promise.resolve();
</script>

<script setup lang="ts">
import { onBeforeUnmount, onMounted, reactive, ref } from "vue";

import { api, errorText } from "../api";
import type { AppRule, Level, PrivacyRules } from "../api/types";
import PillButton from "../design-system/components/PillButton.vue";
import SegmentedControl from "../design-system/components/SegmentedControl.vue";
import TextInput from "../design-system/components/TextInput.vue";

const rules = ref<PrivacyRules | null>(null);
const newKey = ref("");
const error = ref("");
let requestVersion = 0;
let disposed = false;

const levelItems: Array<{ value: Level; label: string }> = [
    { value: "inherit", label: "使用默认" },
    { value: "share", label: "同步" },
    { value: "hide", label: "隐藏" },
];

function snapshotRules(current: PrivacyRules): PrivacyRules {
    return {
        ...current,
        defaults: { ...current.defaults },
        apps: Object.fromEntries<AppRule>(
            Object.entries(current.apps).map(([key, rule]) => [key, { ...rule }]),
        ),
    };
}

function setRules(current: PrivacyRules): void {
    const next = snapshotRules(current);
    for (const key of Object.keys(next.apps)) {
        next.apps[key] = reactive(next.apps[key]);
    }
    rules.value = next;
}

async function load(): Promise<void> {
    const version = ++requestVersion;
    error.value = "";
    try {
        await saveQueue;
        if (disposed || version !== requestVersion) return;
        const loaded = await api.getPrivacyRules();
        if (disposed || version !== requestVersion) return;
        setRules(loaded);
    } catch (e) {
        if (!disposed && version === requestVersion) error.value = errorText(e);
    }
}

async function save(): Promise<void> {
    if (!rules.value) return;
    const version = ++requestVersion;
    const submitted = snapshotRules(rules.value);
    error.value = "";
    const request = saveQueue.then(() => api.updatePrivacyRules(submitted));
    saveQueue = request.then(() => undefined, () => undefined);
    try {
        const saved = await request;
        if (disposed || version !== requestVersion) return;
        setRules(saved);
        error.value = "";
    } catch (e) {
        if (!disposed) error.value = errorText(e);
    }
}

async function addRule(): Promise<void> {
    if (!rules.value || !newKey.value.trim()) return;
    rules.value.apps = {
        ...rules.value.apps,
        [newKey.value.trim().toLowerCase()]: reactive<AppRule>({
            application: "inherit",
            windowTitle: "inherit",
            media: "inherit",
            displayAlias: null,
        }),
    };
    newKey.value = "";
    await save();
}

async function removeRule(key: string): Promise<void> {
    if (!rules.value) return;
    delete rules.value.apps[key];
    await save();
}

function markAliasEdited(): void {
    ++requestVersion;
}

async function setAlias(rule: AppRule, event: Event): Promise<void> {
    if (!(event.target instanceof HTMLInputElement)) return;
    rule.displayAlias = event.target.value || null;
    await save();
}

onMounted(load);
onBeforeUnmount(() => {
    disposed = true;
    ++requestVersion;
});
</script>

<template>
  <div v-if="rules || error" class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <div v-if="error" role="alert" class="flex items-center gap-2.5 type-body text-danger">
      {{ error }}
      <PillButton v-if="!rules" @click="load">重试</PillButton>
    </div>
    <template v-if="rules">
      <section class="page-card">
        <div class="min-w-0">
          <h2 class="type-heading">默认规则</h2>
          <div class="type-meta font-semibold text-secondary">未单独设置的应用会使用这些规则。更改后自动保存。</div>
        </div>

        <div class="mt-4 flex min-h-11 items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">应用名称</span>
          <SegmentedControl
            v-model="rules.defaults.application"
            :items="levelItems"
            @update:model-value="save"
          />
        </div>
        <div class="flex min-h-11 items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">窗口标题</span>
          <SegmentedControl
            v-model="rules.defaults.windowTitle"
            :items="levelItems"
            @update:model-value="save"
          />
        </div>
        <div class="flex min-h-11 items-center gap-3">
          <span class="w-[84px] flex-none type-body text-secondary">播放内容</span>
          <SegmentedControl
            v-model="rules.defaults.media"
            :items="levelItems"
            @update:model-value="save"
          />
        </div>
      </section>

      <section class="page-card">
        <div class="min-w-0">
          <h2 class="type-heading">应用规则</h2>
          <div class="type-meta font-semibold text-secondary">
            输入应用的程序文件名（例如 msedge.exe），为它单独设置同步规则。
          </div>
        </div>
        <div class="mt-4 flex flex-wrap items-center gap-2.5">
          <TextInput
            v-model="newKey"
            autocomplete="off"
            spellcheck="false"
            placeholder="msedge.exe"
            aria-label="应用程序文件名"
            class="w-[200px]"
          />
          <PillButton @click="addRule">添加规则</PillButton>
        </div>

        <div
          v-for="(rule, key) in rules.apps"
          :key="key"
          class="mt-4 border-t border-gray-200/70 pt-4 dark:border-white/10"
        >
          <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
            <span class="mono min-w-0 flex-1 truncate">{{ key }}</span>
            <PillButton destructive @click="removeRule(key)">删除规则</PillButton>
          </div>
          <div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-2">
            <div class="flex min-h-9 items-center gap-2">
              <span class="type-body text-secondary">应用名称</span>
              <SegmentedControl
                v-model="rule.application"
                :items="levelItems"
                @update:model-value="save"
              />
            </div>
            <div class="h-5 w-px flex-none bg-gray-200/80 dark:bg-white/10" aria-hidden="true"></div>
            <div class="flex min-h-9 items-center gap-2">
              <span class="type-body text-secondary">窗口标题</span>
              <SegmentedControl
                v-model="rule.windowTitle"
                :items="levelItems"
                @update:model-value="save"
              />
            </div>
            <div class="h-5 w-px flex-none bg-gray-200/80 dark:bg-white/10" aria-hidden="true"></div>
            <div class="flex min-h-9 items-center gap-2">
              <span class="type-body text-secondary">播放内容</span>
              <SegmentedControl
                v-model="rule.media"
                :items="levelItems"
                @update:model-value="save"
              />
            </div>
            <div class="h-5 w-px flex-none bg-gray-200/80 dark:bg-white/10" aria-hidden="true"></div>
            <TextInput
              :model-value="rule.displayAlias ?? ''"
              autocomplete="off"
              spellcheck="false"
              placeholder="显示名称（可选）"
              aria-label="自定义应用名称"
              title="在 Live Desk 中显示的应用名称"
              class="w-[140px]"
              @input="markAliasEdited"
              @change="setAlias(rule, $event)"
            />
          </div>
        </div>
        <div v-if="!Object.keys(rules.apps).length" class="mt-4 type-body text-secondary">
          还没有应用规则，所有应用都使用上面的默认设置。
        </div>
      </section>
    </template>
  </div>
</template>
