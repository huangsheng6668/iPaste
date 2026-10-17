// commands/clips.rs — 剪贴板历史查询/CRUD/回贴
// 重 DB 命令一律 async：同步命令会占用 Tauri 主线程，5k 条历史下快照/搜索
// 的整页扫描会直接卡住快捷键响应与面板唤出。
use crate::clipboard::{
    clear_system_clipboard_after_delete, record_inserted_capture, write_clipboard_and_mark,
};
use crate::error::AppError;
use crate::models::{
    AppSnapshot, AppState, ClipPage, ClipUpdate, MainWindowActivation, SearchResult,
};
use crate::paste::paste_to_target_app;
use crate::window::{hide_main_window, show_main_window};
use crate::CLIP_PAGE_SIZE;

use super::build_app_snapshot;

#[tauri::command]
pub(crate) async fn get_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<AppSnapshot, AppError> {
    build_app_snapshot(state)
}

#[tauri::command]
pub(crate) async fn list_clips(
    state: tauri::State<'_, AppState>,
    offset: Option<usize>,
    limit: Option<usize>,
    search: Option<String>,
    type_filter: Option<String>,
) -> Result<ClipPage, AppError> {
    state
        .store
        .list_clips(
            offset.unwrap_or(0),
            limit.unwrap_or(CLIP_PAGE_SIZE),
            search.unwrap_or_default(),
            type_filter.unwrap_or_default(),
        )
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn search_with_fallback(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
    search: String,
    type_filter: Option<String>,
) -> Result<SearchResult, AppError> {
    state
        .store
        .search_with_fallback(offset, limit, &search, &type_filter.unwrap_or_default())
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn delete_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let deleted_hash = state
        .store
        .delete_clip_returning_hash(id)
        .map_err(AppError::from)?;
    if let Some(hash) = deleted_hash {
        clear_system_clipboard_after_delete(&app, &state, Some(&hash));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn clear_clips(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<usize, AppError> {
    let deleted = state.store.clear_clips().map_err(AppError::from)?;
    if deleted > 0 {
        clear_system_clipboard_after_delete(&app, &state, None);
    }
    Ok(deleted)
}

#[tauri::command]
pub(crate) fn rename_clip(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    display_name: Option<String>,
) -> Result<ClipUpdate, AppError> {
    state
        .store
        .rename_clip(id, collection, display_name)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_clip_content(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    text: String,
) -> Result<ClipUpdate, AppError> {
    state
        .store
        .update_clip_content(id, collection, text)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn set_clip_pinned(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    is_pinned: bool,
) -> Result<ClipUpdate, AppError> {
    state
        .store
        .set_clip_pinned(id, collection, is_pinned)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn copy_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    clip_type: String,
    text: String,
) -> Result<(), AppError> {
    let captured_item = write_clipboard_and_mark(&state, &clip_type, &text)?;
    record_inserted_capture(&app, &state, captured_item)
}

#[tauri::command]
pub(crate) async fn apply_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    clip_type: String,
    text: String,
) -> Result<(), AppError> {
    let captured_item = write_clipboard_and_mark(&state, &clip_type, &text)?;
    let _ = hide_main_window(&app);
    // 面板隐藏/恢复留在命令层：paste.rs 不反向依赖 window.rs（window.rs 已依赖 paste.rs）。
    // 粘贴编排含 180ms 固定聚焦等待（macOS 还有 AX 轮询），移到阻塞线程池，
    // 不占异步运行时 worker；State 句柄不可跨 await，故先解析出目标应用。
    let target_app_bundle_id = state
        .window
        .target_app_bundle_id()
        .map_err(AppError::internal)?;
    let app_for_paste = app.clone();
    let paste_result = tauri::async_runtime::spawn_blocking(move || {
        paste_to_target_app(&app_for_paste, target_app_bundle_id)
    })
    .await
    .map_err(|error| AppError::internal(error.to_string()))?;
    if let Err(error) = paste_result {
        let _ = show_main_window(&app, MainWindowActivation::Activate);
        return Err(error);
    }
    if captured_item.is_some() {
        record_inserted_capture(&app, &state, captured_item)
    } else {
        state.store.touch_clip_captured(&id).map_err(AppError::from)
    }
}
