use std::thread;
#[cfg(target_os = "macos")]
use std::process::Command;

use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::error::AppError;
use crate::{cloud::test_cloud_connection, CLIP_PAGE_SIZE};
use crate::clipboard::{record_inserted_capture, write_clipboard_and_mark};
use crate::events::{EVENT_LISTENING_CHANGED, ListeningChanged};
use crate::models::{
    AppInfo, AppSettings, AppSnapshot, AppState, AutomationAction, AutomationInput,
    AutomationRunDetail, AutomationRunSummary, Category, CategoryItem, CategoryWithItem,
    ClipPage, ClipUpdate, ImageOcrResult, MainWindowActivation, OcrInstallStatus,
    OcrResultPayload, ScreenshotSelection, SearchResult,
};
use crate::paste::paste_to_previous_app;
use crate::shortcut::{
    emit_settings_changed, set_app_shortcut_enabled_inner, update_registered_app_shortcut,
};
use crate::tray::{
    apply_tray_language, set_append_copy_enabled_inner, update_pause_capture_menu_label,
};
use crate::util::{clean_api_address, clean_api_key, clean_shortcut, localized_text};
use crate::window::{
    CLIP_VIEWER_WINDOW_PREFIX, SETTINGS_WINDOW, apply_main_window_layout_geometry,
    hide_main_window, show_clip_viewer_window, show_main_window, show_settings_window,
    start_native_main_panel_drag,
};
/// get_snapshot 与 sync_cloud_now 共用的 AppSnapshot 组装
/// （原两处逐行重复约 22 行）。prune_expired 统一在读取前执行。
fn build_app_snapshot(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, AppError> {
    state.store.prune_expired()?;
    let (clip_page, categories, category_items) = state.store.snapshot()?;
    let settings = state.store.settings()?;
    let is_listening = *state
        .is_listening
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?;
    let is_append_copy_enabled = state
        .append_copy_state
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .is_enabled;
    Ok(AppSnapshot {
        clips: clip_page.clips,
        has_more_clips: clip_page.has_more,
        clip_total_count: clip_page.all_count,
        categories,
        category_items,
        shortcut: settings.shortcut.clone(),
        is_listening,
        is_append_copy_enabled,
        settings,
    })
}

#[tauri::command]
pub(crate) fn get_snapshot(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, AppError> {
    build_app_snapshot(state)
}

#[tauri::command]
pub(crate) fn list_clips(
    state: tauri::State<'_, AppState>,
    offset: Option<usize>,
    limit: Option<usize>,
    search: Option<String>,
) -> Result<ClipPage, AppError> {
    state.store.list_clips(
        offset.unwrap_or(0),
        limit.unwrap_or(CLIP_PAGE_SIZE),
        search.unwrap_or_default(),
    )
    .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn search_with_fallback(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
    search: String,
) -> Result<SearchResult, AppError> {
    state.store.search_with_fallback(offset, limit, &search).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn list_categories(state: tauri::State<'_, AppState>) -> Result<Vec<Category>, AppError> {
    state.store.list_categories().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn list_category_items(state: tauri::State<'_, AppState>) -> Result<Vec<CategoryItem>, AppError> {
    state.store.list_category_items().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn reorder_categories(
    state: tauri::State<'_, AppState>,
    category_ids: Vec<String>,
) -> Result<Vec<Category>, AppError> {
    state.store.reorder_categories(category_ids).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn reorder_category_items(
    state: tauri::State<'_, AppState>,
    category_id: String,
    item_ids: Vec<String>,
) -> Result<Vec<CategoryItem>, AppError> {
    state.store.reorder_category_items(category_id, item_ids).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn create_category(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Category, AppError> {
    state.store.create_category(name, color).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn create_category_with_clip(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
    clip_id: String,
) -> Result<CategoryWithItem, AppError> {
    state.store.create_category_with_clip(name, color, clip_id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_category(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    color: String,
) -> Result<Category, AppError> {
    state.store.update_category(id, name, color).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn delete_category(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.store.delete_category(id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn add_clip_to_category(
    state: tauri::State<'_, AppState>,
    clip_id: String,
    category_id: String,
) -> Result<CategoryItem, AppError> {
    state.store.add_clip_to_category(clip_id, category_id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn remove_category_item(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.store.remove_category_item(id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn delete_clip(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.store.delete_clip(id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn clear_clips(state: tauri::State<'_, AppState>) -> Result<usize, AppError> {
    state.store.clear_clips().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn rename_clip(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    display_name: Option<String>,
) -> Result<ClipUpdate, AppError> {
    state.store.rename_clip(id, collection, display_name).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_clip_content(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    text: String,
) -> Result<ClipUpdate, AppError> {
    state.store.update_clip_content(id, collection, text).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn set_clip_pinned(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    is_pinned: bool,
) -> Result<ClipUpdate, AppError> {
    state.store.set_clip_pinned(id, collection, is_pinned).map_err(AppError::from)
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
pub(crate) fn set_listening(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, AppError> {
    *state
        .is_listening
        .lock()
        .map_err(|error| error.to_string())? = enabled;
    let _ = app.emit(
        EVENT_LISTENING_CHANGED,
        ListeningChanged {
            is_listening: enabled,
        },
    );
    update_pause_capture_menu_label(&state, enabled);
    Ok(enabled)
}

#[tauri::command]
pub(crate) fn set_append_copy_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, AppError> {
    set_append_copy_enabled_inner(&app, &state, enabled).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    retention_days: i64,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_settings(retention_days)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_append_copy_timeout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    minutes: i64,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_append_copy_timeout_minutes(minutes)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_shortcut(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: String,
) -> Result<AppSettings, AppError> {
    let shortcut = clean_shortcut(shortcut)?;
    let active_ocr_shortcut = state
        .active_ocr_shortcut
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .clone();
    crate::shortcut::ensure_shortcut_not_conflicting(&shortcut, &active_ocr_shortcut)
        .map_err(AppError::from)?;
    update_registered_app_shortcut(&app, &state, &shortcut)?;
    let settings = state.store.update_shortcut(shortcut)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_ocr_shortcut(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: String,
) -> Result<AppSettings, AppError> {
    let shortcut = clean_shortcut(shortcut)?;
    let active_panel_shortcut = state
        .active_shortcut
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .clone();
    crate::shortcut::ensure_shortcut_not_conflicting(&shortcut, &active_panel_shortcut)
        .map_err(AppError::from)?;
    crate::shortcut::update_registered_ocr_shortcut(&app, &state, &shortcut)?;
    let settings = state.store.update_ocr_shortcut(shortcut)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_app_shortcut_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, AppError> {
    set_app_shortcut_enabled_inner(&app, &state, enabled).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_panel_open_behavior(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    behavior: String,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_panel_open_behavior(behavior)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_panel_layout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    layout: String,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_panel_layout(layout)?;
    apply_main_window_layout_geometry(&app, &settings.panel_layout)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_ocr_mode(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_ocr_mode(mode)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_language(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    language: String,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_language(language)?;
    apply_tray_language(&state, &settings.language);
    if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        let _ = window.set_title(localized_text(&settings.language, "settings_title"));
    }
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_cloud_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_address: String,
    api_key: String,
) -> Result<AppSettings, AppError> {
    let settings = state.store.update_cloud_settings(api_address, api_key)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn disable_cloud_sync(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, AppError> {
    let settings = state.store.disable_cloud_sync()?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn test_cloud_settings(api_address: String, api_key: String) -> Result<bool, AppError> {
    let api_address = clean_api_address(api_address)?;
    let api_key = clean_api_key(api_key)?;
    test_cloud_connection(&api_address, &api_key)?;
    Ok(true)
}

#[tauri::command]
pub(crate) fn get_app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
pub(crate) fn get_ocr_install_status(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::install_status(&_app, &_state.store).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn install_ocr_assets(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::install_assets(_app, _state.store.clone())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn remove_ocr_assets(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::remove_assets(&_app, &_state.store).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn recognize_image_text(
    _app: tauri::AppHandle,
    image_path: String,
    profile: Option<String>,
) -> Result<ImageOcrResult, AppError> {
    crate::ocr::recognize_image(_app, image_path, profile)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn sync_cloud_now(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, AppError> {
    state.store.sync_cloud()?;
    build_app_snapshot(state)
}

#[tauri::command]
pub(crate) fn sync_cloud_in_background(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    let store = state.store.clone();
    thread::spawn(move || {
        if let Err(error) = store.sync_cloud() {
            eprintln!("background cloud sync failed: {error}");
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn show_panel(app: tauri::AppHandle) -> Result<(), AppError> {
    show_main_window(&app, MainWindowActivation::Activate).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn show_settings(app: tauri::AppHandle) -> Result<(), AppError> {
    show_settings_window(&app).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn open_clip_viewer(
    app: tauri::AppHandle,
    label: String,
    title: String,
    auto_recognize: Option<bool>,
) -> Result<(), AppError> {
    show_clip_viewer_window(&app, label, title, auto_recognize.unwrap_or(false))
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn close_clip_viewer(app: tauri::AppHandle, label: String) -> Result<(), AppError> {
    if !label.starts_with(CLIP_VIEWER_WINDOW_PREFIX) {
        return Err(AppError::internal("无效的放大窗口标签"));
    }

    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| AppError::internal("未找到放大窗口"))?;
    window.destroy().map_err(|error| error.to_string()).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn hide_panel(app: tauri::AppHandle) -> Result<(), AppError> {
    hide_main_window(&app).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn hide_settings(app: tauri::AppHandle) -> Result<(), AppError> {
    let window = app
        .get_webview_window(SETTINGS_WINDOW)
        .ok_or_else(|| AppError::internal("未找到设置窗口"))?;
    window.hide().map_err(|error| error.to_string()).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn set_main_window_dragging(
    state: tauri::State<'_, AppState>,
    dragging: bool,
) -> Result<(), AppError> {
    let mut is_dragging = state
        .is_dragging_main_window
        .lock()
        .map_err(|error| error.to_string())?;
    *is_dragging = dragging;
    Ok(())
}

#[tauri::command]
pub(crate) fn start_main_window_drag(app: tauri::AppHandle) -> Result<bool, AppError> {
    start_native_main_panel_drag(&app).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn open_accessibility_settings() -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn open_screen_recording_settings() -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn screen_capture_permission_status() -> Result<bool, AppError> {
    Ok(crate::capture::screen::has_screen_capture_permission())
}

#[tauri::command]
pub(crate) fn enable_autostart(app: tauri::AppHandle) -> Result<bool, AppError> {
    app.autolaunch()
        .enable()
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
pub(crate) fn disable_autostart(app: tauri::AppHandle) -> Result<bool, AppError> {
    app.autolaunch()
        .disable()
        .map_err(|error| error.to_string())?;
    Ok(false)
}

#[tauri::command]
pub(crate) fn is_autostart_enabled(app: tauri::AppHandle) -> Result<bool, AppError> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
        .map_err(AppError::from)
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

#[tauri::command]
pub(crate) fn list_automations(state: tauri::State<'_, AppState>) -> Result<Vec<AutomationAction>, AppError> {
    state.store.list_automations().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn create_automation(
    state: tauri::State<'_, AppState>,
    input: AutomationInput,
) -> Result<AutomationAction, AppError> {
    state.store.create_automation(input).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_automation(
    state: tauri::State<'_, AppState>,
    id: String,
    input: AutomationInput,
) -> Result<AutomationAction, AppError> {
    state.store.update_automation(&id, input).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn delete_automation(state: tauri::State<'_, AppState>, id: String) -> Result<(), AppError> {
    state.store.delete_automation(&id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn run_automation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<AutomationRunSummary, AppError> {
    let conn = state.store.connect()?;
    let action = state.store.get_automation_with_conn(&conn, &id)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn(async move { crate::automation::execute_automation(app, &store, action).await })
        .await
        .map_err(|e| format!("任务失败: {e}"))?
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn get_automation_run(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<AutomationRunDetail, AppError> {
    let conn = state.store.connect()?;
    state.store.get_automation_run_detail(&conn, &run_id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn start_screenshot_ocr(app: tauri::AppHandle) -> Result<(), AppError> {
    crate::capture::start_screenshot_ocr(&app).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn submit_screenshot_selection(
    app: tauri::AppHandle,
    selection: ScreenshotSelection,
) -> Result<(), AppError> {
    crate::capture::submit_screenshot_selection(app, selection).await
}

#[tauri::command]
pub(crate) fn cancel_screenshot_ocr(app: tauri::AppHandle) -> Result<(), AppError> {
    crate::capture::cancel_screenshot_ocr(&app).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn get_ocr_result_payload(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<OcrResultPayload, AppError> {
    state
        .ocr_result_payloads
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .remove(&token)
        .ok_or_else(|| AppError::internal("结果载荷不存在或已过期"))
}
