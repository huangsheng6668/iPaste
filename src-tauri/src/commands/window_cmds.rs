// commands/window_cmds.rs — 面板/设置/放大预览窗口控制与原生拖拽
use tauri::Manager;

use crate::error::AppError;
use crate::models::{AppState, MainWindowActivation};
use crate::window::macos_panel::start_native_main_panel_drag;
use crate::window::{
    hide_main_window, show_clip_viewer_window, show_main_window, show_settings_window,
    CLIP_VIEWER_WINDOW_PREFIX, SETTINGS_WINDOW,
};

#[tauri::command]
pub(crate) fn show_panel(app: tauri::AppHandle) -> Result<(), AppError> {
    show_main_window(&app, MainWindowActivation::Activate).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn show_settings(app: tauri::AppHandle) -> Result<(), AppError> {
    show_settings_window(&app).map_err(AppError::from)
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
    window
        .hide()
        .map_err(|error| error.to_string())
        .map_err(AppError::from)
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
    window
        .destroy()
        .map_err(|error| error.to_string())
        .map_err(AppError::from)
}
