<script setup lang="ts">
import { ref } from "vue";
import ConnectionView from "./views/ConnectionView.vue";
import GeneralView from "./views/GeneralView.vue";
import PrivacyView from "./views/PrivacyView.vue";
import HistoryView from "./views/HistoryView.vue";
import AdvancedView from "./views/AdvancedView.vue";

const tab = ref("connection");
const tabs: Array<[string, string]> = [
    ["connection", "连接"],
    ["general", "通用"],
    ["privacy", "隐私"],
    ["history", "历史"],
    ["advanced", "高级"],
];
</script>

<template>
    <nav class="side">
        <div class="brand">Yohaku Companion</div>
        <button
            v-for="[id, label] in tabs"
            :key="id"
            :class="{ active: tab === id }"
            @click="tab = id"
        >
            {{ label }}
        </button>
        <div class="spacer" />
        <div class="muted" style="padding: 0 12px">v1.7.3</div>
    </nav>
    <main class="main">
        <ConnectionView v-if="tab === 'connection'" />
        <GeneralView v-else-if="tab === 'general'" />
        <PrivacyView v-else-if="tab === 'privacy'" />
        <HistoryView v-else-if="tab === 'history'" />
        <AdvancedView v-else />
    </main>
</template>
