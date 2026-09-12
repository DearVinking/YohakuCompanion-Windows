<script setup lang="ts">
import { IconPlug, IconSliders, IconShield, IconHistory, IconWrench } from "./sidebarIcons";
import appIconUrl from "../assets/app-icon.png";

export interface NavItem {
    id: string;
    label: string;
    icon: typeof IconPlug;
}

defineProps<{
    navItems: NavItem[];
    activeView: string;
    appVersion: string;
}>();

defineEmits<{ navigate: [id: string] }>();
</script>

<template>
  <div class="flex items-center gap-2.5 px-1 pb-3">
    <img
      :src="appIconUrl"
      alt=""
      class="size-9 flex-none select-none"
      draggable="false"
    />
    <div class="min-w-0">
      <div class="truncate type-heading leading-tight">Yohaku Companion</div>
      <div class="truncate type-meta leading-tight text-secondary tabular-nums">
        v{{ appVersion }}
      </div>
    </div>
  </div>

  <nav aria-label="主导航" class="mt-2 flex flex-col gap-1">
    <button
      v-for="item in navItems"
      :key="item.id"
      type="button"
      class="flex h-[34px] shrink-0 cursor-pointer items-center gap-2 rounded-lg px-2.5 hover:bg-[rgba(0,0,0,.035)] dark:hover:bg-[rgba(255,255,255,.05)]"
      :class="
        activeView === item.id
          ? 'bg-[rgba(0,0,0,.047)] dark:bg-[rgba(255,255,255,.047)]'
          : ''
      "
      @click="$emit('navigate', item.id)"
    >
      <span class="flex w-5 flex-none justify-center">
        <component :is="item.icon" :size="16" />
      </span>
      <span class="truncate type-body">{{ item.label }}</span>
    </button>
  </nav>
</template>
