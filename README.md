# Yohaku Companion for Windows

一个 Windows 原生托盘应用：把**当前前台应用**与**正在播放的音乐**按 Companion Protocol v2 同步到 Yohaku Core 的 Live Desk。应用图标取自 macOS 原版的 AppIcon.appiconset（1024×1024 源，脚本生成多尺寸 ICO/PNG）。

## 架构

Cargo virtual workspace + Tauri 2 / Vue 3 壳（架构蓝本来自 NTEye）：

```
apps/desktop/
├── src/                   # Vue 3 + TS：AppApi 统一边界 + mock adapter（浏览器可跑）
└── src-tauri/             # Tauri 壳：薄命令、托盘、绑定逐字节门禁
crates/
├── yohaku-protocol/       # Companion Protocol v2 纯逻辑（forbid(unsafe)）
├── yohaku-store/          # JSON 原子持久化：设置/规则/连接元数据/历史（forbid(unsafe)）
├── yohaku-app/            # 平台无关：协调器/隐私管道/S3 SigV4/端口（forbid(unsafe)）
└── yohaku-platform/       # Windows 捕获：前台窗口/GSMTC 媒体/电源锁屏/DPAPI
```

数据流：平台捕获事件（mpsc）→ 单协调器线程（合并、限速、心跳）→ 隐私管道净化 → 可选 S3 资产上传 → PUT `/companion/presence`；状态快照由前端 1s 轮询。

## 协议要点（与 macOS 版逐项对齐）

- 端点：`GET /companion/capabilities`、`POST /companion/pairings/claim`、`PUT /companion/presence`、`POST /companion/presence/clear`（Bearer + `X-Yohaku-Companion-Version: 1.7.3`）。
- JSON 全键排序；「键存在但为 null」与「键缺失」是两种语义（encodeNullable / decodeRequiredNullable 语义对齐）；日期 RFC3339 UTC 毫秒。
- 序列号发送前先持久化（崩溃只留合法空洞）；歧义传输失败以**同一 sequence/requestId/body** 立即重试一次；`acceptedSequence` 单调 reconcile。
- 能力协商：semver 比较、schema v2、`liveDesk`/`mediaTimeline`/`mediaArtwork` 特性、limits 数值校验（含 heartbeat ∈ [leaseMin, leaseMax]）；`COMPANION_SCHEMA_UNSUPPORTED` / `COMPANION_FEATURE_UNAVAILABLE` / HTTP 426 → 丢弃 authority 重新协商。
- 睡眠/锁屏/暂停/关机 → best-effort 清除（500ms 超时，租约过期兜底）。
- 配对成功后 Live Desk **默认关闭**；开启必须先查看净化预览（10 分钟内有效），隐私策略指纹变化即作废同意。

## 构建

依赖：Rust（`rust-toolchain.toml` 固定 1.98.1，rustup 会自动安装）、Microsoft C++ Build Tools、Node 22 + pnpm、WebView2 Runtime（Win11 自带）。

```bash
# Rust 侧
cargo check
cargo test --workspace        # 99 个测试
cargo clippy --workspace --all-targets

# 前端（apps/desktop）
pnpm install
pnpm build                    # vue-tsc --noEmit && vite build

# 生成/校验 TS 绑定（generated.ts 有逐字节门禁）
cd apps/desktop/src-tauri
UPDATE_BINDINGS=1 cargo test --test bindings   # 重新生成
cargo test --test bindings                     # 仅校验

# 开发运行
pnpm tauri dev                # apps/desktop 下

# 打包
pnpm tauri build
```

## 数据与安全

- 数据目录 `%APPDATA%\YohakuCompanion\`（debug 构建 `YohakuCompanion.debug\`，两者完全隔离）。
- deviceToken 与 S3 SecretKey 经 **DPAPI**（CurrentUser）加密，普通 JSON 不含任何凭据。
- 原始捕获值（exe 路径、applicationKey、原始封面字节）不进入净化快照、日志与历史。
- 历史是有界投递审计（≤1000 条，裁最旧），只含触发原因/状态/固定错误码/安全摘要。
- 网络仅访问用户配置的 Yohaku Core 服务器与用户自己的 S3 兼容存储。

## 已知限制

- GSMTC 只能看到接入 SMTC 的播放器；封面（Thumbnail）可用性因应用而异，缺失时自动降级纯文本 Presence（协议本就支持）。
- `GetWindowTextW` 对部分 UWP/提权窗口返回空 → 标题按「未采集」处理（与 macOS 无辅助功能权限时语义一致）。
- QQ 音乐/网易云的「播放链接」白名单能力暂未实现（macOS 版读取其本地数据库；Windows 侧生态不同），协议侧链接键保持缺失语义。
- 媒体位置由采样值 + 速率外推；不支持时间线的会话位置为 null（不上报位置）。

## 许可

MIT
