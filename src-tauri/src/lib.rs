use std::{
    fs,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[cfg(target_os = "macos")]
use std::ffi::c_int;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use objc2::ffi::NSUInteger;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWorkspace,
};
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;
use rusqlite::{params, Connection};
#[cfg(target_os = "windows")]
use tauri::{menu::MenuItem, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use uuid::Uuid;

mod clipboard;
use crate::clipboard::*;

mod models;
use crate::models::*;

mod ocr;
use crate::ocr::*;

mod store;
use crate::store::*;

mod tray;
use crate::tray::*;

mod util;
use crate::util::*;

mod window;
use crate::window::*;

mod cloud;
use crate::cloud::*;

pub(crate) const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+V";
pub(crate) const PAUSE_CAPTURE_LABEL: &str = "暂停捕捉";
pub(crate) const RESUME_CAPTURE_LABEL: &str = "恢复捕捉";
pub(crate) const ENABLE_APPEND_COPY_LABEL: &str = "开启追加复制";
pub(crate) const DISABLE_APPEND_COPY_LABEL: &str = "关闭追加复制";
pub(crate) const DEFAULT_OCR_MODE: &str = "fast";
pub(crate) const DEFAULT_RETENTION_DAYS: i64 = 30;
pub(crate) const RETENTION_OPTIONS: [i64; 4] = [7, 14, 30, 90];
pub(crate) const DEFAULT_APPEND_COPY_TIMEOUT_MINUTES: i64 = 1;
pub(crate) const APPEND_COPY_TIMEOUT_OPTIONS: [i64; 4] = [1, 3, 5, 10];
pub(crate) const DEFAULT_PANEL_OPEN_BEHAVIOR: &str = "history";
pub(crate) const DEFAULT_PANEL_LAYOUT: &str = "top";
pub(crate) const DEFAULT_LANGUAGE: &str = "en";
pub(crate) const CLIP_PAGE_SIZE: usize = 20;
pub(crate) const IMAGE_DIR: &str = "clip-images";
pub(crate) const DEFAULT_CLIPBOARD_SEEDS: [(&str, Option<&str>, &str); 6] = [
    (
        "text",
        Some("Welcome to iPaste"),
        "Welcome to iPaste. Copied text, links, colors, and images are saved in local history so you can search and paste them again.",
    ),
    (
        "text",
        Some("Open panel shortcut"),
        "Press Command/Ctrl + Shift + V to open the iPaste panel, or click the tray icon.",
    ),
    (
        "text",
        Some("Content worth saving"),
        "Save reusable content into categories, such as support replies, addresses, emails, code snippets, prompts, or invoice details.",
    ),
    ("link", Some("iPaste project"), "https://github.com/iPaste-app/iPaste"),
    ("color", Some("iPaste accent color"), "#0D9488"),
    (
        "text",
        Some("Example prompt"),
        "Example prompt: Rewrite the following text to be clearer and more concise while preserving the original meaning.",
    ),
];

#[cfg(target_os = "macos")]
const PASTE_FOCUS_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
const PASTE_FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(30);
#[allow(dead_code)]
const CLOUD_SYNC_TYPES: [&str; 4] = ["text", "link", "color", "html"];

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn GetProcessForPID(pid: c_int, psn: *mut ProcessSerialNumber) -> i32;
    fn SetFrontProcessWithOptions(psn: *const ProcessSerialNumber, options: u32) -> i32;
}

#[cfg(target_os = "macos")]
const SET_FRONT_PROCESS_FRONT_WINDOW_ONLY: u32 = 1;


#[tauri::command]
fn get_snapshot(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, String> {
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
fn list_clips(
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
fn list_categories(state: tauri::State<'_, AppState>) -> Result<Vec<Category>, String> {
    state.store.list_categories()
}

#[tauri::command]
fn list_category_items(state: tauri::State<'_, AppState>) -> Result<Vec<CategoryItem>, String> {
    state.store.list_category_items()
}

#[tauri::command]
fn reorder_categories(
    state: tauri::State<'_, AppState>,
    category_ids: Vec<String>,
) -> Result<Vec<Category>, String> {
    state.store.reorder_categories(category_ids)
}

#[tauri::command]
fn reorder_category_items(
    state: tauri::State<'_, AppState>,
    category_id: String,
    item_ids: Vec<String>,
) -> Result<Vec<CategoryItem>, String> {
    state.store.reorder_category_items(category_id, item_ids)
}

#[tauri::command]
fn create_category(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Category, String> {
    state.store.create_category(name, color)
}

#[tauri::command]
fn create_category_with_clip(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
    clip_id: String,
) -> Result<CategoryWithItem, String> {
    state.store.create_category_with_clip(name, color, clip_id)
}

#[tauri::command]
fn update_category(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    color: String,
) -> Result<Category, String> {
    state.store.update_category(id, name, color)
}

#[tauri::command]
fn delete_category(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_category(id)
}

#[tauri::command]
fn add_clip_to_category(
    state: tauri::State<'_, AppState>,
    clip_id: String,
    category_id: String,
) -> Result<CategoryItem, String> {
    state.store.add_clip_to_category(clip_id, category_id)
}

#[tauri::command]
fn remove_category_item(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.remove_category_item(id)
}

#[tauri::command]
fn delete_clip(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    state.store.delete_clip(id)
}

#[tauri::command]
fn clear_clips(state: tauri::State<'_, AppState>) -> Result<usize, String> {
    state.store.clear_clips()
}

#[tauri::command]
fn rename_clip(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    display_name: Option<String>,
) -> Result<ClipUpdate, String> {
    state.store.rename_clip(id, collection, display_name)
}

#[tauri::command]
fn update_clip_content(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    text: String,
) -> Result<ClipUpdate, String> {
    state.store.update_clip_content(id, collection, text)
}

#[tauri::command]
fn set_clip_pinned(
    state: tauri::State<'_, AppState>,
    id: String,
    collection: String,
    is_pinned: bool,
) -> Result<ClipUpdate, String> {
    state.store.set_clip_pinned(id, collection, is_pinned)
}

#[tauri::command]
fn copy_clip(
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
fn set_listening(
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
fn set_append_copy_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    set_append_copy_enabled_inner(&app, &state, enabled)
}

#[tauri::command]
fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    retention_days: i64,
) -> Result<AppSettings, String> {
    let settings = state.store.update_settings(retention_days)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
fn update_append_copy_timeout(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    minutes: i64,
) -> Result<AppSettings, String> {
    let settings = state.store.update_append_copy_timeout_minutes(minutes)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
fn update_shortcut(
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
fn set_app_shortcut_enabled(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<bool, String> {
    set_app_shortcut_enabled_inner(&app, &state, enabled)
}

#[tauri::command]
fn update_panel_open_behavior(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    behavior: String,
) -> Result<AppSettings, String> {
    let settings = state.store.update_panel_open_behavior(behavior)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
fn update_panel_layout(
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
fn update_ocr_mode(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    mode: String,
) -> Result<AppSettings, String> {
    let settings = state.store.update_ocr_mode(mode)?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
fn update_language(
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
fn update_cloud_settings(
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
fn disable_cloud_sync(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, String> {
    let settings = state.store.disable_cloud_sync()?;
    emit_settings_changed(&app, &settings);
    Ok(settings)
}

#[tauri::command]
fn test_cloud_settings(api_address: String, api_key: String) -> Result<bool, String> {
    let api_address = clean_api_address(api_address)?;
    let api_key = clean_api_key(api_key)?;
    test_cloud_connection(&api_address, &api_key)?;
    Ok(true)
}

#[tauri::command]
fn get_app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
fn get_ocr_install_status(
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
async fn install_ocr_assets(
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
fn remove_ocr_assets(
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
async fn recognize_image_text(
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
fn sync_cloud_now(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, String> {
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
fn sync_cloud_in_background(state: tauri::State<'_, AppState>) -> Result<(), String> {
    let store = state.store.clone();
    thread::spawn(move || {
        if let Err(error) = store.sync_cloud() {
            eprintln!("background cloud sync failed: {error}");
        }
    });
    Ok(())
}

#[tauri::command]
fn show_panel(app: tauri::AppHandle) -> Result<(), String> {
    show_main_window(&app, MainWindowActivation::Activate)
}

#[tauri::command]
async fn show_settings(app: tauri::AppHandle) -> Result<(), String> {
    show_settings_window(&app)
}

#[tauri::command]
async fn open_clip_viewer(
    app: tauri::AppHandle,
    label: String,
    title: String,
) -> Result<(), String> {
    show_clip_viewer_window(&app, label, title)
}

#[tauri::command]
fn close_clip_viewer(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if !label.starts_with(CLIP_VIEWER_WINDOW_PREFIX) {
        return Err("无效的放大窗口标签".to_string());
    }

    let window = app
        .get_webview_window(&label)
        .ok_or_else(|| "未找到放大窗口".to_string())?;
    window.destroy().map_err(|error| error.to_string())
}

#[tauri::command]
fn hide_panel(app: tauri::AppHandle) -> Result<(), String> {
    hide_main_window(&app)
}

#[tauri::command]
fn hide_settings(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(SETTINGS_WINDOW)
        .ok_or_else(|| "未找到设置窗口".to_string())?;
    window.hide().map_err(|error| error.to_string())
}

#[tauri::command]
fn set_main_window_dragging(
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
fn start_main_window_drag(app: tauri::AppHandle) -> Result<bool, String> {
    start_native_main_panel_drag(&app)
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
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
fn enable_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .enable()
        .map_err(|error| error.to_string())?;
    Ok(true)
}

#[tauri::command]
fn disable_autostart(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .disable()
        .map_err(|error| error.to_string())?;
    Ok(false)
}

#[tauri::command]
fn is_autostart_enabled(app: tauri::AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn apply_clip(
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

    if let Err(error) = prepare_target_for_paste(&app, target_app_bundle_id) {
        let _ = show_main_window(&app, MainWindowActivation::Activate);
        return Err(error);
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }

                    let Some(state) = app.try_state::<AppState>() else {
                        return;
                    };
                    let Ok(active_shortcut) =
                        state.active_shortcut.lock().map(|value| value.clone())
                    else {
                        return;
                    };
                    if !shortcut_matches(shortcut, &active_shortcut) {
                        return;
                    }

                    remember_target_app_for_paste(app);
                    let app = app.clone();
                    thread::spawn(move || {
                        let _ = show_main_window(&app, MainWindowActivation::PreserveCurrentApp);
                        let _ = app.emit("ipaste://shortcut-opened", active_shortcut);
                    });
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            list_clips,
            list_categories,
            list_category_items,
            reorder_categories,
            reorder_category_items,
            create_category,
            create_category_with_clip,
            update_category,
            delete_category,
            add_clip_to_category,
            remove_category_item,
            delete_clip,
            clear_clips,
            rename_clip,
            update_clip_content,
            set_clip_pinned,
            copy_clip,
            set_listening,
            set_append_copy_enabled,
            update_settings,
            update_append_copy_timeout,
            update_shortcut,
            set_app_shortcut_enabled,
            update_panel_open_behavior,
            update_panel_layout,
            update_ocr_mode,
            update_language,
            update_cloud_settings,
            disable_cloud_sync,
            test_cloud_settings,
            get_app_info,
            get_ocr_install_status,
            install_ocr_assets,
            remove_ocr_assets,
            recognize_image_text,
            sync_cloud_now,
            sync_cloud_in_background,
            show_panel,
            show_settings,
            open_clip_viewer,
            close_clip_viewer,
            hide_panel,
            hide_settings,
            open_accessibility_settings,
            enable_autostart,
            disable_autostart,
            is_autostart_enabled,
            set_main_window_dragging,
            start_main_window_drag,
            apply_clip
        ])
        .setup(|app| {
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            let db_path = app.path().app_data_dir()?.join("ipaste.sqlite3");
            let store = Store::new(db_path)?;
            let settings = store.settings()?;
            let show_menu_item = MenuItem::with_id(
                app,
                "show",
                localized_text(&settings.language, "open_ipaste"),
                true,
                Some(settings.shortcut.as_str()),
            )?;
            let append_copy_menu_item = MenuItem::with_id(
                app,
                "append-copy",
                localized_text(&settings.language, "enable_append_copy"),
                true,
                None::<&str>,
            )?;
            let pause_capture_menu_item = MenuItem::with_id(
                app,
                "pause",
                localized_text(&settings.language, "pause_capture"),
                true,
                None::<&str>,
            )?;
            let settings_menu_item = MenuItem::with_id(
                app,
                "settings",
                localized_text(&settings.language, "settings"),
                true,
                None::<&str>,
            )?;
            let quit_menu_item = MenuItem::with_id(
                app,
                "quit",
                localized_text(&settings.language, "quit_ipaste"),
                true,
                None::<&str>,
            )?;
            let state = AppState {
                store: store.clone(),
                is_listening: Arc::new(Mutex::new(true)),
                show_menu_item: show_menu_item.clone(),
                append_copy_menu_item: append_copy_menu_item.clone(),
                pause_capture_menu_item: pause_capture_menu_item.clone(),
                settings_menu_item: settings_menu_item.clone(),
                quit_menu_item: quit_menu_item.clone(),
                append_copy_state: Arc::new(Mutex::new(AppendCopyState::default())),
                last_clipboard_change_id: Arc::new(Mutex::new(None)),
                last_clipboard_hash: Arc::new(Mutex::new(None)),
                is_dragging_main_window: Arc::new(Mutex::new(false)),
                target_app_bundle_id: Arc::new(Mutex::new(None)),
                main_window_activation: Arc::new(Mutex::new(MainWindowActivation::Activate)),
                active_shortcut: Arc::new(Mutex::new(settings.shortcut.clone())),
                is_app_shortcut_enabled: Arc::new(Mutex::new(true)),
                #[cfg(target_os = "macos")]
                main_panel_state: Arc::new(Mutex::new(None)),
            };

            let app_handle = app.handle().clone();
            spawn_clipboard_watcher(
                app_handle.clone(),
                store,
                state.is_listening.clone(),
                state.append_copy_state.clone(),
                state.last_clipboard_change_id.clone(),
                state.last_clipboard_hash.clone(),
            );

            app.manage(state);
            build_tray(
                app.handle(),
                show_menu_item,
                append_copy_menu_item,
                pause_capture_menu_item,
                settings_menu_item,
                quit_menu_item,
                settings.language.as_str(),
            )?;
            register_app_shortcut(app.handle(), &settings.shortcut)?;
            show_main_window(app.handle(), MainWindowActivation::Activate)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == MAIN_WINDOW {
                if let WindowEvent::Focused(false) = event {
                    if current_main_window_activation(window.app_handle())
                        == MainWindowActivation::PreserveCurrentApp
                    {
                        return;
                    }

                    let window = window.clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(180));
                        let app = window.app_handle();
                        let is_dragging = app
                            .try_state::<AppState>()
                            .and_then(|state| {
                                state
                                    .is_dragging_main_window
                                    .lock()
                                    .ok()
                                    .map(|value| *value)
                            })
                            .unwrap_or(false);

                        if is_dragging || window.is_focused().unwrap_or(false) {
                            return;
                        }

                        let _ = hide_main_window(&app);
                    });
                }
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                let _ = show_main_window(app, MainWindowActivation::Activate);
            }
            "settings" => {
                let app = app.clone();
                thread::spawn(move || {
                    let _ = show_settings_window(&app);
                });
            }
            "append-copy" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let enabled = state
                        .append_copy_state
                        .lock()
                        .map(|value| !value.is_enabled)
                        .unwrap_or(true);
                    let _ = set_append_copy_enabled_inner(app, &state, enabled);
                }
            }
            "pause" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if let Ok(mut listening) = state.is_listening.lock() {
                        *listening = !*listening;
                        update_pause_capture_menu_label(&state, *listening);
                        let _ = app.emit(
                            "ipaste://listening-changed",
                            ListeningChanged {
                                is_listening: *listening,
                            },
                        );
                    }
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn shortcut_matches(shortcut: &Shortcut, shortcut_spec: &str) -> bool {
    shortcut_spec
        .parse::<Shortcut>()
        .map(|expected| shortcut.id() == expected.id())
        .unwrap_or(false)
}

fn register_app_shortcut(app: &tauri::AppHandle, shortcut: &str) -> Result<(), String> {
    if app.global_shortcut().is_registered(shortcut) {
        return Ok(());
    }

    app.global_shortcut()
        .register(shortcut)
        .map_err(|error| shortcut_registration_error(shortcut, error))
}

fn unregister_app_shortcut(app: &tauri::AppHandle, shortcut: &str) -> Result<(), String> {
    if !app.global_shortcut().is_registered(shortcut) {
        return Ok(());
    }

    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|error| error.to_string())
}

fn set_app_shortcut_enabled_inner(
    app: &tauri::AppHandle,
    state: &AppState,
    enabled: bool,
) -> Result<bool, String> {
    let shortcut = state
        .active_shortcut
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    if enabled {
        register_app_shortcut(app, &shortcut)?;
    } else {
        unregister_app_shortcut(app, &shortcut)?;
    }

    *state
        .is_app_shortcut_enabled
        .lock()
        .map_err(|error| error.to_string())? = enabled;
    Ok(enabled)
}

fn update_registered_app_shortcut(
    app: &tauri::AppHandle,
    state: &AppState,
    shortcut: &str,
) -> Result<(), String> {
    let mut active_shortcut = state
        .active_shortcut
        .lock()
        .map_err(|error| error.to_string())?;
    let previous = active_shortcut.clone();

    if previous == shortcut {
        if is_app_shortcut_enabled(state)? && !app.global_shortcut().is_registered(shortcut) {
            register_app_shortcut(app, shortcut)?;
        }
        state
            .show_menu_item
            .set_accelerator(Some(shortcut))
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let was_enabled = is_app_shortcut_enabled(state)?;
    unregister_app_shortcut(app, previous.as_str())?;

    if was_enabled {
        if let Err(error) = register_app_shortcut(app, shortcut) {
            let _ = register_app_shortcut(app, &previous);
            return Err(error);
        }
    }

    if let Err(error) = state.show_menu_item.set_accelerator(Some(shortcut)) {
        let _ = app.global_shortcut().unregister(shortcut);
        if was_enabled {
            let _ = register_app_shortcut(app, &previous);
        }
        return Err(error.to_string());
    }

    *active_shortcut = shortcut.to_string();
    Ok(())
}

fn is_app_shortcut_enabled(state: &AppState) -> Result<bool, String> {
    state
        .is_app_shortcut_enabled
        .lock()
        .map(|value| *value)
        .map_err(|error| error.to_string())
}

fn shortcut_registration_error(shortcut: &str, error: impl ToString) -> String {
    format!(
        "无法注册快捷键 {shortcut}：{}。请换一个未被系统或其他应用占用的组合。",
        error.to_string()
    )
}

fn emit_settings_changed(app: &tauri::AppHandle, settings: &AppSettings) {
    let _ = app.emit(
        "ipaste://settings-changed",
        SettingsChanged {
            settings: settings.clone(),
        },
    );
}

pub(crate) fn remember_main_window_activation(
    app: &tauri::AppHandle,
    activation: MainWindowActivation,
) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };

    let mut current = state
        .main_window_activation
        .lock()
        .map_err(|error| error.to_string())?;
    *current = activation;
    Ok(())
}

pub(crate) fn current_main_window_activation(app: &tauri::AppHandle) -> MainWindowActivation {
    app.try_state::<AppState>()
        .and_then(|state| {
            state
                .main_window_activation
                .lock()
                .ok()
                .map(|activation| *activation)
        })
        .unwrap_or(MainWindowActivation::Activate)
}

pub(crate) fn remember_target_app_for_paste(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    if let Some(bundle_id) = frontmost_external_app_bundle_id(app) {
        if let Ok(mut target) = state.target_app_bundle_id.lock() {
            *target = Some(bundle_id);
        }
    }
}

#[cfg(target_os = "macos")]
fn frontmost_external_app_bundle_id(app: &tauri::AppHandle) -> Option<String> {
    let app_bundle_id = current_app_bundle_id(app);
    let frontmost = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let bundle_id = frontmost.bundleIdentifier()?.to_string();

    if Some(bundle_id.as_str()) == app_bundle_id.as_deref() {
        None
    } else {
        Some(bundle_id)
    }
}

#[cfg(not(target_os = "macos"))]
fn frontmost_external_app_bundle_id(_app: &tauri::AppHandle) -> Option<String> {
    None
}

fn prepare_target_for_paste(
    _app: &tauri::AppHandle,
    _target_app_bundle_id: Option<String>,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(bundle_id) = _target_app_bundle_id {
            activate_app_for_paste(_app, &bundle_id)?;
            return Ok(());
        }
    }

    thread::sleep(Duration::from_millis(180));
    Ok(())
}

#[cfg(target_os = "macos")]
fn activate_app_for_paste(app: &tauri::AppHandle, bundle_id: &str) -> Result<(), String> {
    if Some(bundle_id) == current_app_bundle_id(app).as_deref() {
        thread::sleep(Duration::from_millis(180));
        return Ok(());
    }

    if Some(bundle_id) == current_frontmost_app_bundle_id_for_paste(app).as_deref() {
        thread::sleep(Duration::from_millis(40));
        return Ok(());
    }

    if activate_running_app_for_paste(app, bundle_id)? {
        thread::sleep(Duration::from_millis(70));
        return Ok(());
    }

    if wait_for_frontmost_app(app, bundle_id, PASTE_FOCUS_TIMEOUT).is_ok() {
        return Ok(());
    }

    let _ = open_app_bundle_for_paste(bundle_id);
    if activate_running_app_for_paste(app, bundle_id)? {
        thread::sleep(Duration::from_millis(70));
        return Ok(());
    }

    let _ = wait_for_frontmost_app(app, bundle_id, PASTE_FOCUS_TIMEOUT);
    thread::sleep(Duration::from_millis(70));
    Ok(())
}

#[cfg(target_os = "macos")]
fn activate_running_app_for_paste(
    app: &tauri::AppHandle,
    bundle_id: &str,
) -> Result<bool, String> {
    let bundle_id = bundle_id.to_string();
    run_on_main_thread_for_paste(app, move || {
        activate_running_app_for_paste_on_main_thread(&bundle_id)
    })?
}

#[cfg(target_os = "macos")]
fn activate_running_app_for_paste_on_main_thread(bundle_id: &str) -> Result<bool, String> {
    deactivate_current_application_for_paste();

    let target_bundle_id = NSString::from_str(bundle_id);
    let applications =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&target_bundle_id);
    let Some(target) = (unsafe { applications.firstObject_unchecked() }) else {
        return Err("无法自动粘贴：目标应用已退出，请重新打开 iPaste 面板后再粘贴。".to_string());
    };

    let _ = target.unhide();
    let pid = target.processIdentifier();
    if set_front_process_for_pid(pid as c_int).is_ok() {
        return Ok(true);
    }

    let activation_options = NSApplicationActivationOptions(
        NSApplicationActivationOptions::ActivateAllWindows.bits() | (1 as NSUInteger) << 1,
    );
    let current_app = NSRunningApplication::currentApplication();
    let activated = target.activateFromApplication_options(&current_app, activation_options)
        || target.activateWithOptions(activation_options);
    if !activated {
        return Err("无法自动粘贴：无法切回目标应用，请确认目标窗口仍可用。".to_string());
    }

    Ok(false)
}

#[cfg(target_os = "macos")]
fn deactivate_current_application_for_paste() {
    if let Some(marker) = objc2::MainThreadMarker::new() {
        NSApplication::sharedApplication(marker).deactivate();
    }
}

#[cfg(target_os = "macos")]
fn set_front_process_for_pid(pid: c_int) -> Result<(), String> {
    if pid < 0 {
        return Err("无效的目标应用进程".to_string());
    }

    let mut psn = ProcessSerialNumber {
        highLongOfPSN: 0,
        lowLongOfPSN: 0,
    };
    let get_status = unsafe { GetProcessForPID(pid, &mut psn) };
    if get_status != 0 {
        return Err(format!("GetProcessForPID failed with status {get_status}"));
    }

    let set_status =
        unsafe { SetFrontProcessWithOptions(&psn, SET_FRONT_PROCESS_FRONT_WINDOW_ONLY) };
    if set_status != 0 {
        return Err(format!(
            "SetFrontProcessWithOptions failed with status {set_status}"
        ));
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn open_app_bundle_for_paste(bundle_id: &str) -> bool {
    Command::new("open")
        .arg("-b")
        .arg(bundle_id)
        .spawn()
        .map(|_| true)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn current_frontmost_app_bundle_id_for_paste(app: &tauri::AppHandle) -> Option<String> {
    run_on_main_thread_for_paste(app, current_frontmost_app_bundle_id)
        .ok()
        .flatten()
}

#[cfg(target_os = "macos")]
fn wait_for_frontmost_app(
    app: &tauri::AppHandle,
    bundle_id: &str,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;

    loop {
        if let Some(frontmost_bundle_id) = current_frontmost_app_bundle_id_for_paste(app) {
            if frontmost_bundle_id == bundle_id {
                thread::sleep(Duration::from_millis(40));
                return Ok(());
            }
        }

        if Instant::now() >= deadline {
            return Err(
                "无法自动粘贴：未能切回目标应用，请重新打开 iPaste 面板后再试。".to_string(),
            );
        }

        thread::sleep(PASTE_FOCUS_POLL_INTERVAL);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn run_on_main_thread_for_paste<T, F>(app: &tauri::AppHandle, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    if objc2::MainThreadMarker::new().is_some() {
        return Ok(task());
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = sender.send(task());
    })
    .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn current_frontmost_app_bundle_id() -> Option<String> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|application| application.bundleIdentifier())
        .map(|bundle_id| bundle_id.to_string())
}

#[cfg(target_os = "macos")]
fn current_app_bundle_id(app: &tauri::AppHandle) -> Option<String> {
    NSRunningApplication::currentApplication()
        .bundleIdentifier()
        .map(|bundle_id| bundle_id.to_string())
        .or_else(|| Some(app.config().identifier.clone()))
}


pub(crate) fn test_cloud_connection(api_address: &str, api_key: &str) -> Result<(), String> {
    let payload: HealthPayload = cloud_get(api_address, api_key, "/api/health")?;
    if payload.service.as_deref() == Some("ipaste-cloud") {
        Ok(())
    } else {
        Err("云同步服务响应不正确".to_string())
    }
}

pub(crate) fn is_syncable_clip_type(clip_type: &str) -> bool {
    CLOUD_SYNC_TYPES.contains(&clip_type)
}

pub(crate) fn clean_color(color: String) -> String {
    let color = color.trim();
    if color.starts_with('#')
        && (color.len() == 7 || color.len() == 4)
        && color[1..].chars().all(|char| char.is_ascii_hexdigit())
    {
        color.to_string()
    } else {
        "#0D9488".to_string()
    }
}

pub(crate) fn safe_filename(value: &str) -> String {
    value
        .chars()
        .filter(|char| char.is_ascii_alphanumeric() || matches!(char, '-' | '_'))
        .collect::<String>()
}

pub(crate) fn new_id() -> String {
    Uuid::new_v4().to_string()
}

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, String> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let exists = table_column_names(conn, table)?
        .iter()
        .any(|name| name == column);

    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

pub(crate) fn table_column_names(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    columns
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn map_clip(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipItem> {
    Ok(ClipItem {
        id: row.get(0)?,
        clip_type: row.get(1)?,
        content_hash: row.get(2)?,
        display_name: row.get(3)?,
        preview_text: row.get(4)?,
        text: row.get(5)?,
        source_app: row.get(6)?,
        last_captured_at: row.get(7)?,
        favorite_count: row.get(8)?,
        is_pinned: row.get(9)?,
    })
}

pub(crate) fn map_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub(crate) fn map_category_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<CategoryItem> {
    Ok(CategoryItem {
        id: row.get(0)?,
        category_id: row.get(1)?,
        clip_snapshot_id: row.get(2)?,
        clip_type: row.get(3)?,
        content_hash: row.get(4)?,
        display_name: row.get(5)?,
        preview_text: row.get(6)?,
        text: row.get(7)?,
        sort_order: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        sync_state: row.get(11)?,
        is_pinned: row.get(12)?,
    })
}
