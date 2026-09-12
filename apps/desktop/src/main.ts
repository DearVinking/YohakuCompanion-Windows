import { createApp } from "vue";

import App from "./App.vue";
import { scrollbarDirective } from "./design-system/scrollbar";

import "overlayscrollbars/overlayscrollbars.css";
import "./design-system/styles/tokens.css";

if (window.__TAURI_INTERNALS__) {
  document.body.classList.add("backdrop");
}

createApp(App).directive("scrollbar", scrollbarDirective).mount("#app");
