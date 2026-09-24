// commands/settings.rs — 应用设置/快捷键/语言/自启动/权限状态
#[cfg(target_os = "macos")]
use std::process::Command;

use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::error::AppError;
use crate::events::{ListeningChanged, EVENT_LISTENING_CHANGED};
use crate::models::{AppInfo, AppSettings, AppState};
use crate::shortcut::{
    emit_settings_changed, set_app_shortcut_enabled_inner, update_registered_app_shortcut,
};
use crate::tray::{
    apply_tray_language, set_append_copy_enabled_inner, update_pause_capture_menu_label,
};
use crate::util::{clean_shortcut, localized_text};
use crate::window::{apply_main_window_layout_geometry, SETTINGS_WINDOW};

use super::apply_settings_update;

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
    apply_settings_update(&app, state.store.update_settings(retention_days))
}

#[tauri::command]
pub(crate) fn update_append_copy_timeout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    minutes: i64,
) -> Result<AppSettings, AppError> {
    apply_settings_update(
        &app,
        state.store.update_append_copy_timeout_minutes(minutes),
    )
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
    apply_settings_update(&app, state.store.update_shortcut(shortcut))
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
    apply_settings_update(&app, state.store.update_ocr_shortcut(shortcut))
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
    apply_settings_update(&app, state.store.update_panel_open_behavior(behavior))
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
    apply_settings_update(&app, state.store.update_ocr_mode(mode))
}

#[tauri::command]
pub(crate) fn update_ocr_engine(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    engine: String,
) -> Result<AppSettings, AppError> {
    apply_settings_update(&app, state.store.update_ocr_engine(engine))
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
pub(crate) fn get_app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
    }
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
pub(crate) fn accessibility_permission_status() -> Result<bool, AppError> {
    #[cfg(target_os = "macos")]
    {
        Ok(crate::paste::ax_is_process_trusted())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(true)
    }
}
