<script setup lang="ts">
import { onMounted, ref } from "vue";

import { api, errorText } from "../api";
import PillButton from "../design-system/components/PillButton.vue";
import TextInput from "../design-system/components/TextInput.vue";
import type { S3ConfigView } from "../api/types";

const config = ref<S3ConfigView | null>(null);
const secretKey = ref("");
const error = ref("");
const saved = ref(false);

async function load() {
    config.value = await api.getS3Config();
}

async function save() {
    if (!config.value) return;
    error.value = "";
    try {
        config.value = await api.updateS3Config({
            endpoint: config.value.endpoint ?? "",
            bucket: config.value.bucket,
            region: config.value.region,
            customDomain: config.value.customDomain ?? "",
            basePath: config.value.basePath,
            accessKey: config.value.accessKey,
            secretKey: secretKey.value || null,
        });
        secretKey.value = "";
        saved.value = true;
        setTimeout(() => (saved.value = false), 1500);
    } catch (e) {
        error.value = errorText(e);
    }
}

onMounted(load);
</script>

<template>
  <div v-if="config" class="mx-auto flex max-w-[860px] flex-col gap-4 py-4">
    <section
      class="rounded-2xl border border-gray-200 bg-white/50 p-5 backdrop-blur-md dark:border-white/20 dark:bg-black/30"
    >
      <div class="min-w-0">
        <h2 class="type-heading">S3 资产托管</h2>
        <div class="type-meta font-semibold text-secondary">
          应用图标与媒体封面的公网 URL 来源
        </div>
      </div>
      <label class="mt-4 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">Endpoint</span>
        <TextInput
          :model-value="config.endpoint ?? ''"
          @change="config.endpoint = ($event.target as HTMLInputElement).value || null"
          autocomplete="off"
          spellcheck="false"
          placeholder="file.example.com（留空用 AWS 虚拟主机式）"
          class="min-w-0 flex-1"
        />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">Bucket</span>
        <TextInput v-model="config.bucket" autocomplete="off" spellcheck="false" class="min-w-0 flex-1" />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">Region</span>
        <TextInput v-model="config.region" autocomplete="off" spellcheck="false" class="min-w-0 flex-1" />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">公网域</span>
        <TextInput
          :model-value="config.customDomain ?? ''"
          @change="config.customDomain = ($event.target as HTMLInputElement).value || null"
          autocomplete="off"
          spellcheck="false"
          placeholder="cdn.example.com（可选）"
          class="min-w-0 flex-1"
        />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">键前缀</span>
        <TextInput v-model="config.basePath" autocomplete="off" spellcheck="false" class="min-w-0 flex-1" />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">Access Key</span>
        <TextInput v-model="config.accessKey" autocomplete="off" spellcheck="false" class="min-w-0 flex-1" />
      </label>
      <label class="mt-2 flex min-h-11 max-w-[420px] items-center gap-3">
        <span class="w-[84px] flex-none type-body text-secondary">Secret Key</span>
        <TextInput
          v-model="secretKey"
          type="password"
          autocomplete="off"
          spellcheck="false"
          :placeholder="config.hasCredentials ? '已保存，留空保持不变' : '未设置'"
          class="min-w-0 flex-1"
        />
      </label>
      <div class="mt-4 flex flex-wrap items-center gap-2.5">
        <PillButton primary @click="save">{{ saved ? "已保存" : "保存" }}</PillButton>
        <span class="type-meta text-secondary">凭据经 DPAPI 加密存储。</span>
      </div>
      <div v-if="error" class="mt-3 type-body text-danger">{{ error }}</div>
    </section>
  </div>
</template>
