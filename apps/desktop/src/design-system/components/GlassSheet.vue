<script setup lang="ts">
import { X } from "@lucide/vue";
import {
  nextTick,
  onBeforeUnmount,
  onMounted,
  useId,
  useTemplateRef,
  watch,
} from "vue";

import { scrollbarDirective as vScrollbar } from "../scrollbar";
import RoundIconButton from "./RoundIconButton.vue";

defineOptions({ inheritAttrs: false });

const props = defineProps<{
  closeDisabled?: boolean;
  pageKey?: string;
  title?: string;
}>();
const emit = defineEmits<{ close: []; keydown: [event: KeyboardEvent] }>();
const titleId = useId();
const statusId = useId();
const dialog = useTemplateRef<HTMLDialogElement>("dialog");
const heading = useTemplateRef<HTMLElement>("heading");
const content = useTemplateRef<HTMLElement>("content");
let previousFocus: HTMLElement | null = null;

function focusHeading() {
  (heading.value ?? content.value)?.focus({ preventScroll: true });
}

function requestClose() {
  if (!props.closeDisabled) emit("close");
}

function onKeydown(event: KeyboardEvent) {
  emit("keydown", event);
  if (event.defaultPrevented || event.key !== "Tab" || !dialog.value) return;

  const focusable = Array.from(
    dialog.value.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, summary, [tabindex], [contenteditable="true"]',
    ),
  ).filter(
    (element) =>
      element.tabIndex >= 0 &&
      !element.matches(":disabled") &&
      !element.closest("[inert]") &&
      element.getClientRects().length > 0 &&
      getComputedStyle(element).visibility !== "hidden",
  );
  const index = focusable.indexOf(document.activeElement as HTMLElement);
  const wraps = event.shiftKey
    ? index <= 0
    : index < 0 || index === focusable.length - 1;
  if (!wraps) return;

  event.preventDefault();
  const target = event.shiftKey ? focusable.at(-1) : focusable[0];
  (target ?? heading.value ?? content.value)?.focus();
}

onMounted(() => {
  previousFocus =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  dialog.value?.showModal();
  focusHeading();
});

onBeforeUnmount(() => {
  const restoreFocus = dialog.value?.contains(document.activeElement);
  const target = previousFocus;
  dialog.value?.close();
  if (!restoreFocus || !target) return;

  // Workflow controls become enabled after Vue flushes the closing state.
  void nextTick(() => {
    const active = document.activeElement;
    if (
      !target.isConnected ||
      target.matches(":disabled") ||
      (active &&
        active !== document.body &&
        active !== document.documentElement)
    )
      return;
    target.focus({ preventScroll: true });
  });
});

watch(
  () => props.pageKey,
  () => {
    content.value?.scrollTo(0, 0);
    focusHeading();
  },
  { flush: "post" },
);
</script>

<template>
  <Teleport to="body">
    <dialog
      ref="dialog"
      v-bind="$attrs"
      class="glass-sheet"
      :aria-labelledby="title ? titleId : undefined"
      :aria-describedby="$slots.status ? statusId : undefined"
      aria-modal="true"
      @cancel.prevent="requestClose"
      @keydown.stop="onKeydown"
    >
      <div class="sheet-layout">
        <header
          class="sheet-header"
          :class="{ 'sheet-header-titleless': !title }"
        >
          <h2
            v-if="title"
            :id="titleId"
            ref="heading"
            tabindex="-1"
            class="min-w-0 flex-1 type-heading text-label outline-none"
          >
            {{ title }}
          </h2>
          <RoundIconButton
            class="flex-none"
            :disabled="closeDisabled"
            title="关闭"
            aria-label="关闭"
            @click="requestClose"
          >
            <X :size="16" aria-hidden="true" />
          </RoundIconButton>
        </header>
        <div
          v-if="$slots.status"
          :id="statusId"
          class="sheet-status type-compact text-secondary"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          <slot name="status" />
        </div>
        <div
          ref="content"
          v-scrollbar.vertical
          class="sheet-content"
          role="region"
          :aria-labelledby="title ? titleId : undefined"
          tabindex="0"
        >
          <slot />
        </div>
        <footer v-if="$slots.footer" class="sheet-footer">
          <slot name="footer" />
        </footer>
      </div>
    </dialog>
  </Teleport>
</template>

<style scoped>
.glass-sheet {
  /* Match the native sheet's 26px corner safe area plus 24px content inset. */
  --sheet-inset: 50px;

  position: fixed;
  inset: 0;
  width: min(900px, calc(100vw - 40px));
  height: min(640px, calc(100dvh - 48px));
  min-width: 0;
  max-width: none;
  max-height: none;
  margin: auto;
  padding: 0;
  overflow: hidden;
  border: 0;
  border-radius: 26px;
  color: var(--label);
  background: rgb(255 255 255 / 96%);
  backdrop-filter: blur(30px) saturate(150%);
  box-shadow:
    inset 0 0 0 1px rgb(255 255 255 / 70%),
    0 24px 70px rgb(0 0 0 / 25%),
    0 0 0 1px rgb(0 0 0 / 8%);
  animation: sheet-appear 240ms cubic-bezier(0.2, 0.8, 0.2, 1);
}

.glass-sheet::backdrop {
  background: rgb(0 0 0 / 16%);
}

.sheet-layout {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-width: 0;
}

.sheet-header {
  display: flex;
  flex: none;
  align-items: center;
  gap: 12px;
  padding: 16px var(--sheet-inset) 0;
  overflow-wrap: anywhere;
}

.sheet-header-titleless {
  justify-content: flex-end;
}

.sheet-status {
  flex: none;
  min-height: 22px;
  padding: 2px var(--sheet-inset) 0;
  overflow-wrap: anywhere;
}

.sheet-content {
  position: relative;
  flex: 1;
  min-height: 0;
  padding: 16px var(--sheet-inset) 36px;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
}

.sheet-content:focus-visible {
  outline: 2px solid var(--focus-ring);
  outline-offset: -2px;
}

.sheet-footer {
  flex: none;
  min-height: 60px;
  padding: 8px var(--sheet-inset) 20px;
}

@media (prefers-color-scheme: dark) {
  .glass-sheet {
    background: rgb(30 30 34 / 96%);
    box-shadow:
      inset 0 0 0 1px rgb(255 255 255 / 18%),
      inset 0 1px 0 rgb(255 255 255 / 6%),
      0 24px 70px rgb(0 0 0 / 50%);
  }
}

@keyframes sheet-appear {
  from {
    opacity: 0;
    transform: translateY(-8px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}

@media (max-width: 600px) {
  .glass-sheet {
    --sheet-inset: 24px;

    width: calc(100vw - 24px);
  }
}

@media (max-height: 480px) {
  .glass-sheet {
    height: calc(100dvh - 24px);
  }
}

@media (max-height: 360px) {
  .sheet-header {
    padding-top: 12px;
  }

  .sheet-content {
    padding-top: 12px;
    padding-bottom: 20px;
  }

  .sheet-footer {
    min-height: 52px;
    padding-bottom: 12px;
  }
}

@media (prefers-reduced-motion: reduce) {
  .glass-sheet {
    animation: none;
  }
}

@media (prefers-reduced-transparency: reduce) {
  .glass-sheet {
    background: #ffffff;
    backdrop-filter: none;
  }
}

@media (prefers-reduced-transparency: reduce) and (prefers-color-scheme: dark) {
  .glass-sheet {
    background: #1e1e22;
  }
}

@media (forced-colors: active) {
  .glass-sheet {
    color: CanvasText;
    background: Canvas;
    outline: 1px solid CanvasText;
    outline-offset: -1px;
    backdrop-filter: none;
    box-shadow: none;
  }

  .glass-sheet::backdrop {
    background: Canvas;
    opacity: 0.7;
  }

  .sheet-content:focus-visible {
    outline-color: Highlight;
  }
}
</style>
