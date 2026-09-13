import type { CoordinatorState } from "./api/types";

const connectionStates: Record<CoordinatorState, string> = {
    active: "已连接",
    connecting: "正在连接…",
    degraded: "同步失败，等待重试",
    updateRequired: "请更新 Yohaku Companion",
    serverFeatureUnavailable: "服务器暂不支持 Live Desk",
    suspended: "已暂停同步",
    disabled: "未开启同步",
};

export function connectionStateText(state?: CoordinatorState): string {
    return state ? connectionStates[state] ?? "暂时无法读取状态" : "正在读取状态…";
}

const incompatibleVersion = "应用与服务器版本不兼容，请更新 Yohaku Companion 或联系服务器管理员。";
const unavailableFeature = "服务器暂不支持 Live Desk，请检查服务器版本和设置。";
const failedOperation = "操作未完成，请稍后重试。";

const errorMessages: Record<string, string> = {
    NOT_PAIRED: "请先在「同步」页面完成配对。",
    LIVE_DESK_DISABLED: "同步尚未开启，请点击右上角的同步按钮。",
    NEGOTIATION_FAILED: "无法连接到 Live Desk，请检查服务器设置。",
    TRANSPORT: "网络连接失败，请检查网络和服务器地址。",
    CAPABILITIES_UNAVAILABLE: "无法连接到服务器，请检查服务器地址和网络连接。",
    UPDATE_REQUIRED: "请更新 Yohaku Companion 后再连接。",
    SCHEMA_UNSUPPORTED: incompatibleVersion,
    COMPANION_SCHEMA_UNSUPPORTED: incompatibleVersion,
    SCHEMA_REJECTED: incompatibleVersion,
    FEATURE_UNAVAILABLE: unavailableFeature,
    COMPANION_FEATURE_UNAVAILABLE: unavailableFeature,
    INVALID_CAPABILITIES: "无法读取服务器配置，请检查服务器地址或稍后重试。",
    COMPANION_PAIRING_EXPIRED: "配对码已过期，请在 Yohaku 中重新生成。",
    VALIDATION_FAILED: "请检查配对码和设备名称后重试。",
    MISSING_SCOPE: "这次配对没有同步权限，请在 Yohaku 中重新生成配对码。",
    MALFORMED: "服务器返回的信息无法识别，请检查服务器地址和版本。",
    RATE_LIMITED: "操作太频繁，请稍等片刻再试。",
    PAYLOAD_TOO_LARGE: "同步内容超过服务器限制，请减少同步内容后重试。",
    DECODE: "无法读取服务器的回复，请稍后重试。",
    SERVER: "服务器未能完成操作，请稍后重试。",
    HTTP_ERROR: "服务器未能完成操作，请稍后重试。",
    STORE: "无法读取或保存本机数据，请检查磁盘空间和文件权限。",
    INTERNAL: failedOperation,
    INTERNAL_ERROR: failedOperation,
};

export function errorCodeText(code: string, fallback?: string): string {
    return Object.hasOwn(errorMessages, code) ? errorMessages[code] : fallback || failedOperation;
}
