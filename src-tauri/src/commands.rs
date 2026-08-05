#[cfg(not(target_os = "macos"))]
use std::fs;
use std::thread;
#[cfg(target_os = "macos")]
use std::process::Command;

use rusqlite::params;
use tauri::{Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::clipboard::*;
use crate::models::*;
use crate::ocr::*;
use crate::paste::*;
use crate::shortcut::*;
use crate::tray::*;
use crate::util::*;
use crate::window::*;
use crate::{test_cloud_connection, CLIP_PAGE_SIZE};
#[tauri::command]
pub(crate) fn get_snapshot(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, String> {
    state.store.prune_expired()?;
    let (clip_page, categories, category_items) = state.store.snapshot()?;
    let settings = state.store.settings()?;
    Ok(AppSnapshot {
        clips: clip_page.clips,
        has_more_clips: clip_page.has_more,
        clip_total_count: clip_page.all_count,
        categories,
        category_items,
        shortcut: settings.shortcut.clone(),
        is_listening: *state
            .is_listening
            .lock()
            .map_err(|error| error.to_string())?,
        is_append_copy_enabled: state
            .append_copy_state
            .lock()
            .map(|value| value.is_enabled)
            .map_err(|error| error.to_string())?,
        settings,
    })
}

#[tauri::command]
pub(crate) fn list_clips(
    state: tauri::State<'_, AppState>,
    offset: Option<usize>,
    limit: Option<usize>,
    search: Option<String>,
) -> Result<ClipPage, String> {
    state.store.list_clips(
        offset.unwrap_or(0),
        limit.unwrap_or(CLIP_PAGE_SIZE),
        search.unwrap_or_default(),
    )
}

#[tauri::command]
pub(crate) fn search_with_fallback(
    state: tauri::State<'_, AppState>,
    offset: usize,
    limit: usize,
    search: String,
) -> Result<SearchResult, String> {
    state.store.search_with_fallback(offset, limit, &search)
}

#[tauri::command]
pub(crate) fn list_categories(state: tauri::State<'_, AppState>) -> Result<Vec<Category>, String> {
    state.store.list_categories()
}

#[tauri::command]
pub(crate) fn list_category_items(state: tauri::State<'_, AppState>) -> Result<Vec<CategoryItem>, String> {
    state.store.list_category_items()
}

#[tauri::command]
pub(crate) fn reorder_categories(
    state: tauri::State<'_, AppState>,
    category_ids: Vec<String>,
) -> Result<Vec<Category>, String> {
    state.store.reorder_categories(category_ids)
}

#[tauri::command]
pub(crate) fn reorder_category_items(
    state: tauri::State<'_, AppState>,
    category_id: String,
    item_ids: Vec<String>,
) -> Result<Vec<CategoryItem>, String> {
    state.store.reorder_category_items(category_id, item_ids)
}

#[tauri::command]
pub(crate) fn create_category(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Category, String> {
    state.store.create_category(name, color)
}

#[tauri::command]
pub(crate) fn create_category_with_clip(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
    clip_id: String,
) -> Result<CategoryWithItem, String> {
    state.store.create_category_with_clip(name, color, clip_id)
}

#[tauri::command]
pub(crate) fn update_category(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    color: String,
) -> Result<Category, String> {
    state.store.update_category(id, name, color)
}

#[tauri::command]
pub(crate) fn delete_category(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_category(id)
}

#[tauri::command]
pub(crate) fn add_clip_to_category(
    state: tauri::State<'_, AppState>,
    clip_id: String,
    category_id: String,
) -> Result<CategoryItem, String> {
    state.store.add_clip_to_category(clip_id, category_id)
}

#[tauri::command]
pub(crate) fn remove_category_item(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.remove_category_item(id)
}

#[tauri::command]
pub(crate) fn delete_clip(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_clip(id)
}

#[tauri::command]
pub(crate) fn clear_clips(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    state.store.clear_clips()
}

#[tauri::command]
pub(crate) fn rename_clip(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    display_name: Option<String>,
) -> Result<ClipUpdate, String> {
    state.store.rename_clip(id, collection, display_name)
}

#[tauri::command]
pub(crate) fn update_clip_content(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    text: String,
) -> Result<ClipUpdate, String> {
    state.store.update_clip_content(id, collection, text)
}

#[tauri::command]
pub(crate) fn set_clip_pinned(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    is_pinned: bool,
) -> Result<ClipUpdate, String> {
    state.store.set_clip_pinned(id, collection, is_pinned)
}

#[tauri::command]
pub(crate) fn copy_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    clip_type: String,
    text: String,
) -> Result<(), String> {
    let captured_item = captured_item_from_payload(&clip_type, &text)?;

    if clip_type == "image" {
        write_clipboard_image(&text)?;
    } else {
        write_clipboard_text(&text)?;
    }

    remember_current_clipboard_marker(
        &state.last_clipboard_change_id,
        &state.last_clipboard_hash,
        captured_item.as_ref().map(|item| item.content_hash.clone()),
    );

    if let Some(item) = captured_item {
        if let Some((clip, clip_total_count, was_inserted)) =
            state.store.insert_captured_item(item)?
        {
            let _ = app.emit(
                "ipaste://clipboard-captured",
                ClipboardCaptured {
                    clip,
                    clip_total_count,
                    was_inserted,
                },
            );
        }
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn set_listening(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    *state
        .is_listening
        .lock()
        .map_err(|error| error.to_string())? = enabled;
    let _ = app.emit(
        "ipaste://listening-changed",
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
) -> Result<bool, String> {
    set_append_copy_enabled_inner(&app, &state, enabled)
}

#[tauri::command]
pub(crate) fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    retention_days: i64,
) -> Result<AppSettings, String> {
    let settings = state.store.update_settings(retention_days)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_append_copy_timeout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    minutes: i64,
) -> Result<AppSettings, String> {
    let settings = state.store.update_append_copy_timeout_minutes(minutes)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_shortcut(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    shortcut: String,
) -> Result<AppSettings, String> {
    let shortcut = clean_shortcut(shortcut)?;
    update_registered_app_shortcut(&app, &state, &shortcut)?;
    let settings = state.store.update_shortcut(shortcut)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn set_app_shortcut_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    set_app_shortcut_enabled_inner(&app, &state, enabled)
}

#[tauri::command]
pub(crate) fn update_panel_open_behavior(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    behavior: String,
) -> Result<AppSettings, String> {
    let settings = state.store.update_panel_open_behavior(behavior)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_panel_layout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    layout: String,
) -> Result<AppSettings, String> {
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
) -> Result<AppSettings, String> {
    let settings = state.store.update_ocr_mode(mode)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn update_language(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    language: String,
) -> Result<AppSettings, String> {
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
) -> Result<AppSettings, String> {
    let settings = state.store.update_cloud_settings(api_address, api_key)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn disable_cloud_sync(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, String> {
    let settings = state.store.disable_cloud_sync()?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
pub(crate) fn test_cloud_settings(api_address: String, api_key: String) -> Result<bool, String> {
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
) -> Result<OcrInstallStatus, String> {
    #[cfg(target_os = "macos")]
    {
        return macos_ocr_install_status();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mode = _state.store.settings()?.ocr_mode;
        ocr_install_status(&_app, &mode)
    }
}

#[tauri::command]
pub(crate) async fn install_ocr_assets(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, String> {
    #[cfg(target_os = "macos")]
    {
        emit_ocr_install_progress(&_app, "completed", None, 0, 0);
        return macos_ocr_install_status();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let app_for_task = _app.clone();
        let mode = _state.store.settings()?.ocr_mode;
        tokio::task::spawn_blocking(move || install_ocr_assets_inner(&app_for_task, &mode))
            .await
            .map_err(|error| error.to_string())?
    }
}

#[tauri::command]
pub(crate) fn remove_ocr_assets(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, String> {
    #[cfg(target_os = "macos")]
    {
        return macos_ocr_install_status();
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mode = _state.store.settings()?.ocr_mode;
        let root = ocr_root_dir(&_app)?;
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        }
        ocr_install_status(&_app, &mode)
    }
}

#[tauri::command]
pub(crate) async fn recognize_image_text(
    _app: tauri::AppHandle,
    image_path: String,
) -> Result<ImageOcrResult, String> {
    #[cfg(target_os = "macos")]
    {
        return tokio::task::spawn_blocking(move || recognize_image_text_macos(image_path))
            .await
            .map_err(|error| error.to_string())?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        tokio::task::spawn_blocking(move || recognize_image_text_inner(&_app, image_path))
            .await
            .map_err(|error| error.to_string())?
    }
}

#[tauri::command]
pub(crate) fn sync_cloud_now(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, String> {
    state.store.sync_cloud()?;
    let (clip_page, categories, category_items) = state.store.snapshot()?;
    let settings = state.store.settings()?;
    Ok(AppSnapshot {
        clips: clip_page.clips,
        has_more_clips: clip_page.has_more,
        clip_total_count: clip_page.all_count,
        categories,
        category_items,
        shortcut: settings.shortcut.clone(),
        is_listening: *state
            .is_listening
            .lock()
            .map_err(|error| error.to_string())?,
        is_append_copy_enabled: state
            .append_copy_state
            .lock()
            .map(|value| value.is_enabled)
            .map_err(|error| error.to_string())?,
        settings,
    })
}

#[tauri::command]
pub(crate) fn sync_cloud_in_background(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let store = state.store.clone();
    thread::spawn(move || {
        if let Err(error) = store.sync_cloud() {
            eprintln!("background cloud sync failed: {error}");
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn show_panel(app: tauri::AppHandle) -> Result<(), String> {
    show_main_window(&app, MainWindowActivation::Activate)
}

#[tauri::command]
pub(crate) async fn show_settings(app: tauri::AppHandle) -> Result<(), String> {
    show_settings_window(&app)
}

#[tauri::command]
pub(crate) async fn open_clip_viewer(
    app: tauri::AppHandle,
    label: String,
    title: String,
) -> Result<(), String> {
    show_clip_viewer_window(&app, label, title)
}

#[tauri::command]
pub(crate) fn close_clip_viewer(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if !label.starts_with(CLIP_VIEWER_WINDOW_PREFIX) {
        return Err("无效的放大窗口标签".to_string());
    }

    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "未找到放大窗口".to_string())?;
    window.destroy().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn hide_panel(app: tauri::AppHandle) -> Result<(), String> {
    hide_main_window(&app)
}

#[tauri::command]
pub(crate) fn hide_settings(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(SETTINGS_WINDOW)
        .ok_or_else(|| "未找到设置窗口".to_string())?;
    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_main_window_dragging(
    state: tauri::State<'_, AppState>,
    dragging: bool,
) -> Result<(), String> {
    let mut is_dragging = state
        .is_dragging_main_window
        .lock()
        .map_err(|error| error.to_string())?;
    *is_dragging = dragging;
    Ok(())
}

#[tauri::command]
pub(crate) fn start_main_window_drag(app: tauri::AppHandle) -> Result<bool, String> {
    start_native_main_panel_drag(&app)
}

#[tauri::command]
pub(crate) fn open_accessibility_settings() -> Result<(), String> {
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
pub(crate) fn enable_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .enable()
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
pub(crate) fn disable_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .disable()
        .map_err(|error| error.to_string())?;
    Ok(false)
}

#[tauri::command]
pub(crate) fn is_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn apply_clip(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    clip_type: String,
    text: String,
) -> Result<(), String> {
    let captured_item = captured_item_from_payload(&clip_type, &text)?;
    if clip_type == "image" {
        write_clipboard_image(&text)?;
    } else {
        write_clipboard_text(&text)?;
    }
    remember_current_clipboard_marker(
        &state.last_clipboard_change_id,
        &state.last_clipboard_hash,
        captured_item.as_ref().map(|item| item.content_hash.clone()),
    );
    let target_app_bundle_id = state
        .target_app_bundle_id
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    let _ = hide_main_window(&app);

    if let Err(error) = prepare_target_for_paste(&app, target_app_bundle_id.clone()) {
        let _ = show_main_window(&app, MainWindowActivation::Activate);
        return Err(error);
    }

    // 等待并强制目标应用获得键盘焦点，避免 Cmd+V 投递到未就绪的窗口
    #[cfg(target_os = "macos")]
    if let Some(bundle_id) = target_app_bundle_id.as_deref() {
        if let Some(pid) = pid_for_bundle_id(bundle_id) {
            if let Err(error) = focus_target_app_window(pid) {
                let _ = show_main_window(&app, MainWindowActivation::Activate);
                return Err(error);
            }
        }
    }

    if let Err(error) = send_paste_shortcut() {
        let _ = show_main_window(&app, MainWindowActivation::Activate);
        return Err(error);
    }

    if let Some(item) = captured_item {
        if let Some((clip, clip_total_count, was_inserted)) =
            state.store.insert_captured_item(item)?
        {
            let _ = app.emit(
                "ipaste://clipboard-captured",
                ClipboardCaptured {
                    clip,
                    clip_total_count,
                    was_inserted,
                },
            );
        }
    } else {
        let conn = state.store.connect()?;
        let clip = state.store.get_clip_with_conn(&conn, &id)?;
        conn.execute(
            "UPDATE clips SET last_captured_at = ?1 WHERE id = ?2",
            params![now(), clip.id],
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn list_automations(state: tauri::State<'_, AppState>) -> Result<Vec<AutomationAction>, String> {
    state.store.list_automations()
}

#[tauri::command]
pub(crate) fn create_automation(
    state: tauri::State<'_, AppState>,
    input: AutomationInput,
) -> Result<AutomationAction, String> {
    state.store.create_automation(input)
}

#[tauri::command]
pub(crate) fn update_automation(
    state: tauri::State<'_, AppState>,
    id: String,
    input: AutomationInput,
) -> Result<AutomationAction, String> {
    state.store.update_automation(&id, input)
}

#[tauri::command]
pub(crate) fn delete_automation(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_automation(&id)
}

#[tauri::command]
pub(crate) async fn run_automation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<AutomationRunSummary, String> {
    let conn = state.store.connect()?;
    let action = state.store.get_automation_with_conn(&conn, &id)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn(async move { crate::automation::execute_automation(app, &store, action).await })
        .await
        .map_err(|e| format!("任务失败: {e}"))?
}

#[tauri::command]
pub(crate) fn get_automation_run(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<AutomationRunDetail, String> {
    let conn = state.store.connect()?;
    state.store.get_automation_run_detail(&conn, &run_id)
}
