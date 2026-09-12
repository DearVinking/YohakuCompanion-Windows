<script setup lang="ts">
import { onMounted, ref } from "vue";
import { api } from "../api";
import type { AppRule, Level, PrivacyRules } from "../api/types";

const rules = ref<PrivacyRules | null>(null);
const newKey = ref("");
const saved = ref(false);

const levelOptions: Array<[Level, string]> = [
    ["inherit", "跟随全局"],
    ["share", "允许"],
    ["hide", "隐藏"],
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

onMounted(load);
</script>

<template>
    <div v-if="rules">
        <div class="card">
            <h3>全局默认</h3>
            <div class="row"><span class="grow">应用</span></div>
            <div class="row">
                <select v-model="rules.defaults.application" @change="save">
                    <option v-for="[v, t] in levelOptions" :key="v" :value="v">{{ t }}</option>
                </select>
            </div>
            <div class="row"><span class="grow">窗口标题</span></div>
            <div class="row">
                <select v-model="rules.defaults.windowTitle" @change="save">
                    <option v-for="[v, t] in levelOptions" :key="v" :value="v">{{ t }}</option>
                </select>
            </div>
            <div class="row"><span class="grow">媒体</span></div>
            <div class="row">
                <select v-model="rules.defaults.media" @change="save">
                    <option v-for="[v, t] in levelOptions" :key="v" :value="v">{{ t }}</option>
                </select>
            </div>
            <div class="muted" style="margin-top: 8px">
                「隐藏」拥有最高优先级；逐应用规则选择「跟随全局」时使用上面的默认值。
            </div>
        </div>

        <div class="card">
            <h3>逐应用规则</h3>
            <div class="row">
                <input v-model="newKey" type="text" placeholder="应用键（exe 文件名，如 msedge.exe）" />
                <button class="ghost" @click="addRule">添加</button>
                <button class="primary" @click="save">{{ saved ? "已保存" : "保存全部" }}</button>
            </div>
            <table v-if="Object.keys(rules.apps ?? {}).length">
                <thead>
                    <tr>
                        <th>应用键</th><th>应用</th><th>标题</th><th>媒体</th><th>别名</th><th></th>
                    </tr>
                </thead>
                <tbody>
                    <tr v-for="(rule, key) in rules.apps" :key="key">
                        <td class="mono">{{ key }}</td>
                        <td>
                            <select v-model="rule.application" @change="save">
                                <option v-for="[v, t] in levelOptions" :key="v" :value="v">{{ t }}</option>
                            </select>
                        </td>
                        <td>
                            <select v-model="rule.windowTitle" @change="save">
                                <option v-for="[v, t] in levelOptions" :key="v" :value="v">{{ t }}</option>
                            </select>
                        </td>
                        <td>
                            <select v-model="rule.media" @change="save">
                                <option v-for="[v, t] in levelOptions" :key="v" :value="v">{{ t }}</option>
                            </select>
                        </td>
                        <td>
                            <input
                                type="text"
                                :value="rule.displayAlias ?? ''"
                                placeholder="显示别名"
                                @change="((rule.displayAlias = ($event.target as HTMLInputElement).value || null), save())"
                            />
                        </td>
                        <td><button class="ghost" @click="removeRule(key as string)">删除</button></td>
                    </tr>
                </tbody>
            </table>
            <div class="muted" v-else>暂无逐应用规则。</div>
        </div>
    </div>
</template>

<style scoped>
select {
    background: var(--panel-2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: 8px;
    padding: 7px 9px;
    font-size: 13px;
}
</style>
