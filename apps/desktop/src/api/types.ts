// 类型单一来源：tauri-specta 生成（generated.ts 与 Rust 逐字节门禁）。
// 本文件仅做类型重导出 + AppApi 形状定义，mock 与真实 adapter 共同实现。
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
    S3ConfigPatch,
    S3ConfigView,
    Settings,
    SettingsPatch,
    SyncEvent,
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
    S3ConfigPatch,
    S3ConfigView,
    Settings,
    SettingsPatch,
    SyncEvent,
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
    listHistory(): Promise<SyncEvent[]>;
    clearHistory(): Promise<void>;
    getS3Config(): Promise<S3ConfigView>;
    updateS3Config(patch: S3ConfigPatch): Promise<S3ConfigView>;
    quitApp(): Promise<void>;
}
