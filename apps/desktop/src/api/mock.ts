import type {
    AppApi,
    ConnectionStatusView,
    CoordinatorState,
    PrivacyRules,
    Settings,
    SettingsPatch,
} from "./types";

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
let lastSentAt: string | null = null;

function status(): ConnectionStatusView {
    let state: CoordinatorState = "disabled";
    if (enabled) {
        state = settings.pauseSharing ? "suspended" : "active";
    }
    if (paired && enabled && !settings.pauseSharing) {
        lastSentAt = new Date().toISOString();
    }
    return {
        paired,
        baseUrl: paired ? "https://core.example.com" : null,
        deviceId: paired ? "11111111-1111-4111-8111-111111111111" : null,
        liveDeskEnabled: enabled,
        coordinator: {
            state,
            lastErrorCode: null,
            lastSentAt,
            mediaCapable: true,
            serverBaseUrl: paired ? "https://core.example.com" : null,
            deviceId: paired ? "11111111-1111-4111-8111-111111111111" : null,
        },
        appVersion: "1.7.3",
    };
}

export const mockApi: AppApi = {
    getSettings: async () => ({ ...settings }),
    updateSettings: async (patch: SettingsPatch) => {
        const preferredPlayers = patch.preferredPlayers
            ?.map((name) => name.replace(/^\p{White_Space}+|\p{White_Space}+$/gu, "").toLowerCase())
            .filter(Boolean);
        settings = {
            ...settings,
            shareApplications: patch.shareApplications ?? settings.shareApplications,
            shareWindowTitles: patch.shareWindowTitles ?? settings.shareWindowTitles,
            shareMedia: patch.shareMedia ?? settings.shareMedia,
            ignoreNullArtist: patch.ignoreNullArtist ?? settings.ignoreNullArtist,
            launchAtLogin: patch.launchAtLogin ?? settings.launchAtLogin,
            pauseSharing: patch.pauseSharing ?? settings.pauseSharing,
            preferredPlayers: preferredPlayers?.length ? preferredPlayers : settings.preferredPlayers,
        };
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
        enabled = false;
        lastSentAt = null;
        return { deviceId: "11111111-1111-4111-8111-111111111111" };
    },
    removePairing: async () => {
        paired = false;
        enabled = false;
        lastSentAt = null;
    },
    enableLiveDesk: async () => {
        if (!paired) throw { code: "NOT_PAIRED" };
        enabled = true;
    },
    disableLiveDesk: async () => {
        enabled = false;
    },
    getPreview: async () => ({
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
    quitApp: async () => undefined,
};
