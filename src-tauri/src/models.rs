use std::sync::{Arc, Mutex};

use crate::store::Store;

use serde::{Deserialize, Serialize};
use tauri::{menu::MenuItem, Wry};
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
    pub(crate) panel_open_behavior: String,
    pub(crate) panel_layout: String,
    pub(crate) ocr_mode: String,
    pub(crate) language: String,
    pub(crate) cloud: CloudSettings,
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

pub struct AppState {
    pub store: Store,
    pub is_listening: Arc<Mutex<bool>>,
    pub show_menu_item: MenuItem<Wry>,
    pub append_copy_menu_item: MenuItem<Wry>,
    pub pause_capture_menu_item: MenuItem<Wry>,
    pub settings_menu_item: MenuItem<Wry>,
    pub quit_menu_item: MenuItem<Wry>,
    pub append_copy_state: Arc<Mutex<AppendCopyState>>,
    pub last_clipboard_change_id: Arc<Mutex<Option<u64>>>,
    pub last_clipboard_hash: Arc<Mutex<Option<String>>>,
    pub is_dragging_main_window: Arc<Mutex<bool>>,
    pub target_app_bundle_id: Arc<Mutex<Option<String>>>,
    pub main_window_activation: Arc<Mutex<MainWindowActivation>>,
    pub active_shortcut: Arc<Mutex<String>>,
    pub active_ocr_shortcut: Arc<Mutex<String>>,
    pub ocr_menu_item: MenuItem<Wry>,
    pub is_app_shortcut_enabled: Arc<Mutex<bool>>,
    #[cfg(target_os = "macos")]
    pub main_panel_state: Arc<Mutex<Option<MainPanelState>>>,
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
