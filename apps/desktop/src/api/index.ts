import type { AppApi } from "./types";
import { tauriApi } from "./tauri";
import { mockApi } from "./mock";

const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api: AppApi = isTauri() ? tauriApi : mockApi;

export function errorText(e: unknown): string {
    if (e && typeof e === "object" && "message" in e) {
        return String((e as { message: unknown }).message);
    }
    return String(e);
}
