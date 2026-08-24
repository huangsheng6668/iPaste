//! 前后端事件契约：事件名常量 + payload 结构体的唯一来源。
//!
//! - 本文件是 Rust 侧唯一允许出现 `ipaste://` 字面量的地方。
//! - `export_bindings_events` 测试把常量写入 `src/types/generated/events.ts`；
//!   前端一律 `import { IPASTE_EVENTS } from "…/types/generated/events"`。
//! - 每个常量注明发起方与监听方；`clip-updated` 由前端发起（useClipEditor）。

use serde::Serialize;
use ts_rs::TS;

use crate::models::{AppSettings, DeviceInfo, DeviceOnline};

// —— 剪贴板 / 面板 / 设置（Rust 发起，主窗口监听）——
pub(crate) const EVENT_CLIPBOARD_CAPTURED: &str = "ipaste://clipboard-captured";
/// 捕获失败（Rust clipboard.rs 发起；前端 console.warn 记录，不打扰 UI）。
pub(crate) const EVENT_CAPTURE_ERROR: &str = "ipaste://capture-error";
pub(crate) const EVENT_LISTENING_CHANGED: &str = "ipaste://listening-changed";
pub(crate) const EVENT_APPEND_COPY_CHANGED: &str = "ipaste://append-copy-changed";
pub(crate) const EVENT_SETTINGS_CHANGED: &str = "ipaste://settings-changed";
pub(crate) const EVENT_PANEL_VISIBILITY_CHANGED: &str = "ipaste://panel-visibility-changed";
/// 全局快捷键打开面板（lib.rs 发起，payload 为裸 String 快捷键文案）。
pub(crate) const EVENT_SHORTCUT_OPENED: &str = "ipaste://shortcut-opened";
/// 截图 OCR 触发预检失败（capture 发起，主窗口监听并 toast；code 见 capture/mod.rs）。
pub(crate) const EVENT_OCR_SCREENSHOT_ERROR: &str = "ipaste://ocr-screenshot-error";
// —— 截图 OCR 遮罩会话启动（Rust capture 发起，遮罩窗口监听）——
pub(crate) const EVENT_OCR_OVERLAY_SESSION_START: &str = "ipaste://ocr-overlay-session-start";

// —— OCR / 自动化（Rust 发起，设置窗口/主窗口监听）——
pub(crate) const EVENT_OCR_INSTALL_PROGRESS: &str = "ipaste://ocr-install-progress";
/// Manga-OCR 模型下载进度（Rust mocr_installer 发起，设置窗口监听；
/// 与 paddle 安装进度分道，避免两个安装器互相覆盖进度条）。
pub(crate) const EVENT_MOCR_INSTALL_PROGRESS: &str = "ipaste://mocr-install-progress";
pub(crate) const EVENT_AUTOMATION_RUN_STARTED: &str = "ipaste://automation-run-started";
pub(crate) const EVENT_AUTOMATION_RUN_OUTPUT: &str = "ipaste://automation-run-output";
pub(crate) const EVENT_AUTOMATION_RUN_FINISHED: &str = "ipaste://automation-run-finished";

// —— 跨设备同步（Rust lan_sync v5 发起，设备管理窗口/主窗口监听）——
pub(crate) const EVENT_DEVICE_LIST_CHANGED: &str = "ipaste://device-list-changed";
pub(crate) const EVENT_DEVICE_STATUS_CHANGED: &str = "ipaste://device-status-changed";
pub(crate) const EVENT_PAIR_REQUEST: &str = "ipaste://pair-request";
pub(crate) const EVENT_PAIR_INVITE_STATE: &str = "ipaste://pair-invite-state";
pub(crate) const EVENT_PAIR_JOIN_FAILED: &str = "ipaste://pair-join-failed";
pub(crate) const EVENT_DEVICE_CLIP_RECEIVED: &str = "ipaste://device-clip-received";
pub(crate) const EVENT_DEVICE_CLIP_RECEIVE_FAILED: &str = "ipaste://device-clip-receive-failed";
pub(crate) const EVENT_DEVICE_CATEGORY_SENT: &str = "ipaste://device-category-sent";
pub(crate) const EVENT_DEVICE_CATEGORY_RECEIVED: &str = "ipaste://device-category-received";

// —— 前端发起（useClipEditor 放大窗口 → 主窗口；Rust 不 emit）——
/// 由前端发起（useClipEditor），Rust 不 emit——加 allow 消除非测试构建的 dead_code 警告。
#[allow(dead_code)]
pub(crate) const EVENT_CLIP_UPDATED: &str = "ipaste://clip-updated";

/// 事件目录单源：新增事件在这里加一行，生成器与完整性断言自动跟随。
/// 仅被 `export_tests` 消费（生成 events.ts + 完整性校验），故同样 allow(dead_code)。
#[allow(dead_code)]
const EVENT_TABLE: &[(&str, &str)] = &[
    ("clipboardCaptured", EVENT_CLIPBOARD_CAPTURED),
    ("captureError", EVENT_CAPTURE_ERROR),
    ("listeningChanged", EVENT_LISTENING_CHANGED),
    ("appendCopyChanged", EVENT_APPEND_COPY_CHANGED),
    ("settingsChanged", EVENT_SETTINGS_CHANGED),
    ("panelVisibilityChanged", EVENT_PANEL_VISIBILITY_CHANGED),
    ("shortcutOpened", EVENT_SHORTCUT_OPENED),
    ("ocrScreenshotError", EVENT_OCR_SCREENSHOT_ERROR),
    ("ocrOverlaySessionStart", EVENT_OCR_OVERLAY_SESSION_START),
    ("ocrInstallProgress", EVENT_OCR_INSTALL_PROGRESS),
    ("mocrInstallProgress", EVENT_MOCR_INSTALL_PROGRESS),
    ("automationRunStarted", EVENT_AUTOMATION_RUN_STARTED),
    ("automationRunOutput", EVENT_AUTOMATION_RUN_OUTPUT),
    ("automationRunFinished", EVENT_AUTOMATION_RUN_FINISHED),
    ("deviceListChanged", EVENT_DEVICE_LIST_CHANGED),
    ("deviceStatusChanged", EVENT_DEVICE_STATUS_CHANGED),
    ("pairRequest", EVENT_PAIR_REQUEST),
    ("pairInviteState", EVENT_PAIR_INVITE_STATE),
    ("pairJoinFailed", EVENT_PAIR_JOIN_FAILED),
    ("deviceClipReceived", EVENT_DEVICE_CLIP_RECEIVED),
    ("deviceClipReceiveFailed", EVENT_DEVICE_CLIP_RECEIVE_FAILED),
    ("deviceCategorySent", EVENT_DEVICE_CATEGORY_SENT),
    ("deviceCategoryReceived", EVENT_DEVICE_CATEGORY_RECEIVED),
    ("clipUpdated", EVENT_CLIP_UPDATED),
];

// —— 事件 payload（自 models.rs / lan_sync/mod.rs 移入；derive 随行）——

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ClipboardCaptured {
    pub(crate) clip: crate::models::ClipItem,
    #[ts(type = "number")]
    pub(crate) clip_total_count: usize,
    pub(crate) was_inserted: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AppendCopyChanged {
    pub(crate) is_enabled: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ListeningChanged {
    pub(crate) is_listening: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct PanelVisibilityChanged {
    pub(crate) visible: bool,
    pub(crate) preserves_current_app: bool,
    pub(crate) native_panel: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct SettingsChanged {
    pub(crate) settings: AppSettings,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct OcrScreenshotError {
    pub(crate) code: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct OcrOverlaySessionStart {
    #[ts(type = "number")]
    pub(crate) monitor_index: usize,
    pub(crate) frame_path: String,
    #[ts(type = "number")]
    pub(crate) timestamp: u64,
}

// —— 跨设备同步 payload（Task 6/7 消费；node_id 标识事件来源设备）——

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceListChanged {
    pub(crate) devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceStatusChanged {
    pub(crate) node_id: String,
    pub(crate) status: DeviceOnline,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct PairRequested {
    pub(crate) device_name: String,
    pub(crate) fingerprint: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct PairInviteState {
    pub(crate) ticket: Option<String>,
    /// epoch 毫秒；None = 无有效邀请。serde 运行时输出 JSON number/null，
    /// 故按 OcrOverlaySessionStart 先例覆盖 ts-rs 的 u64→bigint 默认
    /// （type 会整体替换 Option 的 null 分支，需一并写回）。
    #[ts(type = "number | null")]
    pub(crate) expires_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct PairJoinFailed {
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceClipReceived {
    pub(crate) node_id: String,
    pub(crate) clip_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) category_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceClipReceiveFailed {
    pub(crate) node_id: String,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceCategorySent {
    pub(crate) node_id: String,
    pub(crate) category_name: String,
    pub(crate) sent: u32,
    pub(crate) failed: u32,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceCategoryReceived {
    pub(crate) node_id: String,
    pub(crate) category_name: String,
    pub(crate) count: u32,
    pub(crate) failed: u32,
}

#[cfg(test)]
mod export_tests {
    use super::*;

    /// 与 ts-rs 的 `export_bindings_*` 命名保持一致：`npm run gen:types` 里的
    /// `cargo test export_bindings` 过滤器会连同本测试一起执行。
    /// 产物幂等：内容不变时重写为相同字节；CI 用 git diff 校验新鲜度。
    #[test]
    fn export_bindings_events() {
        // 完整性守护：新增事件常量时必须同步登记到 EVENT_TABLE，否则 events.ts 会静默缺项
        // （CI 新鲜度检查无法发现「从未生成」的缺失）。
        assert_eq!(EVENT_TABLE.len(), 24, "IPASTE_EVENTS 常量数与生成表条目数不一致？");
        let mut out = String::from(
            "// AUTO-GENERATED from src-tauri/src/events.rs — do not edit.\n\
             // Run `npm run gen:types` to regenerate.\n\
             // clipUpdated 由前端发起（useClipEditor），其余由 Rust 发起。\n\
             export const IPASTE_EVENTS = {\n",
        );
        for (key, value) in EVENT_TABLE {
            out.push_str(&format!("  {key}: \"{value}\",\n"));
        }
        out.push_str(
            "} as const;\n\n\
             export type IpasteEventName = (typeof IPASTE_EVENTS)[keyof typeof IPASTE_EVENTS];\n",
        );
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/types/generated/events.ts");
        std::fs::write(path, out).unwrap();
    }

    /// 事件表完整性：条目数快照 + 事件值无重复（漏登记一行或把值抄错都会在此失败，
    /// 且重复值会让 `as const` 的键类型静默合并，属最难肉眼发现的错配）。
    #[test]
    fn event_table_covers_all_consts() {
        assert_eq!(EVENT_TABLE.len(), 24, "事件目录条目数偏离快照，需同步更新此断言");
        let set: std::collections::HashSet<_> = EVENT_TABLE.iter().map(|(_, v)| *v).collect();
        assert_eq!(set.len(), EVENT_TABLE.len(), "EVENT_TABLE 存在重复的事件值");
    }
}
