use crate::store::Store;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy)]
pub(crate) struct WindowGeometry {
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) min_width: f64,
    pub(crate) min_height: f64,
    pub(crate) max_width: Option<f64>,
    pub(crate) max_height: Option<f64>,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[allow(non_snake_case)]
pub(crate) struct ProcessSerialNumber {
    pub(crate) highLongOfPSN: u32,
    pub(crate) lowLongOfPSN: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MainWindowActivation {
    Activate,
    PreserveCurrentApp,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ClipItem {
    pub(crate) id: String,
    pub(crate) clip_type: String,
    pub(crate) content_hash: String,
    pub(crate) display_name: Option<String>,
    pub(crate) preview_text: String,
    pub(crate) text: String,
    pub(crate) source_app: Option<String>,
    pub(crate) last_captured_at: String,
    #[ts(type = "number")]
    pub(crate) favorite_count: i64,
    pub(crate) is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct Category {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) color: String,
    #[ts(type = "number")]
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CategoryItem {
    pub(crate) id: String,
    pub(crate) category_id: String,
    pub(crate) clip_snapshot_id: String,
    pub(crate) clip_type: String,
    pub(crate) content_hash: String,
    pub(crate) display_name: Option<String>,
    pub(crate) preview_text: String,
    pub(crate) text: String,
    #[ts(type = "number")]
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) sync_state: String,
    pub(crate) is_pinned: bool,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CategoryWithItem {
    pub(crate) category: Category,
    pub(crate) item: CategoryItem,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(untagged)]
#[ts(export)]
pub(crate) enum ClipUpdate {
    Clip(ClipItem),
    CategoryItem(CategoryItem),
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AppSnapshot {
    pub(crate) clips: Vec<ClipItem>,
    pub(crate) has_more_clips: bool,
    #[ts(type = "number")]
    pub(crate) clip_total_count: usize,
    pub(crate) categories: Vec<Category>,
    pub(crate) category_items: Vec<CategoryItem>,
    pub(crate) shortcut: String,
    pub(crate) is_listening: bool,
    pub(crate) is_append_copy_enabled: bool,
    pub(crate) settings: AppSettings,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AppInfo {
    pub(crate) version: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ClipPage {
    pub(crate) clips: Vec<ClipItem>,
    pub(crate) has_more: bool,
    #[ts(type = "number")]
    pub(crate) total_count: usize,
    #[ts(type = "number")]
    pub(crate) all_count: usize,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CategoryHitGroup {
    pub(crate) category: Category,
    pub(crate) items: Vec<CategoryItem>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export)]
pub(crate) enum SearchResult {
    History { page: ClipPage },
    CategoryHits { groups: Vec<CategoryHitGroup> },
}

/// 主面板布局。线格式与历史 DB 字符串逐字一致（Task 40 枚举化）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) enum PanelLayout {
    #[serde(rename = "top")]
    Top,
    #[serde(rename = "side")]
    Side,
}

impl PanelLayout {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Side => "side",
        }
    }
    /// DB 历史字符串解析；非法值由调用方回落默认。
    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "top" => Some(Self::Top),
            "side" => Some(Self::Side),
            _ => None,
        }
    }
    pub(crate) fn default_value() -> Self {
        Self::Top
    }
}

/// 本地 OCR 识别模式（Paddle 快速/精确）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) enum OcrMode {
    #[serde(rename = "fast")]
    Fast,
    #[serde(rename = "best")]
    Best,
}

impl OcrMode {
    /// macOS 上调用方（Paddle 安装器/识别管线）被 cfg 裁掉，仅测试引用；
    /// allow(dead_code) 消除平台性警告（与 tokens.rs 同法）。
    #[allow(dead_code)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Best => "best",
        }
    }
    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "fast" => Some(Self::Fast),
            "best" => Some(Self::Best),
            _ => None,
        }
    }
    pub(crate) fn default_value() -> Self {
        Self::Fast
    }
}

/// OCR 引擎（本地 / OpenAI 兼容云端）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) enum OcrEngine {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "openai")]
    Openai,
}

impl OcrEngine {
    /// 生产调用方 cloud_ocr_engine_ready 只被非 macOS 的 preflight 分支使用，
    /// macOS 上仅测试引用；allow(dead_code) 消除平台性警告。
    #[allow(dead_code)]
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Openai => "openai",
        }
    }
    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "openai" => Some(Self::Openai),
            _ => None,
        }
    }
    pub(crate) fn default_value() -> Self {
        Self::Local
    }
}

/// 面板唤出后的默认落点。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) enum PanelOpenBehavior {
    #[serde(rename = "history")]
    History,
    #[serde(rename = "last_selected")]
    LastSelected,
}

// 注意：PanelOpenBehavior 只有解码方向（DB 字符串 → 枚举），没有 as_str：
// 写入侧仍走 registry 的 clean_panel_open_behavior（Task 40 有意保留 String 写签名），
// 非测试代码里不存在枚举 → 字符串的转换点，留着就是死代码。
impl PanelOpenBehavior {
    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "history" => Some(Self::History),
            "last_selected" => Some(Self::LastSelected),
            _ => None,
        }
    }
    pub(crate) fn default_value() -> Self {
        Self::History
    }
}

/// 界面语言（与 i18n locale 码一致）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub(crate) enum Language {
    #[serde(rename = "en")]
    En,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "ja")]
    Ja,
    #[serde(rename = "ko")]
    Ko,
    #[serde(rename = "es")]
    Es,
    #[serde(rename = "fr")]
    Fr,
    #[serde(rename = "de")]
    De,
}

impl Language {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::ZhCn => "zh-CN",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Es => "es",
            Self::Fr => "fr",
            Self::De => "de",
        }
    }
    pub(crate) fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "en" => Some(Self::En),
            "zh-CN" => Some(Self::ZhCn),
            "ja" => Some(Self::Ja),
            "ko" => Some(Self::Ko),
            "es" => Some(Self::Es),
            "fr" => Some(Self::Fr),
            "de" => Some(Self::De),
            _ => None,
        }
    }
    pub(crate) fn default_value() -> Self {
        Self::En
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AppSettings {
    pub(crate) shortcut: String,
    pub(crate) ocr_shortcut: String,
    #[ts(type = "number")]
    pub(crate) retention_days: i64,
    #[ts(type = "number")]
    pub(crate) append_copy_timeout_minutes: i64,
    pub(crate) panel_open_behavior: PanelOpenBehavior,
    pub(crate) panel_layout: PanelLayout,
    pub(crate) ocr_mode: OcrMode,
    pub(crate) ocr_engine: OcrEngine,
    pub(crate) language: Language,
    pub(crate) cloud: CloudSettings,
    pub(crate) cloud_ocr: CloudOcrSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CloudSettings {
    pub(crate) api_address: String,
    pub(crate) api_key: String,
    pub(crate) enabled: bool,
    pub(crate) last_connected_at: Option<String>,
}

/// 单条云 OCR 提示词消息（OpenAI 兼容 /chat/completions 消息体）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CloudOcrPromptMessage {
    pub(crate) role: String,
    pub(crate) content: String,
}

pub(crate) fn default_openai_ocr_prompts() -> Vec<CloudOcrPromptMessage> {
    vec![
        CloudOcrPromptMessage {
            role: "system".to_string(),
            content: "You are an OCR engine.\nRecognition language: {recognition_language}\nExtract all readable visible text from the user's image. Return the recognized text only. Preserve line breaks and reading order. If no text is present, return an empty response. Do not describe the image or add explanations.".to_string(),
        },
        CloudOcrPromptMessage {
            role: "user".to_string(),
            content: "Recognize all text in this image. Recognition language: {recognition_language}.".to_string(),
        },
    ]
}

/// 云 OCR 配置：通用 OpenAI 兼容接口（Base URL + 模型 + Key + 自定义 Prompt 列表），
/// 任意兼容厂商可接。Key 存系统凭据库（settings 列空串占位），仅在读取设置时回填
/// 给前端展示；Base URL、模型名与 Prompt 列表为普通 KV。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CloudOcrSettings {
    pub(crate) openai_base_url: String,
    pub(crate) openai_model: String,
    pub(crate) openai_api_key: String,
    #[serde(default = "default_openai_ocr_prompts")]
    pub(crate) openai_prompts: Vec<CloudOcrPromptMessage>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct OcrInstallStatus {
    pub(crate) installed: bool,
    pub(crate) engine_id: String,
    pub(crate) engine_version: Option<String>,
    pub(crate) mode: String,
    pub(crate) platform: String,
    pub(crate) manifest_url: String,
    pub(crate) install_dir: String,
    #[ts(type = "number")]
    pub(crate) downloaded_bytes: u64,
    #[ts(type = "number")]
    pub(crate) total_bytes: u64,
    pub(crate) missing_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct OcrInstallProgress {
    pub(crate) phase: String,
    pub(crate) file_name: Option<String>,
    #[ts(type = "number")]
    pub(crate) downloaded_bytes: u64,
    #[ts(type = "number")]
    pub(crate) total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ImageOcrResult {
    pub(crate) text: String,
    pub(crate) engine: String,
    pub(crate) language: String,
    pub(crate) words: Vec<ImageOcrWord>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ImageOcrWord {
    pub(crate) text: String,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) confidence: f64,
    #[ts(type = "number")]
    pub(crate) block_index: i64,
    #[ts(type = "number")]
    pub(crate) paragraph_index: i64,
    #[ts(type = "number")]
    pub(crate) line_index: i64,
    #[ts(type = "number")]
    pub(crate) word_index: i64,
}

/// 截图 OCR：遮罩窗提交的框选区域（显示器内 CSS 逻辑像素，方向无关归一化前）。
#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct ScreenshotSelection {
    #[ts(type = "number")]
    pub(crate) monitor_index: usize,
    pub(crate) left: f64,
    pub(crate) top: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

/// 截图 OCR：结果窗凭 token 读取的载荷（AppState 内单读即删）。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct OcrResultPayload {
    pub(crate) image_path: String,
    pub(crate) item_id: String,
    #[ts(type = "number")]
    pub(crate) monitor_index: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OcrManifest {
    pub(crate) engine: OcrManifestEngine,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OcrManifestEngine {
    pub(crate) id: String,
    pub(crate) version: String,
    pub(crate) platform: String,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    pub(crate) base_url: String,
    pub(crate) files: Vec<OcrManifestFile>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OcrManifestFile {
    pub(crate) role: String,
    pub(crate) name: String,
    pub(crate) path: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    /// 可选绝对下载 URL 覆盖（manga-ocr 主权重 445MB 超 GitHub Pages 单文件
    /// 100MB 上限，直接指向 Release 扁平资产；缺省时用 engine.base_url + path）。
    #[serde(default)]
    pub(crate) url: Option<String>,
    #[serde(default)]
    pub(crate) archive: Option<String>,
    #[serde(default)]
    pub(crate) install_dir: Option<String>,
    #[serde(default)]
    pub(crate) entries: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct CloudSnapshot {
    pub(crate) categories: Vec<Category>,
    pub(crate) category_items: Vec<CategoryItem>,
    #[serde(default)]
    pub(crate) deleted_category_ids: Vec<String>,
    #[serde(default)]
    pub(crate) deleted_category_item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudPushPayload {
    pub(crate) categories: Vec<Category>,
    pub(crate) category_items: Vec<CategoryItem>,
    pub(crate) deleted_category_ids: Vec<String>,
    pub(crate) deleted_category_item_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CloudEnvelope<T> {
    pub(crate) ok: Option<bool>,
    pub(crate) error: Option<String>,
    #[serde(flatten)]
    pub(crate) data: T,
}

#[derive(Debug, Deserialize)]
pub(crate) struct HealthPayload {
    pub(crate) service: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Tombstone {
    pub(crate) entity: String,
    pub(crate) entity_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CapturedClipboardItem {
    pub(crate) clip_type: String,
    pub(crate) content_hash: String,
    pub(crate) preview_text: String,
    pub(crate) text: String,
    pub(crate) image_bytes: Option<Vec<u8>>,
    /// 条目的重命名显示名；本地捕获恒为 `None`，LAN 接收侧可能携带对端重命名。
    pub(crate) display_name: Option<String>,
}

pub(crate) enum ClipboardRead {
    Empty,
    Occupied,
    Item(CapturedClipboardItem),
}

#[derive(Debug, Default)]
pub(crate) struct AppendCopyState {
    pub(crate) is_enabled: bool,
    pub(crate) clip_id: Option<String>,
    pub(crate) session_id: Option<String>,
    pub(crate) text: String,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct MainPanelState {
    pub(crate) panel: usize,
    pub(crate) visible: bool,
}

/// 应用级共享状态：组合根（Task 27 起按域聚合）。
///
/// 每个域结构体只承载状态句柄，锁的粒度与获取顺序与聚合前完全一致；
/// 命令层仍以 `state.<domain>.<field>` 访问。
pub struct AppState {
    pub store: Store,
    /// 剪贴板捕获域：监听开关 / 追加复制会话 / 去重光标。
    pub capture: crate::state::CaptureState,
    /// 托盘菜单句柄。
    pub ui: crate::state::UiHandles,
    /// 全局快捷键域。
    pub shortcuts: crate::state::ShortcutState,
    /// 窗口域：拖动抑制、目标应用、激活方式、macOS 面板缓存。
    pub window: crate::state::WindowState,
    /// OCR 域：截图会话与结果载荷缓存。
    pub ocr: crate::state::OcrRuntime,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_result_history_serializes_with_kind_tag() {
        let page = ClipPage {
            clips: vec![],
            has_more: false,
            total_count: 0,
            all_count: 0,
        };
        let json = serde_json::to_string(&SearchResult::History { page }).unwrap();
        assert!(json.contains(r#""kind":"history""#), "got: {json}");
    }

    #[test]
    fn search_result_category_hits_serializes_with_kind_tag() {
        let res = SearchResult::CategoryHits { groups: vec![] };
        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains(r#""kind":"categoryHits""#), "got: {json}");
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AutomationAction {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) cwd: Option<String>,
    pub(crate) run_mode: String,
    pub(crate) confirm_before_run: bool,
    pub(crate) close_panel_on_success: bool,
    #[ts(type = "number")]
    pub(crate) sort_order: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
    pub(crate) last_run: Option<AutomationRunSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AutomationRunSummary {
    pub(crate) id: String,
    pub(crate) status: String,
    #[ts(type = "number | null")]
    pub(crate) exit_code: Option<i64>,
    pub(crate) started_at: String,
    pub(crate) finished_at: Option<String>,
    #[ts(type = "number | null")]
    pub(crate) duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AutomationRunDetail {
    pub(crate) id: String,
    pub(crate) automation_id: String,
    pub(crate) status: String,
    #[ts(type = "number | null")]
    pub(crate) exit_code: Option<i64>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) stdout_truncated: bool,
    pub(crate) stderr_truncated: bool,
    pub(crate) started_at: String,
    pub(crate) finished_at: Option<String>,
    #[ts(type = "number | null")]
    pub(crate) duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AutomationInput {
    pub(crate) name: String,
    pub(crate) command: String,
    pub(crate) cwd: Option<String>,
    pub(crate) confirm_before_run: bool,
    pub(crate) close_panel_on_success: bool,
}

// —— 跨设备同步：信任设备（lan_sync v5）——

/// 每设备自动同步偏好（Spec 2 消费；默认文本类自动）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum AutoSyncMode {
    TextOnly,
    All,
    Off,
}

/// 自动推送全局设置（settings KV `sync_auto_push_master/notify`；缺省 true/false）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct AutoPushSettings {
    pub(crate) master: bool,
    pub(crate) notify: bool,
}

/// paired_devices 表的行模型。node_id 为 EndpointId 的 hex（64 字符）。
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct PairedDevice {
    pub(crate) node_id: String,
    pub(crate) device_name: String,
    pub(crate) relay_url: Option<String>,
    /// 直连地址线索（ip:port），JSON 数组存库。
    pub(crate) direct_addrs: Vec<String>,
    pub(crate) auto_sync_mode: AutoSyncMode,
    pub(crate) added_at: String,
    pub(crate) last_seen_at: Option<String>,
    pub(crate) revoked_at: Option<String>,
}

/// 设备在线状态（DeviceLinkRegistry 的运行态，不入库）。
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub(crate) enum DeviceOnline {
    Offline,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct DeviceInfo {
    pub(crate) device: PairedDevice,
    pub(crate) online: DeviceOnline,
}
