// 真实 adapter：tauri-specta 生成的命令绑定（generated.ts 与 Rust 逐字节门禁）。
import { commands } from "./generated";
import type { AppApi, S3ConfigPatch, SettingsPatch } from "./types";

const toRustPatch = (p: SettingsPatch): Record<string, unknown> => ({
    shareApplications: p.shareApplications ?? null,
    shareWindowTitles: p.shareWindowTitles ?? null,
    shareMedia: p.shareMedia ?? null,
    ignoreNullArtist: p.ignoreNullArtist ?? null,
    launchAtLogin: p.launchAtLogin ?? null,
    pauseSharing: p.pauseSharing ?? null,
    preferredPlayers: p.preferredPlayers ?? null,
});

const toRustS3Patch = (p: S3ConfigPatch): Record<string, unknown> => ({
    endpoint: p.endpoint ?? null,
    bucket: p.bucket ?? null,
    region: p.region ?? null,
    customDomain: p.customDomain ?? null,
    basePath: p.basePath ?? null,
    accessKey: p.accessKey ?? null,
    secretKey: p.secretKey ?? null,
});

export const tauriApi: AppApi = {
    getSettings: () => commands.getSettings(),
    updateSettings: (patch) => commands.updateSettings(toRustPatch(patch) as never),
    getPrivacyRules: () => commands.getPrivacyRules(),
    updatePrivacyRules: (rules) => commands.updatePrivacyRules(rules as never),
    getConnectionStatus: () => commands.getConnectionStatus(),
    startPairing: (serverUrl, pairingCode, deviceName) =>
        commands.startPairing(serverUrl, pairingCode, deviceName),
    removePairing: async () => { await commands.removePairing(); },
    enableLiveDesk: async () => { await commands.enableLiveDesk(); },
    disableLiveDesk: async () => { await commands.disableLiveDesk(); },
    getPreview: () => commands.getPreview(),
    setPaused: (paused) => commands.setPaused(paused),
    listHistory: () => commands.listHistory(),
    clearHistory: async () => { await commands.clearHistory(); },
    getS3Config: () => commands.getS3Config(),
    updateS3Config: (patch) => commands.updateS3Config(toRustS3Patch(patch) as never),
    quitApp: async () => { await commands.quitApp(); },
};
