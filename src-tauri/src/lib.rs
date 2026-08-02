use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

#[cfg(not(target_os = "macos"))]
use std::io::{Read, Write};
#[cfg(target_os = "macos")]
use std::ffi::c_int;
#[cfg(target_os = "macos")]
use std::time::Instant;

#[cfg(target_os = "macos")]
use objc2::{
    define_class,
    ffi::NSUInteger,
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject, Bool},
    sel, ClassType, MainThreadOnly,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSAutoresizingMaskOptions,
    NSBackingStoreType, NSFloatingWindowLevel, NSPanel, NSRunningApplication, NSView,
    NSResponder, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
    NSWindowStyleMask, NSWorkspace,
};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CGRect;
#[cfg(target_os = "macos")]
use objc2_foundation::{
    NSArray, NSError, NSObjectProtocol, NSPoint, NSRange, NSRect, NSString, NSURL,
};
use reqwest::blocking::Client;
use rusqlite::{params, Connection};
#[cfg(target_os = "windows")]
use tauri::PhysicalSize;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    utils::config::Color,
    Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use uuid::Uuid;
#[cfg(not(target_os = "macos"))]
use zip::ZipArchive;

mod clipboard;
use crate::clipboard::*;

mod models;
use crate::models::*;

mod store;
use crate::store::*;

mod util;
use crate::util::*;

mod cloud;
use crate::cloud::*;

#[cfg(target_os = "macos")]
define_class!(
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "IPastePanel"]
    #[ivars = ()]
    struct IPastePanel;

    impl IPastePanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }

        #[unsafe(method(canBecomeMainWindow))]
        fn can_become_main_window(&self) -> bool {
            false
        }
    }
);

const MAIN_WINDOW: &str = "main";
const SETTINGS_WINDOW: &str = "settings";
const CLIP_VIEWER_WINDOW_PREFIX: &str = "clip-viewer-";
pub(crate) const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+V";
pub(crate) const PAUSE_CAPTURE_LABEL: &str = "暂停捕捉";
pub(crate) const RESUME_CAPTURE_LABEL: &str = "恢复捕捉";
pub(crate) const ENABLE_APPEND_COPY_LABEL: &str = "开启追加复制";
pub(crate) const DISABLE_APPEND_COPY_LABEL: &str = "关闭追加复制";
#[cfg(not(target_os = "macos"))]
const OCR_GITHUB_RELEASE_BASE_URL: &str =
    "https://github.com/iPaste-app/iPaste/releases/download/ipaste-ocr-windows-v1/";
#[cfg(not(target_os = "macos"))]
const OCR_R2_BASE_URL: &str = env!("IPASTE_OCR_R2_BASE_URL");
#[cfg(not(target_os = "macos"))]
const UPDATER_R2_ENDPOINT: &str = env!("IPASTE_UPDATER_R2_ENDPOINT");
#[cfg(not(target_os = "macos"))]
const OCR_DIR: &str = "ocr";
#[cfg(not(target_os = "macos"))]
const OCR_ASSET_DIR: &str = "assets";
#[cfg(not(target_os = "macos"))]
const OCR_ENGINE_DIR: &str = "tesseract";
pub(crate) const DEFAULT_OCR_MODE: &str = "fast";
#[cfg(not(target_os = "macos"))]
const OCR_FAST_TOTAL_BYTES: u64 = 37_557_099;
#[cfg(not(target_os = "macos"))]
const OCR_BEST_TOTAL_BYTES: u64 = 59_452_879;
#[cfg(target_os = "macos")]
const MACOS_OCR_ENGINE_ID: &str = "apple-vision";
#[cfg(target_os = "macos")]
const MACOS_OCR_LANGUAGE: &str = "zh-Hans+en";
#[cfg(target_os = "macos")]
const MACOS_OCR_RECOGNITION_LEVEL_ACCURATE: isize = 0;
const PANEL_GAP: i32 = 12;
const SCREEN_MARGIN: i32 = 12;
const MAIN_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 560.0,
    height: 620.0,
    min_width: 560.0,
    min_height: 500.0,
    max_width: Some(720.0),
    max_height: None,
};
const SIDE_MAIN_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 720.0,
    height: 620.0,
    min_width: 700.0,
    min_height: 500.0,
    max_width: Some(720.0),
    max_height: None,
};
const SETTINGS_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 760.0,
    height: 520.0,
    min_width: 680.0,
    min_height: 460.0,
    max_width: None,
    max_height: None,
};
const CLIP_VIEWER_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 840.0,
    height: 620.0,
    min_width: 640.0,
    min_height: 460.0,
    max_width: None,
    max_height: None,
};
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

#[cfg(target_os = "macos")]
fn start_native_main_panel_drag(app: &tauri::AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    let panel_state = state.main_panel_state.clone();
    if !panel_state
        .lock()
        .map_err(|error| error.to_string())?
        .map(|state| state.visible)
        .unwrap_or(false)
    {
        return Ok(false);
    }

    run_on_main_thread_for_paste(app, move || -> Result<bool, String> {
        autoreleasepool(|_| {
            let Some(mtm) = objc2::MainThreadMarker::new() else {
                return Ok(false);
            };
            let guard = panel_state.lock().map_err(|error| error.to_string())?;
            let Some(current) = *guard else {
                return Ok(false);
            };
            if !current.visible {
                return Ok(false);
            }

            let panel = unsafe { &*(current.panel as *mut NSPanel) };
            let app = NSApplication::sharedApplication(mtm);
            let Some(event) = app.currentEvent() else {
                return Ok(false);
            };
            panel.performWindowDragWithEvent(&event);
            Ok(true)
        })
    })?
}

#[cfg(not(target_os = "macos"))]
fn start_native_main_panel_drag(_app: &tauri::AppHandle) -> Result<bool, String> {
    Ok(false)
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

fn update_pause_capture_menu_label(state: &AppState, is_listening: bool) {
    let language = state
        .store
        .settings()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| DEFAULT_LANGUAGE.to_string());
    let label = localized_text(
        &language,
        if is_listening {
            "pause_capture"
        } else {
            "resume_capture"
        },
    );
    let _ = state.pause_capture_menu_item.set_text(label);
}

fn set_append_copy_enabled_inner(
    app: &tauri::AppHandle,
    state: &AppState,
    enabled: bool,
) -> Result<bool, String> {
    let (is_enabled, timer_session_id) = {
        let mut append_copy = state
            .append_copy_state
            .lock()
            .map_err(|error| error.to_string())?;
        let mut timer_session_id = None;

        if append_copy.is_enabled != enabled {
            append_copy.is_enabled = enabled;
            append_copy.clip_id = None;
            append_copy.text.clear();
            append_copy.session_id = enabled.then(new_id);
            timer_session_id = append_copy.session_id.clone();
        }

        (append_copy.is_enabled, timer_session_id)
    };

    update_append_copy_menu_label(state, is_enabled);
    let _ = app.emit(
        "ipaste://append-copy-changed",
        AppendCopyChanged { is_enabled },
    );
    if let Some(session_id) = timer_session_id {
        let settings = state.store.settings()?;
        let timeout = Duration::from_secs(settings.append_copy_timeout_minutes.max(1) as u64 * 60);
        spawn_append_copy_timeout(
            app.clone(),
            state.append_copy_state.clone(),
            state.append_copy_menu_item.clone(),
            session_id,
            timeout,
            settings.language,
        );
    }
    Ok(is_enabled)
}

fn update_append_copy_menu_label(state: &AppState, is_enabled: bool) {
    let language = state
        .store
        .settings()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| DEFAULT_LANGUAGE.to_string());
    let label = localized_text(
        &language,
        if is_enabled {
            "disable_append_copy"
        } else {
            "enable_append_copy"
        },
    );
    let _ = state.append_copy_menu_item.set_text(label);
}

fn spawn_append_copy_timeout(
    app: tauri::AppHandle,
    append_copy_state: Arc<Mutex<AppendCopyState>>,
    append_copy_menu_item: MenuItem<tauri::Wry>,
    session_id: String,
    timeout: Duration,
    language: String,
) {
    thread::spawn(move || {
        thread::sleep(timeout);
        let should_emit = append_copy_state
            .lock()
            .map(|mut append_copy| {
                if !append_copy.is_enabled
                    || append_copy.session_id.as_deref() != Some(session_id.as_str())
                {
                    return false;
                }

                append_copy.is_enabled = false;
                append_copy.clip_id = None;
                append_copy.session_id = None;
                append_copy.text.clear();
                true
            })
            .unwrap_or(false);

        if !should_emit {
            return;
        }

        let _ = append_copy_menu_item.set_text(localized_text(&language, "enable_append_copy"));
        let _ = app.emit(
            "ipaste://append-copy-changed",
            AppendCopyChanged { is_enabled: false },
        );
    });
}

fn build_tray(
    app: &tauri::AppHandle,
    show: MenuItem<tauri::Wry>,
    append_copy: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    language: &str,
) -> tauri::Result<()> {
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&show, &append_copy, &settings, &pause, &separator, &quit],
    )?;

    let mut tray = TrayIconBuilder::with_id("ipaste")
        .tooltip(localized_text(language, "tray_tooltip"))
        .menu(&menu)
        .icon_as_template(false)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_main_window(tray.app_handle(), MainWindowActivation::Activate);
            }
        });

    if let Some(icon) = tray_icon().or_else(|| app.default_window_icon().cloned()) {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

fn tray_icon() -> Option<tauri::image::Image<'static>> {
    #[cfg(target_os = "windows")]
    let bytes = include_bytes!("../icons/tray-icon-windows.png");

    #[cfg(not(target_os = "windows"))]
    let bytes = include_bytes!("../icons/tray-icon.png");

    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(tauri::image::Image::new_owned(
        image.into_raw(),
        width,
        height,
    ))
}

fn current_main_window_geometry(app: &tauri::AppHandle) -> WindowGeometry {
    app.try_state::<AppState>()
        .and_then(|state| state.store.settings().ok())
        .map(|settings| main_window_geometry_for_layout(&settings.panel_layout))
        .unwrap_or(MAIN_WINDOW_GEOMETRY)
}

fn main_window_geometry_for_layout(layout: &str) -> WindowGeometry {
    if layout == "side" {
        SIDE_MAIN_WINDOW_GEOMETRY
    } else {
        MAIN_WINDOW_GEOMETRY
    }
}

fn apply_main_window_layout_geometry(app: &tauri::AppHandle, layout: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        return Ok(());
    };
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or(app.primary_monitor().map_err(|error| error.to_string())?)
        .ok_or_else(|| "未找到可用屏幕".to_string())?;

    apply_window_geometry_for_monitor(&window, &monitor, main_window_geometry_for_layout(layout))?;
    Ok(())
}

fn show_main_window(
    app: &tauri::AppHandle,
    activation: MainWindowActivation,
) -> Result<(), String> {
    remember_target_app_for_paste(app);

    let geometry = current_main_window_geometry(app);

    let window = if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        window
    } else {
        WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
            .title("iPaste")
            .inner_size(geometry.width, geometry.height)
            .min_inner_size(geometry.min_width, geometry.min_height)
            .max_inner_size(
                geometry.max_width.unwrap_or(10000.0),
                geometry.max_height.unwrap_or(10000.0),
            )
            .decorations(false)
            .transparent(true)
            .resizable(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focusable(false)
            .focused(false)
            .visible(false)
            .build()
            .map_err(|error| error.to_string())?
    };

    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
    let _ = window.set_shadow(false);

    let mut effective_activation = activation;
    let mut native_panel = false;

    match effective_activation {
        MainWindowActivation::Activate => {
            remember_main_window_activation(app, MainWindowActivation::Activate)?;
            restore_main_webview_to_host_window(app, &window)?;
            let _ = window.set_focusable(true);
            configure_main_window_activation(&window, MainWindowActivation::Activate);
            position_window_near_cursor(app, &window, geometry)?;
            window.show().map_err(|error| error.to_string())?;
            position_window_near_cursor(app, &window, geometry)?;
            window.set_focus().map_err(|error| error.to_string())?;
        }
        MainWindowActivation::PreserveCurrentApp => {
            remember_main_window_activation(app, MainWindowActivation::PreserveCurrentApp)?;
            let _ = window.set_focusable(true);
            position_window_near_cursor(app, &window, geometry)?;
            match show_main_window_with_native_panel(app, &window) {
                Ok(true) => {
                    native_panel = true;
                }
                Ok(false) => {
                    effective_activation = MainWindowActivation::Activate;
                    remember_main_window_activation(app, MainWindowActivation::Activate)?;
                    restore_main_webview_to_host_window(app, &window)?;
                    let _ = window.set_focusable(true);
                    configure_main_window_activation(&window, MainWindowActivation::Activate);
                    window.show().map_err(|error| error.to_string())?;
                    position_window_near_cursor(app, &window, geometry)?;
                    window.set_focus().map_err(|error| error.to_string())?;
                }
                Err(error) => {
                    eprintln!("failed to show native main panel, falling back to activation: {error}");
                    effective_activation = MainWindowActivation::Activate;
                    remember_main_window_activation(app, MainWindowActivation::Activate)?;
                    restore_main_webview_to_host_window(app, &window)?;
                    let _ = window.set_focusable(true);
                    configure_main_window_activation(&window, MainWindowActivation::Activate);
                    window.show().map_err(|error| error.to_string())?;
                    position_window_near_cursor(app, &window, geometry)?;
                    window.set_focus().map_err(|error| error.to_string())?;
                }
            }
        }
    }

    let _ = app.emit(
        "ipaste://panel-visibility-changed",
        PanelVisibilityChanged {
            visible: true,
            preserves_current_app: effective_activation == MainWindowActivation::PreserveCurrentApp,
            native_panel,
        },
    );
    Ok(())
}

fn hide_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_WINDOW)
        .ok_or_else(|| "未找到主面板".to_string())?;
    let activation = current_main_window_activation(app);
    let native_panel = activation == MainWindowActivation::PreserveCurrentApp
        && is_native_main_panel_visible(app);
    let _ = app.emit(
        "ipaste://panel-visibility-changed",
        PanelVisibilityChanged {
            visible: false,
            preserves_current_app: activation == MainWindowActivation::PreserveCurrentApp,
            native_panel,
        },
    );

    let result = if native_panel {
        hide_native_main_panel(app).map(|_| ())
    } else if activation == MainWindowActivation::PreserveCurrentApp {
        hide_main_window_preserving_current_app(&window)
    } else {
        window.hide().map_err(|error| error.to_string())
    };

    let _ = remember_main_window_activation(app, MainWindowActivation::Activate);
    result
}

#[cfg(target_os = "macos")]
fn with_main_webview<T, F>(window: &tauri::WebviewWindow, task: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(tauri::webview::PlatformWebview) -> T + Send + 'static,
{
    let (sender, receiver) = std::sync::mpsc::channel();
    window
        .with_webview(move |webview| {
            let _ = sender.send(task(webview));
        })
        .map_err(|error| error.to_string())?;
    receiver.recv().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn show_main_window_with_native_panel(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<bool, String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    let panel_state = state.main_panel_state.clone();

    with_main_webview(window, move |webview| {
        autoreleasepool(|_| -> Result<bool, String> {
            let host_window_ptr = webview.ns_window();
            let webview_ptr = webview.inner();
            if host_window_ptr.is_null() || webview_ptr.is_null() {
                return Ok(false);
            }

            let host_window = unsafe { &*(host_window_ptr.cast::<NSWindow>()) };
            let webview_view = unsafe { &*(webview_ptr.cast::<NSView>()) };
            let webview_responder = unsafe { &*(webview_ptr.cast::<NSResponder>()) };
            let host_frame = host_window.frame();
            let mut guard = panel_state.lock().map_err(|error| error.to_string())?;
            let mut current = if let Some(current) = *guard {
                current
            } else {
                create_native_main_panel(host_frame)?
            };
            let panel = unsafe { &*(current.panel as *mut NSPanel) };

            configure_native_main_panel(panel);
            panel.setFrame_display(host_frame, false);
            let Some(content_view) = panel.contentView() else {
                return Err("无法创建原生主面板内容视图".to_string());
            };
            webview_view.removeFromSuperview();
            content_view.addSubview(webview_view);
            fit_webview_to_content_view(webview_view, &content_view);

            host_window.orderOut(None);
            panel.orderFrontRegardless();
            panel.makeKeyWindow();
            let _ = panel.makeFirstResponder(Some(webview_responder));

            current.visible = true;
            *guard = Some(current);
            Ok(true)
        })
    })?
}

#[cfg(not(target_os = "macos"))]
fn show_main_window_with_native_panel(
    _app: &tauri::AppHandle,
    _window: &tauri::WebviewWindow,
) -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "macos")]
fn create_native_main_panel(frame: NSRect) -> Result<MainPanelState, String> {
    let mtm = objc2::MainThreadMarker::new()
        .ok_or_else(|| "原生主面板必须在主线程创建".to_string())?;
    let _ = mtm;
    let style = NSWindowStyleMask::NonactivatingPanel
        | NSWindowStyleMask::UtilityWindow
        | NSWindowStyleMask::Resizable
        | NSWindowStyleMask::FullSizeContentView;
    let allocated: *mut AnyObject = unsafe { msg_send![IPastePanel::class(), alloc] };
    if allocated.is_null() {
        return Err("无法分配原生主面板".to_string());
    }
    let panel_ptr: *mut NSPanel = unsafe {
        msg_send![
            allocated,
            initWithContentRect: frame,
            styleMask: style,
            backing: NSBackingStoreType::Buffered,
            defer: Bool::new(false)
        ]
    };
    let panel = unsafe { Retained::from_raw(panel_ptr) }
        .ok_or_else(|| "无法初始化原生主面板".to_string())?;
    configure_native_main_panel(&panel);
    Ok(MainPanelState {
        panel: Retained::into_raw(panel) as usize,
        visible: false,
    })
}

#[cfg(target_os = "macos")]
fn configure_native_main_panel(panel: &NSPanel) {
    panel.setFloatingPanel(true);
    panel.setBecomesKeyOnlyIfNeeded(false);
    panel.setWorksWhenModal(true);
    panel.setLevel(NSFloatingWindowLevel);
    panel.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    panel.setHidesOnDeactivate(false);
    panel.setCanHide(false);
    panel.setMovable(true);
    panel.setMovableByWindowBackground(true);
    panel.setIgnoresMouseEvents(false);
    panel.setAcceptsMouseMovedEvents(true);
    panel.setAnimationBehavior(NSWindowAnimationBehavior::None);
    panel.setHasShadow(false);
    panel.setOpaque(false);
    unsafe {
        panel.setReleasedWhenClosed(false);
    }
    set_native_panel_clear_background(panel);
}

#[cfg(target_os = "macos")]
fn set_native_panel_clear_background(panel: &NSPanel) {
    let Some(color_class) = AnyClass::get(c"NSColor") else {
        return;
    };
    unsafe {
        let clear_color: *mut AnyObject = msg_send![color_class, clearColor];
        if !clear_color.is_null() {
            let _: () = msg_send![panel, setBackgroundColor: clear_color];
        }
    }
}

#[cfg(target_os = "macos")]
fn fit_webview_to_content_view(webview_view: &NSView, content_view: &NSView) {
    let content_frame = content_view.frame();
    webview_view.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), content_frame.size));
    webview_view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
}

#[cfg(target_os = "macos")]
fn restore_main_webview_to_host_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    let panel_state = state.main_panel_state.clone();
    if panel_state
        .lock()
        .map_err(|error| error.to_string())?
        .is_none()
    {
        return Ok(());
    }

    with_main_webview(window, move |webview| {
        autoreleasepool(|_| -> Result<(), String> {
            let host_window_ptr = webview.ns_window();
            let webview_ptr = webview.inner();
            if host_window_ptr.is_null() || webview_ptr.is_null() {
                return Ok(());
            }

            let host_window = unsafe { &*(host_window_ptr.cast::<NSWindow>()) };
            let webview_view = unsafe { &*(webview_ptr.cast::<NSView>()) };
            let webview_responder = unsafe { &*(webview_ptr.cast::<NSResponder>()) };
            let Some(content_view) = host_window.contentView() else {
                return Err("无法还原主面板内容视图".to_string());
            };
            webview_view.removeFromSuperview();
            content_view.addSubview(webview_view);
            fit_webview_to_content_view(webview_view, &content_view);
            let _ = host_window.makeFirstResponder(Some(webview_responder));

            let mut guard = panel_state.lock().map_err(|error| error.to_string())?;
            if let Some(mut current) = *guard {
                let panel = unsafe { &*(current.panel as *mut NSPanel) };
                panel.orderOut(None);
                current.visible = false;
                *guard = Some(current);
            }
            Ok(())
        })
    })?
}

#[cfg(not(target_os = "macos"))]
fn restore_main_webview_to_host_window(
    _app: &tauri::AppHandle,
    _window: &tauri::WebviewWindow,
) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn hide_native_main_panel(app: &tauri::AppHandle) -> Result<bool, String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(false);
    };
    let panel_state = state.main_panel_state.clone();
    run_on_main_thread_for_paste(app, move || -> Result<bool, String> {
        autoreleasepool(|_| {
            let mut guard = panel_state.lock().map_err(|error| error.to_string())?;
            let Some(mut current) = *guard else {
                return Ok(false);
            };
            let panel = unsafe { &*(current.panel as *mut NSPanel) };
            panel.orderOut(None);
            current.visible = false;
            *guard = Some(current);
            Ok(true)
        })
    })?
}

#[cfg(not(target_os = "macos"))]
fn hide_native_main_panel(_app: &tauri::AppHandle) -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "macos")]
fn is_native_main_panel_visible(app: &tauri::AppHandle) -> bool {
    app.try_state::<AppState>()
        .and_then(|state| {
            state
                .main_panel_state
                .lock()
                .ok()
                .and_then(|panel_state| panel_state.map(|state| state.visible))
        })
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
fn is_native_main_panel_visible(_app: &tauri::AppHandle) -> bool {
    false
}

#[cfg(target_os = "macos")]
fn hide_main_window_preserving_current_app(window: &tauri::WebviewWindow) -> Result<(), String> {
    let dispatch_window = window.clone();
    let native_window = window.clone();
    dispatch_window
        .run_on_main_thread(move || {
            let Ok(ns_window_ptr) = native_window.ns_window() else {
                return;
            };
            let ns_window = unsafe { &*(ns_window_ptr.cast::<NSWindow>()) };
            ns_window.orderOut(None);
        })
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn hide_main_window_preserving_current_app(window: &tauri::WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
fn configure_main_window_activation(
    window: &tauri::WebviewWindow,
    activation: MainWindowActivation,
) {
    let dispatch_window = window.clone();
    let native_window = window.clone();
    let _ = dispatch_window.run_on_main_thread(move || {
        configure_main_window_activation_on_main_thread(&native_window, activation);
    });
}

#[cfg(target_os = "macos")]
fn configure_main_window_activation_on_main_thread(
    window: &tauri::WebviewWindow,
    activation: MainWindowActivation,
) {
    let Ok(ns_window_ptr) = window.ns_window() else {
        return;
    };

    let ns_window = unsafe { &*(ns_window_ptr.cast::<NSWindow>()) };
    let mut style_mask = ns_window.styleMask();
    let mut collection_behavior = ns_window.collectionBehavior();
    collection_behavior.remove(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::Transient
            | NSWindowCollectionBehavior::IgnoresCycle,
    );

    if activation == MainWindowActivation::PreserveCurrentApp {
        style_mask.insert(NSWindowStyleMask::NonactivatingPanel);
        collection_behavior.insert(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        set_main_window_prevents_activation(ns_window, true);
    } else {
        style_mask.remove(NSWindowStyleMask::NonactivatingPanel);
        set_main_window_prevents_activation(ns_window, false);
    }

    ns_window.setStyleMask(style_mask);
    ns_window.setLevel(NSFloatingWindowLevel);
    ns_window.setCollectionBehavior(collection_behavior);
    ns_window.setHidesOnDeactivate(false);
    ns_window.setIgnoresMouseEvents(false);
    ns_window.setAcceptsMouseMovedEvents(true);
}

#[cfg(target_os = "macos")]
fn set_main_window_prevents_activation(ns_window: &NSWindow, prevents_activation: bool) {
    let selector = sel!(_setPreventsActivation:);
    if !ns_window.respondsToSelector(selector) {
        return;
    }

    unsafe {
        let _: () = msg_send![
            ns_window,
            _setPreventsActivation: Bool::new(prevents_activation)
        ];
    }
}

#[cfg(not(target_os = "macos"))]
fn configure_main_window_activation(
    _window: &tauri::WebviewWindow,
    _activation: MainWindowActivation,
) {
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

fn remember_main_window_activation(
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

fn current_main_window_activation(app: &tauri::AppHandle) -> MainWindowActivation {
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

fn remember_target_app_for_paste(app: &tauri::AppHandle) {
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
fn run_on_main_thread_for_paste<T, F>(app: &tauri::AppHandle, task: F) -> Result<T, String>
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

fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    let language = app
        .try_state::<AppState>()
        .and_then(|state| state.store.settings().ok())
        .map(|settings| settings.language)
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string());
    let main_monitor = app
        .get_webview_window(MAIN_WINDOW)
        .and_then(|window| window.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let _ = hide_main_window(app);
    let window = if let Some(window) = app.get_webview_window(SETTINGS_WINDOW) {
        window
    } else {
        WebviewWindowBuilder::new(
            app,
            SETTINGS_WINDOW,
            WebviewUrl::App("index.html?window=settings".into()),
        )
        .title(localized_text(&language, "settings_title"))
        .inner_size(
            SETTINGS_WINDOW_GEOMETRY.width,
            SETTINGS_WINDOW_GEOMETRY.height,
        )
        .min_inner_size(
            SETTINGS_WINDOW_GEOMETRY.min_width,
            SETTINGS_WINDOW_GEOMETRY.min_height,
        )
        .resizable(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?
    };

    if let Some(monitor) = &main_monitor {
        position_window_centered_on_monitor(&window, &monitor, SETTINGS_WINDOW_GEOMETRY)?;
    } else {
        window.center().map_err(|error| error.to_string())?;
    }
    window.show().map_err(|error| error.to_string())?;
    if let Some(monitor) = &main_monitor {
        position_window_centered_on_monitor(&window, &monitor, SETTINGS_WINDOW_GEOMETRY)?;
    }
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn show_clip_viewer_window(
    app: &tauri::AppHandle,
    label: String,
    title: String,
) -> Result<(), String> {
    if !label.starts_with(CLIP_VIEWER_WINDOW_PREFIX) {
        return Err("无效的放大窗口标签".to_string());
    }

    let url = format!("index.html?window=clip-viewer&label={label}");
    let window = if let Some(window) = app.get_webview_window(&label) {
        window
    } else {
        WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
            .title(title)
            .inner_size(
                CLIP_VIEWER_WINDOW_GEOMETRY.width,
                CLIP_VIEWER_WINDOW_GEOMETRY.height,
            )
            .min_inner_size(
                CLIP_VIEWER_WINDOW_GEOMETRY.min_width,
                CLIP_VIEWER_WINDOW_GEOMETRY.min_height,
            )
            .decorations(false)
            .resizable(true)
            .always_on_top(true)
            .visible(false)
            .build()
            .map_err(|error| error.to_string())?
    };

    position_clip_viewer_window(app, &window)?;
    window
        .set_always_on_top(true)
        .map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    position_clip_viewer_window(app, &window)?;
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn position_window_near_cursor(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    geometry: WindowGeometry,
) -> Result<(), String> {
    let cursor = app.cursor_position().map_err(|error| error.to_string())?;
    let cursor_x = cursor.x.round() as i32;
    let cursor_y = cursor.y.round() as i32;
    let monitor = monitor_for_point(app, cursor_x, cursor_y)?;
    let work_area = monitor.work_area();
    let (width, height) = apply_window_geometry_for_monitor(window, &monitor, geometry)?;

    let left = work_area.position.x + SCREEN_MARGIN;
    let top = work_area.position.y + SCREEN_MARGIN;
    let right = work_area.position.x + work_area.size.width as i32 - width - SCREEN_MARGIN;
    let bottom = work_area.position.y + work_area.size.height as i32 - height - SCREEN_MARGIN;

    let x = clamp(cursor_x - width / 2, left, right.max(left));
    let below = cursor_y + PANEL_GAP;
    let above = cursor_y - height - PANEL_GAP;
    let y = clamp(
        if below <= bottom {
            below
        } else if above >= top {
            above
        } else {
            below
        },
        top,
        bottom.max(top),
    );

    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

fn position_window_centered_on_monitor(
    window: &tauri::WebviewWindow,
    monitor: &tauri::Monitor,
    geometry: WindowGeometry,
) -> Result<(), String> {
    let work_area = monitor.work_area();
    let (width, height) = apply_window_geometry_for_monitor(window, monitor, geometry)?;
    let x = clamp(
        work_area.position.x + (work_area.size.width as i32 - width) / 2,
        work_area.position.x + SCREEN_MARGIN,
        work_area.position.x + work_area.size.width as i32 - width - SCREEN_MARGIN,
    );
    let y = clamp(
        work_area.position.y + (work_area.size.height as i32 - height) / 2,
        work_area.position.y + SCREEN_MARGIN,
        work_area.position.y + work_area.size.height as i32 - height - SCREEN_MARGIN,
    );

    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

fn position_clip_viewer_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    let main_window = app
        .get_webview_window(MAIN_WINDOW)
        .ok_or_else(|| "未找到主面板".to_string())?;
    let target_monitor = main_window
        .current_monitor()
        .map_err(|error| error.to_string())?
        .or(window
            .current_monitor()
            .map_err(|error| error.to_string())?)
        .or(app.primary_monitor().map_err(|error| error.to_string())?)
        .ok_or_else(|| "未找到可用屏幕".to_string())?;
    let main_position = main_window
        .outer_position()
        .map_err(|error| error.to_string())?;
    let main_size = main_window
        .outer_size()
        .map_err(|error| error.to_string())?;
    let main_work_area = target_monitor.work_area();

    let (width, height) =
        apply_window_geometry_for_monitor(window, &target_monitor, CLIP_VIEWER_WINDOW_GEOMETRY)?;
    let main_center_x = main_position.x + main_size.width as i32 / 2;
    let main_center_y = main_position.y + main_size.height as i32 / 2;
    let x = clamp(
        main_center_x - width / 2,
        main_work_area.position.x + SCREEN_MARGIN,
        main_work_area.position.x + main_work_area.size.width as i32 - width - SCREEN_MARGIN,
    );
    let y = clamp(
        main_center_y - height / 2,
        main_work_area.position.y + SCREEN_MARGIN,
        main_work_area.position.y + main_work_area.size.height as i32 - height - SCREEN_MARGIN,
    );

    window
        .set_position(PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

fn apply_window_geometry_for_monitor(
    window: &tauri::WebviewWindow,
    monitor: &tauri::Monitor,
    geometry: WindowGeometry,
) -> Result<(i32, i32), String> {
    let expected_size = window_size_for_monitor(window, monitor, geometry);
    let target_scale = monitor.scale_factor().max(0.1);

    #[cfg(target_os = "windows")]
    window
        .set_min_size(Some(PhysicalSize::new(
            (geometry.min_width * target_scale).ceil().max(1.0) as u32,
            (geometry.min_height * target_scale).ceil().max(1.0) as u32,
        )))
        .map_err(|error| error.to_string())?;

    #[cfg(target_os = "windows")]
    if geometry.max_width.is_some() || geometry.max_height.is_some() {
        let work_area = monitor.work_area();
        let max_width = geometry
            .max_width
            .map(|value| (value * target_scale).ceil().max(1.0) as u32)
            .unwrap_or(work_area.size.width);
        let max_height = geometry
            .max_height
            .map(|value| (value * target_scale).ceil().max(1.0) as u32)
            .unwrap_or(work_area.size.height);
        window
            .set_max_size(Some(PhysicalSize::new(max_width, max_height)))
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "windows")]
    window
        .set_size(PhysicalSize::new(
            expected_size.0 as u32,
            expected_size.1 as u32,
        ))
        .map_err(|error| error.to_string())?;

    #[cfg(not(target_os = "windows"))]
    window
        .set_min_size(Some(tauri::LogicalSize::new(
            geometry.min_width,
            geometry.min_height,
        )))
        .map_err(|error| error.to_string())?;

    #[cfg(not(target_os = "windows"))]
    if geometry.max_width.is_some() || geometry.max_height.is_some() {
        let work_area = monitor.work_area();
        window
            .set_max_size(Some(tauri::LogicalSize::new(
                geometry
                    .max_width
                    .unwrap_or(work_area.size.width as f64 / target_scale),
                geometry
                    .max_height
                    .unwrap_or(work_area.size.height as f64 / target_scale),
            )))
            .map_err(|error| error.to_string())?;
    }

    #[cfg(not(target_os = "windows"))]
    window
        .set_size(tauri::LogicalSize::new(geometry.width, geometry.height))
        .map_err(|error| error.to_string())?;

    Ok(expected_size)
}

fn window_size_for_monitor(
    _window: &tauri::WebviewWindow,
    monitor: &tauri::Monitor,
    geometry: WindowGeometry,
) -> (i32, i32) {
    let target_scale = monitor.scale_factor().max(0.1);
    let width = (geometry.width * target_scale).ceil() as i32;
    let height = (geometry.height * target_scale).ceil() as i32;
    fit_window_size_to_monitor(monitor, (width.max(1), height.max(1)))
}

fn fit_window_size_to_monitor(monitor: &tauri::Monitor, size: (i32, i32)) -> (i32, i32) {
    let work_area = monitor.work_area();
    let max_width = (work_area.size.width as i32 - SCREEN_MARGIN * 2).max(1);
    let max_height = (work_area.size.height as i32 - SCREEN_MARGIN * 2).max(1);
    (size.0.min(max_width), size.1.min(max_height))
}

fn monitor_for_point(app: &tauri::AppHandle, x: i32, y: i32) -> Result<tauri::Monitor, String> {
    let monitors = app
        .available_monitors()
        .map_err(|error| error.to_string())?;
    if let Some(monitor) = monitors
        .iter()
        .find(|monitor| point_in_monitor(monitor, x, y))
    {
        return Ok(monitor.clone());
    }

    if let Some(monitor) = monitors
        .into_iter()
        .min_by_key(|monitor| monitor_distance_squared(monitor, x, y))
    {
        return Ok(monitor);
    }

    app.primary_monitor()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到可用屏幕".to_string())
}

fn point_in_monitor(monitor: &tauri::Monitor, x: i32, y: i32) -> bool {
    let position = monitor.position();
    let size = monitor.size();
    let left = position.x;
    let top = position.y;
    let right = left + size.width as i32;
    let bottom = top + size.height as i32;

    x >= left && x < right && y >= top && y < bottom
}

fn monitor_distance_squared(monitor: &tauri::Monitor, x: i32, y: i32) -> i64 {
    let position = monitor.position();
    let size = monitor.size();
    let left = position.x as i64;
    let top = position.y as i64;
    let right = left + size.width as i64;
    let bottom = top + size.height as i64;
    let x = x as i64;
    let y = y as i64;

    let dx = if x < left {
        left - x
    } else if x > right {
        x - right
    } else {
        0
    };

    let dy = if y < top {
        top - y
    } else if y > bottom {
        y - bottom
    } else {
        0
    };

    dx * dx + dy * dy
}

fn apply_tray_language(state: &AppState, language: &str) {
    let _ = state
        .show_menu_item
        .set_text(localized_text(language, "open_ipaste"));
    let _ = state
        .settings_menu_item
        .set_text(localized_text(language, "settings"));
    let _ = state
        .quit_menu_item
        .set_text(localized_text(language, "quit_ipaste"));

    let is_append_copy_enabled = state
        .append_copy_state
        .lock()
        .map(|append_copy| append_copy.is_enabled)
        .unwrap_or(false);
    let _ = state.append_copy_menu_item.set_text(localized_text(
        language,
        if is_append_copy_enabled {
            "disable_append_copy"
        } else {
            "enable_append_copy"
        },
    ));

    let is_listening = state
        .is_listening
        .lock()
        .map(|listening| *listening)
        .unwrap_or(true);
    let _ = state.pause_capture_menu_item.set_text(localized_text(
        language,
        if is_listening {
            "pause_capture"
        } else {
            "resume_capture"
        },
    ));
}

pub(crate) fn test_cloud_connection(api_address: &str, api_key: &str) -> Result<(), String> {
    let payload: HealthPayload = cloud_get(api_address, api_key, "/api/health")?;
    if payload.service.as_deref() == Some("ipaste-cloud") {
        Ok(())
    } else {
        Err("云同步服务响应不正确".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn install_ocr_assets_inner(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    emit_ocr_install_progress(app, "fetchingManifest", None, 0, 0);
    let manifest = fetch_ocr_manifest(&mode)?;

    let asset_dir = ocr_asset_dir(app)?;
    let download_dir = ocr_download_dir(app)?;
    fs::create_dir_all(&asset_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&download_dir).map_err(|error| error.to_string())?;
    let total_bytes = manifest_total_bytes(&manifest);
    let mut downloaded_bytes = 0_u64;

    emit_ocr_install_progress(app, "downloading", None, downloaded_bytes, total_bytes);

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?;

    for file in &manifest.engine.files {
        if ocr_manifest_file_installed(app, file)? {
            downloaded_bytes = downloaded_bytes.saturating_add(file.size);
            emit_ocr_install_progress(
                app,
                "downloading",
                Some(file.name.clone()),
                downloaded_bytes.min(total_bytes),
                total_bytes,
            );
            continue;
        }

        let url = format!("{}{}", manifest.engine.base_url, file.path);
        let target_path = ocr_download_target_path(app, file)?;
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temp_path = target_path.with_extension("download");
        let mut response = client.get(url).send().map_err(|error| error.to_string())?;

        if !response.status().is_success() {
            return Err(format!(
                "{} 下载失败：{}",
                file.name,
                response.status().as_u16()
            ));
        }

        let mut output = fs::File::create(&temp_path).map_err(|error| error.to_string())?;
        let mut buffer = [0_u8; 64 * 1024];
        let file_start_bytes = downloaded_bytes;
        let mut file_bytes = 0_u64;

        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }

            output
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            file_bytes = file_bytes.saturating_add(read as u64);
            emit_ocr_install_progress(
                app,
                "downloading",
                Some(file.name.clone()),
                file_start_bytes.saturating_add(file_bytes).min(total_bytes),
                total_bytes,
            );
        }

        output.flush().map_err(|error| error.to_string())?;
        let hash = file_sha256(&temp_path)?;
        if !hash.eq_ignore_ascii_case(&file.sha256) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("{} 校验失败", file.name));
        }

        fs::rename(&temp_path, &target_path).map_err(|error| error.to_string())?;
        if file.archive.as_deref() == Some("zip") {
            install_ocr_zip_archive(app, file, &target_path)?;
            let _ = fs::remove_file(&target_path);
        }
        downloaded_bytes = file_start_bytes.saturating_add(file.size);
    }

    write_ocr_manifest_cache(app, &mode, &manifest)?;
    let status = ocr_install_status_for_manifest(app, &manifest, &mode)?;
    emit_ocr_install_progress(
        app,
        "completed",
        None,
        status.downloaded_bytes,
        status.total_bytes,
    );
    Ok(status)
}

#[cfg(not(target_os = "macos"))]
fn fetch_ocr_manifest(mode: &str) -> Result<OcrManifest, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| error.to_string())?;
    let mut errors = Vec::new();

    for manifest_url in ocr_manifest_urls(mode) {
        match fetch_ocr_manifest_from_url(&client, &manifest_url, mode) {
            Ok(manifest) => return Ok(manifest),
            Err(error) => errors.push(format!("{manifest_url}：{error}")),
        }
    }

    Err(format!("无法获取 OCR 资源信息：{}", errors.join("；")))
}

#[cfg(not(target_os = "macos"))]
fn fetch_ocr_manifest_from_url(
    client: &Client,
    manifest_url: &str,
    mode: &str,
) -> Result<OcrManifest, String> {
    let response = client
        .get(manifest_url)
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }

    let manifest = response
        .json::<OcrManifest>()
        .map_err(|error| format!("无法解析 OCR manifest：{error}"))?;
    validate_ocr_manifest(&manifest, mode)?;
    Ok(manifest)
}

#[cfg(not(target_os = "macos"))]
fn validate_ocr_manifest(manifest: &OcrManifest, mode: &str) -> Result<(), String> {
    if manifest.engine.id != "tesseract" {
        return Err("OCR manifest 引擎不受支持".to_string());
    }
    if manifest.engine.mode.as_deref().unwrap_or(mode) != mode {
        return Err(format!("OCR manifest 模式不匹配：{mode}"));
    }
    if manifest.engine.platform != ocr_platform() {
        return Err(format!(
            "OCR manifest 平台不匹配：{}",
            manifest.engine.platform
        ));
    }
    if !manifest.engine.base_url.starts_with("https://") {
        return Err("OCR manifest 下载地址不安全".to_string());
    }
    if manifest.engine.files.is_empty() {
        return Err("OCR manifest 没有文件".to_string());
    }
    for file in &manifest.engine.files {
        if file.name.contains('/') || file.name.contains('\\') || file.name.contains("..") {
            return Err(format!("OCR 文件名不安全：{}", file.name));
        }
        if file.path.contains("..") {
            return Err(format!("OCR 文件路径不安全：{}", file.path));
        }
        if file.role == "engine" && file.archive.as_deref() != Some("zip") {
            return Err("OCR 引擎需要使用 portable zip 包".to_string());
        }
        if let Some(archive) = &file.archive {
            if archive != "zip" {
                return Err(format!("OCR archive 类型不受支持：{archive}"));
            }
        }
        if let Some(install_dir) = &file.install_dir {
            validate_relative_path(install_dir)?;
        }
        for entry in &file.entries {
            validate_relative_path(entry)?;
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ocr_install_status(app: &tauri::AppHandle, mode: &str) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    match read_ocr_manifest_cache(app, &mode)? {
        Some(manifest) => ocr_install_status_for_manifest(app, &manifest, &mode),
        None => {
            let install_dir = ocr_root_dir(app)?;
            Ok(OcrInstallStatus {
                installed: false,
                engine_id: "tesseract".to_string(),
                engine_version: None,
                mode: mode.clone(),
                platform: ocr_platform().to_string(),
                manifest_url: ocr_primary_manifest_url(&mode),
                install_dir: install_dir.to_string_lossy().to_string(),
                downloaded_bytes: 0,
                total_bytes: ocr_default_total_bytes(&mode),
                missing_files: Vec::new(),
            })
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_ocr_install_status() -> Result<OcrInstallStatus, String> {
    Ok(OcrInstallStatus {
        installed: true,
        engine_id: MACOS_OCR_ENGINE_ID.to_string(),
        engine_version: Some("system".to_string()),
        mode: DEFAULT_OCR_MODE.to_string(),
        platform: ocr_platform().to_string(),
        manifest_url: String::new(),
        install_dir: String::new(),
        downloaded_bytes: 0,
        total_bytes: 0,
        missing_files: Vec::new(),
    })
}

#[cfg(not(target_os = "macos"))]
fn ocr_install_status_for_manifest(
    app: &tauri::AppHandle,
    manifest: &OcrManifest,
    mode: &str,
) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    let install_dir = ocr_root_dir(app)?;
    let mut downloaded_bytes = 0_u64;
    let mut missing_files = Vec::new();

    for file in &manifest.engine.files {
        if ocr_manifest_file_installed(app, file)? {
            downloaded_bytes = downloaded_bytes.saturating_add(file.size);
        } else {
            missing_files.push(file.name.clone());
        }
    }

    let total_bytes = manifest_total_bytes(manifest);
    Ok(OcrInstallStatus {
        installed: missing_files.is_empty() && !manifest.engine.files.is_empty(),
        engine_id: manifest.engine.id.clone(),
        engine_version: Some(manifest.engine.version.clone()),
        mode: mode.clone(),
        platform: manifest.engine.platform.clone(),
        manifest_url: ocr_primary_manifest_url(&mode),
        install_dir: install_dir.to_string_lossy().to_string(),
        downloaded_bytes,
        total_bytes,
        missing_files,
    })
}

#[cfg(not(target_os = "macos"))]
fn manifest_total_bytes(manifest: &OcrManifest) -> u64 {
    manifest.engine.files.iter().map(|file| file.size).sum()
}

#[cfg(not(target_os = "macos"))]
fn ocr_default_total_bytes(mode: &str) -> u64 {
    match mode {
        "best" => OCR_BEST_TOTAL_BYTES,
        _ => OCR_FAST_TOTAL_BYTES,
    }
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_urls(mode: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for base_url in ocr_r2_base_urls() {
        push_unique_url(&mut urls, ocr_manifest_url_for_base(&base_url, mode));
    }
    push_unique_url(
        &mut urls,
        ocr_manifest_url_for_base(OCR_GITHUB_RELEASE_BASE_URL, mode),
    );
    urls
}

#[cfg(not(target_os = "macos"))]
fn ocr_primary_manifest_url(mode: &str) -> String {
    ocr_manifest_urls(mode)
        .into_iter()
        .next()
        .unwrap_or_else(|| ocr_manifest_url_for_base(OCR_GITHUB_RELEASE_BASE_URL, mode))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_url_for_base(base_url: &str, mode: &str) -> String {
    format!("{base_url}ipaste-ocr-windows-x64-{mode}.json")
}

#[cfg(not(target_os = "macos"))]
fn ocr_r2_base_urls() -> Vec<String> {
    let mut base_urls = Vec::new();

    if let Ok(base_url) = std::env::var("IPASTE_OCR_R2_BASE_URL") {
        push_optional_base_url(&mut base_urls, normalize_ocr_base_url(&base_url));
    }
    push_optional_base_url(&mut base_urls, normalize_ocr_base_url(OCR_R2_BASE_URL));

    if let Ok(endpoint) = std::env::var("IPASTE_UPDATER_R2_ENDPOINT") {
        push_optional_base_url(&mut base_urls, derive_ocr_r2_base_url(&endpoint));
    }
    push_optional_base_url(&mut base_urls, derive_ocr_r2_base_url(UPDATER_R2_ENDPOINT));

    base_urls
}

#[cfg(not(target_os = "macos"))]
fn push_optional_base_url(base_urls: &mut Vec<String>, base_url: Option<String>) {
    if let Some(base_url) = base_url {
        push_unique_url(base_urls, base_url);
    }
}

#[cfg(not(target_os = "macos"))]
fn push_unique_url(urls: &mut Vec<String>, url: String) {
    if !urls.iter().any(|existing| existing == &url) {
        urls.push(url);
    }
}

#[cfg(not(target_os = "macos"))]
fn normalize_ocr_base_url(base_url: &str) -> Option<String> {
    let base_url = base_url.trim();
    if base_url.is_empty() || !base_url.starts_with("https://") {
        return None;
    }
    let base_url = base_url.split(['?', '#']).next().unwrap_or(base_url);
    Some(if base_url.ends_with('/') {
        base_url.to_string()
    } else {
        format!("{base_url}/")
    })
}

#[cfg(not(target_os = "macos"))]
fn derive_ocr_r2_base_url(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() || !endpoint.starts_with("https://") {
        return None;
    }
    let endpoint = endpoint
        .split(['?', '#'])
        .next()
        .unwrap_or(endpoint)
        .trim_end_matches('/');
    let parent_index = endpoint.rfind('/')?;
    let parent = &endpoint[..parent_index];
    if parent.len() <= "https://".len() {
        return None;
    }
    Some(format!("{parent}/ocr/"))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_file_installed(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<bool, String> {
    if file.archive.as_deref() == Some("zip") {
        if file.entries.is_empty() {
            return Ok(false);
        }
        let install_dir = ocr_manifest_install_dir(app, file)?;
        return file
            .entries
            .iter()
            .map(|entry| install_dir.join(entry).exists())
            .try_fold(true, |all_exist, exists| Ok(all_exist && exists));
    }

    let target_path = ocr_manifest_file_path(app, file)?;
    file_is_valid(&target_path, &file.sha256)
}

#[cfg(not(target_os = "macos"))]
fn ocr_download_target_path(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    if file.archive.as_deref() == Some("zip") {
        return Ok(ocr_download_dir(app)?.join(&file.name));
    }

    ocr_manifest_file_path(app, file)
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_file_path(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    if file.role == "language" {
        return Ok(ocr_asset_dir(app)?.join(&file.name));
    }
    Ok(ocr_manifest_install_dir(app, file)?.join(&file.name))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_install_dir(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    let root = ocr_root_dir(app)?;
    let install_dir = file
        .install_dir
        .as_deref()
        .unwrap_or(if file.role == "engine" {
            OCR_ENGINE_DIR
        } else {
            OCR_ASSET_DIR
        });
    validate_relative_path(install_dir)?;
    let resolved = root.join(install_dir);
    ensure_path_within(&root, &resolved)?;
    Ok(resolved)
}

#[cfg(not(target_os = "macos"))]
fn install_ocr_zip_archive(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
    archive_path: &Path,
) -> Result<(), String> {
    let install_dir = ocr_manifest_install_dir(app, file)?;
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&install_dir).map_err(|error| error.to_string())?;

    let archive_file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(archive_file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut zipped_file = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(enclosed_name) = zipped_file.enclosed_name().map(PathBuf::from) else {
            return Err("OCR portable zip 包含不安全路径".to_string());
        };
        let output_path = install_dir.join(enclosed_name);
        ensure_path_within(&install_dir, &output_path)?;

        if zipped_file.is_dir() {
            fs::create_dir_all(&output_path).map_err(|error| error.to_string())?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::File::create(&output_path).map_err(|error| error.to_string())?;
        std::io::copy(&mut zipped_file, &mut output).map_err(|error| error.to_string())?;
    }

    if !ocr_manifest_file_installed(app, file)? {
        return Err(format!("{} 解压后文件不完整", file.name));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ensure_path_within(root: &Path, path: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
            root.canonicalize()
        })
        .map_err(|error| error.to_string())?;
    let path = if path.exists() {
        path.canonicalize().map_err(|error| error.to_string())?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| "OCR 路径无父目录".to_string())?;
        let parent = parent
            .canonicalize()
            .or_else(|_| {
                fs::create_dir_all(parent)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
                parent.canonicalize()
            })
            .map_err(|error| error.to_string())?;
        parent.join(
            path.file_name()
                .ok_or_else(|| "OCR 路径无文件名".to_string())?,
        )
    };

    if path.starts_with(root) {
        Ok(())
    } else {
        Err("OCR 路径越界".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
fn file_is_valid(path: &PathBuf, expected_sha256: &str) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let hash = file_sha256(path)?;
    Ok(hash.eq_ignore_ascii_case(expected_sha256))
}

#[cfg(not(target_os = "macos"))]
fn read_ocr_manifest_cache(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<Option<OcrManifest>, String> {
    let path = ocr_manifest_cache_path(app, mode)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str::<OcrManifest>(&content)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn write_ocr_manifest_cache(
    app: &tauri::AppHandle,
    mode: &str,
    manifest: &OcrManifest,
) -> Result<(), String> {
    let path = ocr_manifest_cache_path(app, mode)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn ocr_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| error.to_string())
        .map(|path| path.join(OCR_DIR))
}

#[cfg(not(target_os = "macos"))]
fn ocr_asset_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join(OCR_ASSET_DIR))
}

#[cfg(not(target_os = "macos"))]
fn ocr_engine_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join(OCR_ENGINE_DIR))
}

#[cfg(not(target_os = "macos"))]
fn ocr_download_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join("downloads"))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_cache_path(app: &tauri::AppHandle, mode: &str) -> Result<PathBuf, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    Ok(ocr_root_dir(app)?.join(format!("manifest-{mode}.json")))
}

fn ocr_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "macos-system";
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x64"
    }
    #[cfg(not(any(
        target_os = "macos",
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        "unsupported"
    }
}

fn emit_ocr_install_progress(
    app: &tauri::AppHandle,
    phase: &str,
    file_name: Option<String>,
    downloaded_bytes: u64,
    total_bytes: u64,
) {
    let _ = app.emit(
        "ipaste://ocr-install-progress",
        OcrInstallProgress {
            phase: phase.to_string(),
            file_name,
            downloaded_bytes,
            total_bytes,
        },
    );
}

#[cfg(not(target_os = "macos"))]
fn recognize_image_text_inner(
    app: &tauri::AppHandle,
    image_path: String,
) -> Result<ImageOcrResult, String> {
    let image_path = PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }

    let tesseract = find_tesseract_executable(app)?;
    let tessdata_dir = ocr_asset_dir(app)?;
    if !tessdata_dir.join("eng.traineddata").exists()
        || !tessdata_dir.join("chi_sim.traineddata").exists()
    {
        return Err("请先在偏好设置中下载图片 OCR 资源".to_string());
    }

    let output = Command::new(&tesseract)
        .arg(&image_path)
        .arg("stdout")
        .arg("-l")
        .arg("chi_sim+eng")
        .arg("--tessdata-dir")
        .arg(&tessdata_dir)
        .arg("-c")
        .arg("tessedit_create_tsv=1")
        .output()
        .map_err(|error| format!("无法启动 Tesseract：{error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Tesseract 识别失败".to_string()
        } else {
            stderr
        });
    }

    let tsv = String::from_utf8_lossy(&output.stdout);
    let words = parse_tesseract_tsv(&tsv);
    let text = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(ImageOcrResult {
        text,
        engine: tesseract.to_string_lossy().to_string(),
        language: "chi_sim+eng".to_string(),
        words,
    })
}

#[cfg(not(target_os = "macos"))]
fn find_tesseract_executable(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_tesseract = ocr_engine_dir(app)?.join("tesseract.exe");
    if app_data_tesseract.exists() {
        return Ok(app_data_tesseract);
    }

    Err("未找到 Tesseract 引擎。请先在偏好设置中下载图片 OCR 资源。".to_string())
}

#[cfg(not(target_os = "macos"))]
fn parse_tesseract_tsv(tsv: &str) -> Vec<ImageOcrWord> {
    tsv.lines()
        .skip(1)
        .filter_map(parse_tesseract_tsv_line)
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn parse_tesseract_tsv_line(line: &str) -> Option<ImageOcrWord> {
    let columns = line.split('\t').collect::<Vec<_>>();
    if columns.len() < 12 || columns.first()? != &"5" {
        return None;
    }

    let text = columns[11].trim();
    let confidence = columns[10].parse::<f64>().ok()?;
    if text.is_empty() || confidence < 0.0 {
        return None;
    }

    Some(ImageOcrWord {
        text: text.to_string(),
        left: parse_tsv_number(columns[6])?,
        top: parse_tsv_number(columns[7])?,
        width: parse_tsv_number(columns[8])?,
        height: parse_tsv_number(columns[9])?,
        confidence,
        block_index: columns[2].parse::<i64>().ok()?,
        paragraph_index: columns[3].parse::<i64>().ok()?,
        line_index: columns[4].parse::<i64>().ok()?,
        word_index: columns[5].parse::<i64>().ok()?,
    })
}

#[cfg(target_os = "macos")]
fn recognize_image_text_macos(image_path: String) -> Result<ImageOcrResult, String> {
    let image_path = PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }

    let (image_width, image_height) = image::image_dimensions(&image_path)
        .map_err(|error| format!("无法读取图片尺寸：{error}"))?;
    if image_width == 0 || image_height == 0 {
        return Err("图片尺寸无效".to_string());
    }

    autoreleasepool(|_| recognize_image_text_macos_inner(&image_path, image_width, image_height))
}

#[cfg(target_os = "macos")]
fn recognize_image_text_macos_inner(
    image_path: &Path,
    image_width: u32,
    image_height: u32,
) -> Result<ImageOcrResult, String> {
    let url = NSURL::from_file_path(image_path).ok_or_else(|| "无法读取图片路径".to_string())?;
    let request_class = AnyClass::get(c"VNRecognizeTextRequest")
        .ok_or_else(|| "当前 macOS 不支持系统图片 OCR".to_string())?;
    let handler_class = AnyClass::get(c"VNImageRequestHandler")
        .ok_or_else(|| "当前 macOS 不支持系统图片 OCR".to_string())?;

    let request: Retained<AnyObject> = unsafe { msg_send![request_class, new] };
    configure_macos_text_request(&request);

    let handler_alloc: *mut AnyObject = unsafe { msg_send![handler_class, alloc] };
    let handler_raw: *mut AnyObject = unsafe {
        msg_send![
            handler_alloc,
            initWithURL: &*url,
            options: None::<&AnyObject>
        ]
    };
    let handler = unsafe { Retained::from_raw(handler_raw) }
        .ok_or_else(|| "无法初始化系统图片 OCR".to_string())?;
    let requests = NSArray::from_slice(&[&*request]);
    let mut error: Option<Retained<NSError>> = None;
    let performed: Bool = unsafe {
        msg_send![
            &*handler,
            performRequests: &*requests,
            error: &mut error
        ]
    };

    if !performed.as_bool() {
        return Err(error
            .map(|error| format!("系统图片 OCR 识别失败：{error}"))
            .unwrap_or_else(|| "系统图片 OCR 识别失败".to_string()));
    }

    let observations: Option<Retained<NSArray<AnyObject>>> =
        unsafe { msg_send![&*request, results] };
    let Some(observations) = observations else {
        return Ok(ImageOcrResult {
            text: String::new(),
            engine: MACOS_OCR_ENGINE_ID.to_string(),
            language: MACOS_OCR_LANGUAGE.to_string(),
            words: Vec::new(),
        });
    };

    let mut words = Vec::new();
    let mut lines = Vec::new();
    let observation_count = observations.count();
    for observation_index in 0..observation_count {
        let observation = observations.objectAtIndex(observation_index);
        let candidates = macos_top_text_candidates(&observation, 1);
        let Some(candidate) =
            candidates.and_then(|items| (items.count() > 0).then(|| items.objectAtIndex(0)))
        else {
            continue;
        };

        let line_text = macos_recognized_text_string(&candidate);
        if line_text.trim().is_empty() {
            continue;
        }
        lines.push(line_text.clone());

        let line_confidence = macos_recognized_text_confidence(&candidate) as f64 * 100.0;
        let tokens = macos_ocr_tokens(&line_text);
        if tokens.is_empty() {
            if let Some(bounding_box) = macos_recognized_text_bounding_box(
                &candidate,
                NSRange::new(0, candidate_string_utf16_len(&candidate)),
            ) {
                words.push(macos_ocr_word_from_bounding_box(
                    line_text.trim().to_string(),
                    bounding_box,
                    image_width,
                    image_height,
                    line_confidence,
                    observation_index as i64,
                    0,
                    observation_index as i64,
                    0,
                ));
            }
            continue;
        }

        for (word_index, token) in tokens.into_iter().enumerate() {
            let bounding_box =
                macos_recognized_text_bounding_box(&candidate, token.range).or_else(|| {
                    macos_recognized_text_bounding_box(
                        &candidate,
                        NSRange::new(0, candidate_string_utf16_len(&candidate)),
                    )
                });
            if let Some(bounding_box) = bounding_box {
                words.push(macos_ocr_word_from_bounding_box(
                    token.text,
                    bounding_box,
                    image_width,
                    image_height,
                    line_confidence,
                    observation_index as i64,
                    0,
                    observation_index as i64,
                    word_index as i64,
                ));
            }
        }
    }

    Ok(ImageOcrResult {
        text: lines.join("\n"),
        engine: MACOS_OCR_ENGINE_ID.to_string(),
        language: MACOS_OCR_LANGUAGE.to_string(),
        words,
    })
}

#[cfg(target_os = "macos")]
fn configure_macos_text_request(request: &AnyObject) {
    unsafe {
        let _: () = msg_send![
            request,
            setRecognitionLevel: MACOS_OCR_RECOGNITION_LEVEL_ACCURATE
        ];
        let _: () = msg_send![request, setUsesLanguageCorrection: Bool::YES];
        let supports_languages: Bool =
            msg_send![request, respondsToSelector: sel!(setRecognitionLanguages:)];
        if supports_languages.as_bool() {
            let zh = NSString::from_str("zh-Hans");
            let en = NSString::from_str("en-US");
            let languages = NSArray::from_slice(&[&*zh, &*en]);
            let _: () = msg_send![request, setRecognitionLanguages: &*languages];
        }
        let supports_language_detection: Bool =
            msg_send![request, respondsToSelector: sel!(setAutomaticallyDetectsLanguage:)];
        if supports_language_detection.as_bool() {
            let _: () = msg_send![request, setAutomaticallyDetectsLanguage: Bool::YES];
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_top_text_candidates(
    observation: &AnyObject,
    max_candidates: NSUInteger,
) -> Option<Retained<NSArray<AnyObject>>> {
    unsafe { msg_send![observation, topCandidates: max_candidates] }
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_string(candidate: &AnyObject) -> String {
    let value: Retained<NSString> = unsafe { msg_send![candidate, string] };
    value.to_string()
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_confidence(candidate: &AnyObject) -> f32 {
    unsafe { msg_send![candidate, confidence] }
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_bounding_box(candidate: &AnyObject, range: NSRange) -> Option<CGRect> {
    if range.length == 0 {
        return None;
    }

    let mut error: Option<Retained<NSError>> = None;
    let box_observation: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            candidate,
            boundingBoxForRange: range,
            error: &mut error
        ]
    };
    let box_observation = box_observation?;
    let bounding_box: CGRect = unsafe { msg_send![&*box_observation, boundingBox] };

    if error.is_some() || bounding_box.size.width <= 0.0 || bounding_box.size.height <= 0.0 {
        None
    } else {
        Some(bounding_box)
    }
}

#[cfg(target_os = "macos")]
fn macos_ocr_word_from_bounding_box(
    text: String,
    bounding_box: CGRect,
    image_width: u32,
    image_height: u32,
    confidence: f64,
    block_index: i64,
    paragraph_index: i64,
    line_index: i64,
    word_index: i64,
) -> ImageOcrWord {
    let image_width = image_width as f64;
    let image_height = image_height as f64;
    let left = bounding_box.origin.x * image_width;
    let top = (1.0 - bounding_box.origin.y - bounding_box.size.height) * image_height;
    let width = bounding_box.size.width * image_width;
    let height = bounding_box.size.height * image_height;

    ImageOcrWord {
        text,
        left: left.max(0.0),
        top: top.max(0.0),
        width: width.max(1.0),
        height: height.max(1.0),
        confidence,
        block_index,
        paragraph_index,
        line_index,
        word_index,
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct MacOcrToken {
    text: String,
    range: NSRange,
}

#[cfg(target_os = "macos")]
fn macos_ocr_tokens(text: &str) -> Vec<MacOcrToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_start = 0_usize;
    let mut utf16_offset = 0_usize;
    let mut current_is_cjk = false;

    for char in text.chars() {
        let char_len = char.len_utf16();
        if char.is_whitespace() {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            utf16_offset += char_len;
            current_is_cjk = false;
            continue;
        }

        let is_cjk = is_cjk_char(char);
        if current.is_empty() {
            current_start = utf16_offset;
            current_is_cjk = is_cjk;
        } else if is_cjk || current_is_cjk {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            current_start = utf16_offset;
            current_is_cjk = is_cjk;
        }

        current.push(char);
        utf16_offset += char_len;

        if is_cjk {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            current_is_cjk = false;
        }
    }

    push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
    tokens
}

#[cfg(target_os = "macos")]
fn push_macos_ocr_token(
    tokens: &mut Vec<MacOcrToken>,
    current: &mut String,
    start: usize,
    end: usize,
) {
    let value = current.trim();
    if !value.is_empty() && end > start {
        tokens.push(MacOcrToken {
            text: value.to_string(),
            range: NSRange::new(start, end - start),
        });
    }
    current.clear();
}

#[cfg(target_os = "macos")]
fn candidate_string_utf16_len(candidate: &AnyObject) -> usize {
    let value: Retained<NSString> = unsafe { msg_send![candidate, string] };
    value.length()
}

#[cfg(target_os = "macos")]
fn is_cjk_char(char: char) -> bool {
    matches!(
        char as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

#[cfg(not(target_os = "macos"))]
fn parse_tsv_number(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

#[allow(dead_code)]
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
