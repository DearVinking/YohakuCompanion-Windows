// 浏览器开发用 mock：pnpm dev（无 Tauri 环境）走此 adapter。
import type { AppApi, Settings, SettingsPatch, PrivacyRules } from "./types";

const defaultSettings: Settings = {
    schemaVersion: 1,
    shareApplications: true,
    shareWindowTitles: false,
    shareMedia: true,
    ignoreNullArtist: false,
    launchAtLogin: false,
    pauseSharing: false,
    preferredPlayers: ["spotify", "cloudmusic", "qqmusic"],
};

const defaultRules: PrivacyRules = {
    schemaVersion: 1,
    defaults: { application: "share", windowTitle: "hide", media: "share" },
    apps: {},
};

let settings: Settings = { ...defaultSettings };
let rules: PrivacyRules = JSON.parse(JSON.stringify(defaultRules));
let paired = false;
let enabled = false;

const status = () => ({
    paired,
    baseUrl: paired ? "https://core.example.com" : null,
    deviceId: paired ? "11111111-1111-4111-8111-111111111111" : null,
    liveDeskEnabled: enabled,
    consentStale: false,
    coordinator: {
        state: (enabled ? "active" : "disabled") as import("./types").CoordinatorState,
        lastErrorCode: null,
        lastSentAt: enabled ? new Date().toISOString() : null,
        mediaCapable: true,
        artworkCapable: true,
        serverBaseUrl: paired ? "https://core.example.com" : null,
        deviceId: paired ? "11111111-1111-4111-8111-111111111111" : null,
    },
    appVersion: "1.7.3",
});

const delay = () => new Promise((r) => setTimeout(r, 120));

export const mockApi: AppApi = {
    getSettings: async () => ({ ...settings }),
    updateSettings: async (patch: SettingsPatch) => {
        settings = { ...settings, ...patch } as Settings;
        return { ...settings };
    },
    getPrivacyRules: async () => JSON.parse(JSON.stringify(rules)),
    updatePrivacyRules: async (next) => {
        rules = JSON.parse(JSON.stringify(next));
        return rules;
    },
    getConnectionStatus: async () => status(),
    startPairing: async () => {
        paired = true;
        return { deviceId: "11111111-1111-4111-8111-111111111111" };
    },
    removePairing: async () => {
        paired = false;
        enabled = false;
    },
    enableLiveDesk: async () => {
        enabled = true;
    },
    disableLiveDesk: async () => {
        enabled = false;
    },
    getPreview: async () => ({
        fingerprint: "mock-fingerprint",
        availability: "active",
        application: { displayName: "Microsoft Edge", windowTitle: null },
        media: {
            kind: "music",
            title: "示例歌曲",
            artist: "示例歌手",
            album: null,
            player: "网易云音乐",
            playing: true,
            positionSeconds: 42,
            durationSeconds: 201,
        },
    }),
    setPaused: async (paused) => {
        settings = { ...settings, pauseSharing: paused };
        return { ...settings };
    },
    listHistory: async () => [
        {
            id: "1",
            destination: "liveDesk",
            trigger: "semanticChange",
            state: "succeeded",
            errorCode: null,
            outputSummary: "accepted seq 42",
            startedAt: new Date().toISOString(),
            finishedAt: new Date().toISOString(),
        },
    ],
    clearHistory: async () => undefined,
    getS3Config: async () => ({
        endpoint: null,
        bucket: "",
        region: "ap-east-1",
        customDomain: null,
        basePath: "app-icons",
        accessKey: "",
        hasCredentials: false,
    }),
    updateS3Config: async (patch) => ({
        endpoint: patch.endpoint ?? null,
        bucket: patch.bucket ?? "",
        region: patch.region ?? "ap-east-1",
        customDomain: patch.customDomain ?? null,
        basePath: patch.basePath ?? "app-icons",
        accessKey: patch.accessKey ?? "",
        hasCredentials: Boolean(patch.secretKey),
    }),
    quitApp: async () => undefined,
};

const _ = delay; // 保留引用避免 noUnusedLocals
