<script setup lang="ts">
import { onMounted, ref } from "vue";
import { api, errorText } from "../api";
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
    <div v-if="config">
        <div class="card">
            <h3>S3 资产托管（应用图标 / 媒体封面公网 URL）</h3>
            <div class="row"><span class="grow">Endpoint（自定义时走 path-style，留空用 AWS 虚拟主机式）</span></div>
            <div class="row"><input type="text" v-model="config.endpoint" placeholder="如 file.example.com" /></div>
            <div class="row"><span class="grow">Bucket</span></div>
            <div class="row"><input type="text" v-model="config.bucket" /></div>
            <div class="row"><span class="grow">Region</span></div>
            <div class="row"><input type="text" v-model="config.region" /></div>
            <div class="row"><span class="grow">自定义公网域（可选，优先于 endpoint）</span></div>
            <div class="row"><input type="text" v-model="config.customDomain" placeholder="如 cdn.example.com" /></div>
            <div class="row"><span class="grow">对象键前缀</span></div>
            <div class="row"><input type="text" v-model="config.basePath" placeholder="app-icons" /></div>
            <div class="row"><span class="grow">Access Key</span></div>
            <div class="row"><input type="text" v-model="config.accessKey" /></div>
            <div class="row"><span class="grow">Secret Key（{{ config.hasCredentials ? "已保存，留空保持不变" : "未设置" }}）</span></div>
            <div class="row"><input type="password" v-model="secretKey" placeholder="输入新 Secret Key" /></div>
            <div class="row">
                <button class="primary" @click="save">{{ saved ? "已保存" : "保存" }}</button>
                <span class="muted">凭据经 DPAPI 加密存储，导出/日志中不可见。</span>
            </div>
            <div class="error-text" v-if="error">{{ error }}</div>
        </div>
    </div>
</template>
