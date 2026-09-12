import type { ObjectDirective } from "vue";

import { OverlayScrollbars } from "overlayscrollbars";

type ScrollbarAxis = "horizontal" | "vertical";

function axisFromModifiers(
  modifiers: Partial<Record<string, boolean>>,
): ScrollbarAxis {
  return modifiers.horizontal ? "horizontal" : "vertical";
}

function optionsFor(axis: ScrollbarAxis) {
  return {
    overflow: {
      x: axis === "horizontal" ? ("scroll" as const) : ("hidden" as const),
      y: axis === "vertical" ? ("scroll" as const) : ("hidden" as const),
    },
    scrollbars: {
      autoHide: "leave" as const,
      autoHideDelay: 0,
      autoHideSuspend: false,
      clickScroll: "instant" as const,
      theme: "os-theme-nteye",
    },
  };
}

export const scrollbarDirective: ObjectDirective<HTMLElement> = {
  beforeMount(element) {
    element.setAttribute("data-overlayscrollbars-initialize", "");
  },
  beforeUnmount(element) {
    OverlayScrollbars(element)?.destroy();
  },
  mounted(element, binding) {
    const axis = axisFromModifiers(binding.modifiers);
    element.dataset.scrollbarAxis = axis;
    OverlayScrollbars(
      {
        elements: { viewport: element },
        target: element,
      },
      optionsFor(axis),
    );
  },
};
