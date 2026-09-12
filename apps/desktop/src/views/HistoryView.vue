<script setup lang="ts">
import { onMounted, ref } from "vue";
import { api } from "../api";
import type { SyncEvent } from "../api/types";

const events = ref<SyncEvent[]>([]);

async function load() {
    events.value = await api.listHistory();
}

async function clear() {
    await api.clearHistory();
    await load();
}

onMounted(load);
</script>

<template>
    <div class="card">
        <h3>同步历史（本地审计，最多 1000 条）</h3>
        <div class="row">
            <button class="ghost" @click="load">刷新</button>
            <button class="ghost" @click="clear">清空</button>
        </div>
        <table v-if="events.length">
            <thead>
                <tr><th>时间</th><th>触发</th><th>状态</th><th>错误码</th><th>摘要</th></tr>
            </thead>
            <tbody>
                <tr v-for="e in events" :key="e.id">
                    <td>{{ new Date(e.finishedAt).toLocaleString() }}</td>
                    <td>{{ e.trigger }}</td>
                    <td>
                        <span :style="{ color: e.state === 'succeeded' ? 'var(--ok)' : e.state === 'failed' ? 'var(--bad)' : 'var(--warn)' }">
                            {{ e.state }}
                        </span>
                    </td>
                    <td class="mono">{{ e.errorCode ?? "-" }}</td>
                    <td>{{ e.outputSummary ?? "-" }}</td>
                </tr>
            </tbody>
        </table>
        <div class="muted" v-else>暂无记录。</div>
    </div>
</template>
