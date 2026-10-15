// commands/clips.rs — 剪贴板历史查询/CRUD/回贴
use crate::clipboard::{
    clear_system_clipboard_after_delete, record_inserted_capture, write_clipboard_and_mark,
};
use crate::error::AppError;
use crate::models::{
    AppSnapshot, AppState, ClipPage, ClipUpdate, MainWindowActivation, SearchResult,
};
use crate::paste::paste_to_previous_app;
use crate::window::{hide_main_window, show_main_window};
use crate::CLIP_PAGE_SIZE;

use super::build_app_snapshot;

// 同步命令跑在主线程（wry IPC handler 的调用线程）：下面三个读路径是滚动/
// 搜索/启动的高频入口，DB 读取改走 spawn_blocking，避免每次分页都冻结窗口。
#[tauri::command]
pub(crate) async fn get_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<AppSnapshot, AppError> {
    let store = state.store.clone();
    let is_listening = state.capture.is_listening.clone();
    let append_copy_state = state.capture.append_copy_state.clone();
    tokio::task::spawn_blocking(move || {
        build_app_snapshot(&store, &is_listening, &append_copy_state)
    })
    .await
    .map_err(|error| AppError::internal(format!("快照任务失败：{error}")))?
}

#[tauri::command]
pub(crate) async fn list_clips(
    state: tauri::State<'_, AppState>,
    offset: Option<usize>,
    limit: Option<usize>,
    search: Option<String>,
    type_filter: Option<String>,
) -> Result<ClipPage, AppError> {
    let store = state.store.clone();
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(CLIP_PAGE_SIZE);
    let search = search.unwrap_or_default();
    let type_filter = type_filter.unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        store
            .list_clips(offset, limit, search, type_filter)
            .map_err(AppError::from)
    })
    .await
    .map_err(|error| AppError::internal(format!("列表查询任务失败：{error}")))?
}

#[tauri::command]
pub(crate) async fn search_with_fallback(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
    search: String,
    type_filter: Option<String>,
) -> Result<SearchResult, AppError> {
    let store = state.store.clone();
    let type_filter = type_filter.unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        store
            .search_with_fallback(offset, limit, &search, &type_filter)
            .map_err(AppError::from)
    })
    .await
    .map_err(|error| AppError::internal(format!("搜索任务失败：{error}")))?
}

#[tauri::command]
pub(crate) fn delete_clip(
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
pub(crate) fn clear_clips(
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
pub(crate) fn copy_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    clip_type: String,
    text: String,
) -> Result<(), AppError> {
    let captured_item = write_clipboard_and_mark(&state, &clip_type, &text)?;
    record_inserted_capture(&app, &state, captured_item)
}

#[tauri::command]
pub(crate) fn apply_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    clip_type: String,
    text: String,
) -> Result<(), AppError> {
    let captured_item = write_clipboard_and_mark(&state, &clip_type, &text)?;
    let _ = hide_main_window(&app);
    // 面板隐藏/恢复留在命令层：paste.rs 不反向依赖 window.rs（window.rs 已依赖 paste.rs）。
    if let Err(error) = paste_to_previous_app(&app, &state) {
        let _ = show_main_window(&app, MainWindowActivation::Activate);
        return Err(error);
    }
    if captured_item.is_some() {
        record_inserted_capture(&app, &state, captured_item)
    } else {
        state.store.touch_clip_captured(&id).map_err(AppError::from)
    }
}
