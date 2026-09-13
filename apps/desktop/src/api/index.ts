import type { AppApi } from "./types";
import { tauriApi } from "./tauri";
import { mockApi } from "./mock";
import { errorCodeText } from "../uiText";

const isTauri = () =>
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const api: AppApi = isTauri() ? tauriApi : mockApi;

export function errorText(e: unknown): string {
    if (e && typeof e === "object" && "code" in e && typeof e.code === "string") {
        const message = "message" in e && typeof e.message === "string" ? e.message.trim() : "";
        if (e.code === "VALIDATION_FAILED" && message) {
            return message;
        }
        return errorCodeText(e.code, message);
    }
    return errorCodeText("INTERNAL");
}
