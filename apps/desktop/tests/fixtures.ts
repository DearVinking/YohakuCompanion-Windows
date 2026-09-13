import type { PrivacyRules, Settings, SettingsPatch } from "../src/api/types";

export const initialSettings: Settings = {
    schemaVersion: 1,
    shareApplications: true,
    shareWindowTitles: false,
    shareMedia: true,
    ignoreNullArtist: false,
    launchAtLogin: false,
    pauseSharing: false,
    preferredPlayers: ["spotify", "cloudmusic", "qqmusic"],
};

export const emptyPatch: SettingsPatch = {
    shareApplications: null,
    shareWindowTitles: null,
    shareMedia: null,
    ignoreNullArtist: null,
    launchAtLogin: null,
    pauseSharing: null,
    preferredPlayers: null,
};

export const initialRules: PrivacyRules = {
    schemaVersion: 1,
    defaults: { application: "share", windowTitle: "hide", media: "share" },
    apps: {
        "editor.exe": {
            application: "inherit",
            windowTitle: "hide",
            media: "share",
            displayAlias: "Editor",
        },
    },
};

export function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (error: unknown) => void;
    const promise = new Promise<T>((onResolve, onReject) => {
        resolve = onResolve;
        reject = onReject;
    });
    return { promise, resolve, reject };
}
