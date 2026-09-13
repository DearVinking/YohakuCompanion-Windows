<script setup lang="ts">
import { computed } from "vue";

import { IconStar } from "../icons";

const props = defineProps<{ text: string }>();

const segments = computed(() => props.text.split("★"));
</script>

<template>
  <span class="inline" role="img" :aria-label="text">
    <template v-for="(segment, index) in segments" :key="index"
      >{{ segment
      }}<IconStar
        v-if="index < segments.length - 1"
        class="mx-1 inline-block size-2.5 align-[-0.05em]"
        aria-hidden="true"
    /></template>
  </span>
</template>
