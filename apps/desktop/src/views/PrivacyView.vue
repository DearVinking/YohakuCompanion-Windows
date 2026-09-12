<script setup lang="ts">
import { onMounted, ref } from "vue";

import { api } from "../api";
import type { AppRule, Level, PrivacyRules } from "../api/types";
import PillButton from "../design-system/components/PillButton.vue";
import SegmentedControl from "../design-system/components/SegmentedControl.vue";
import TextInput from "../design-system/components/TextInput.vue";

const rules = ref<PrivacyRules | null>(null);
const newKey = ref("");
const saved = ref(false);

const levelItems: Array<{ value: Level; label: string }> = [
    { value: "inherit", label: "跟随全局" },
    { value: "share", label: "允许" },
    { value: "hide", label: "隐藏" },
];

async function load() {
    rules.value = await api.getPrivacyRules();
}

async function save() {
    if (!rules.value) return;
    rules.value = await api.updatePrivacyRules(rules.value);
    saved.value = true;
    setTimeout(() => (saved.value = false), 1500);
}

function addRule() {
    if (!rules.value || !newKey.value.trim()) return;
    rules.value.apps[newKey.value.trim().toLowerCase()] = {
        application: "inherit",
        windowTitle: "inherit",
        media: "inherit",
        displayAlias: null,
    };
    newKey.value = "";
}

function removeRule(key: string) {
    if (!rules.value) return;
    delete rules.value.apps[key];
}

function ruleOf(key: string): AppRule {
    return rules.value!.apps[key];
}

onMounted(load);
</script>

<template>
  <div v-if="rules" class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">全局默认</h2>
        <div class="type-meta font-semibold text-secondary">「隐藏」恒优先</div>
      </div>

      <div class="mt-4 flex min-h-11 items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">应用</span>
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
        <span class="w-[84px] flex-none type-body text-secondary">媒体</span>
        <SegmentedControl
          v-model="rules.defaults.media"
          :items="levelItems"
          @update:model-value="save"
        />
      </div>
    </section>

    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">逐应用规则</h2>
        <div class="type-meta font-semibold text-secondary">
          应用键为 exe 文件名（小写）
        </div>
      </div>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <TextInput
          v-model="newKey"
          autocomplete="off"
          spellcheck="false"
          placeholder="msedge.exe"
          class="w-[200px]"
        />
        <PillButton @click="addRule">添加</PillButton>
        <PillButton primary @click="save">{{ saved ? "已保存" : "保存全部" }}</PillButton>
      </div>

      <div
        v-for="(rule, key) in rules.apps"
        :key="key"
        class="mt-4 border-t border-gray-200/70 pt-4 dark:border-white/10"
      >
        <div class="flex flex-wrap items-center gap-x-4 gap-y-2">
          <span class="mono min-w-0 flex-1 truncate">{{ key }}</span>
          <PillButton destructive @click="removeRule(key as string)">删除</PillButton>
        </div>
        <div class="mt-2 flex flex-wrap items-center gap-x-4 gap-y-2">
          <div class="flex min-h-9 items-center gap-2">
            <span class="type-body text-secondary">应用</span>
            <SegmentedControl
              v-model="ruleOf(key as string).application"
              :items="levelItems"
              @update:model-value="save"
            />
          </div>
          <div class="flex min-h-9 items-center gap-2">
            <span class="type-body text-secondary">标题</span>
            <SegmentedControl
              v-model="ruleOf(key as string).windowTitle"
              :items="levelItems"
              @update:model-value="save"
            />
          </div>
          <div class="flex min-h-9 items-center gap-2">
            <span class="type-body text-secondary">媒体</span>
            <SegmentedControl
              v-model="ruleOf(key as string).media"
              :items="levelItems"
              @update:model-value="save"
            />
          </div>
          <TextInput
            :model-value="ruleOf(key as string).displayAlias ?? ''"
            autocomplete="off"
            spellcheck="false"
            placeholder="显示别名"
            class="w-[140px]"
            @change="((ruleOf(key as string).displayAlias = ($event.target as HTMLInputElement).value || null), save())"
          />
        </div>
      </div>
      <div v-if="!Object.keys(rules.apps).length" class="mt-4 type-body text-secondary">
        暂无逐应用规则。
      </div>
    </section>
  </div>
</template>
