import { commands } from "./generated";
import type { AppApi, SettingsPatch } from "./types";

function toRustPatch(p: SettingsPatch): SettingsPatch {
    return {
        shareApplications: p.shareApplications ?? null,
        shareWindowTitles: p.shareWindowTitles ?? null,
        shareMedia: p.shareMedia ?? null,
        ignoreNullArtist: p.ignoreNullArtist ?? null,
        launchAtLogin: p.launchAtLogin ?? null,
        pauseSharing: p.pauseSharing ?? null,
        preferredPlayers: p.preferredPlayers ?? null,
    };
}

export const tauriApi: AppApi = {
    getSettings: () => commands.getSettings(),
    updateSettings: (patch) => commands.updateSettings(toRustPatch(patch)),
    getPrivacyRules: () => commands.getPrivacyRules(),
    updatePrivacyRules: (rules) => commands.updatePrivacyRules(rules),
    getConnectionStatus: () => commands.getConnectionStatus(),
    startPairing: (serverUrl, pairingCode, deviceName) =>
        commands.startPairing(serverUrl, pairingCode, deviceName),
    removePairing: async () => { await commands.removePairing(); },
    enableLiveDesk: async () => { await commands.enableLiveDesk(); },
    disableLiveDesk: async () => { await commands.disableLiveDesk(); },
    getPreview: () => commands.getPreview(),
    setPaused: (paused) => commands.setPaused(paused),
    quitApp: async () => { await commands.quitApp(); },
};
