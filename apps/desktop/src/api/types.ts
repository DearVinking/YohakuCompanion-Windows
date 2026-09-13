import type {
    ApiError,
    AppRule,
    CoordinatorState,
    GlobalDefaults,
    Level,
    LiveDeskStatus,
    ConnectionStatusView,
    PairingResultView,
    PrivacyRules,
    Settings,
    SettingsPatch,
    PreviewView,
} from "./generated";

export type {
    ApiError,
    AppRule,
    CoordinatorState,
    GlobalDefaults,
    Level,
    LiveDeskStatus,
    ConnectionStatusView,
    PairingResultView,
    PrivacyRules,
    Settings,
    SettingsPatch,
    PreviewView,
};

export interface AppApi {
    getSettings(): Promise<Settings>;
    updateSettings(patch: SettingsPatch): Promise<Settings>;
    getPrivacyRules(): Promise<PrivacyRules>;
    updatePrivacyRules(rules: PrivacyRules): Promise<PrivacyRules>;
    getConnectionStatus(): Promise<ConnectionStatusView>;
    startPairing(serverUrl: string, pairingCode: string, deviceName: string): Promise<PairingResultView>;
    removePairing(): Promise<void>;
    enableLiveDesk(): Promise<void>;
    disableLiveDesk(): Promise<void>;
    getPreview(): Promise<PreviewView>;
    setPaused(paused: boolean): Promise<Settings>;
    quitApp(): Promise<void>;
}
