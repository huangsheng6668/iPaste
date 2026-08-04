use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use rusqlite::Connection;
use tauri::{menu::MenuItem, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;
use uuid::Uuid;

mod automation;
mod clipboard;
use crate::clipboard::*;

mod commands;
use crate::commands::*;

mod models;
use crate::models::*;

mod ocr;

mod paste;
use crate::paste::*;

mod shortcut;
use crate::shortcut::*;

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

#[allow(dead_code)]
const CLOUD_SYNC_TYPES: [&str; 4] = ["text", "link", "color", "html"];



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
            search_with_fallback,
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
            list_automations,
            create_automation,
            update_automation,
            delete_automation,
            run_automation,
            get_automation_run,
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

pub(crate) fn map_automation(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationAction> {
    Ok(AutomationAction {
        id: row.get(0)?,
        name: row.get(1)?,
        command: row.get(2)?,
        cwd: row.get(3)?,
        run_mode: row.get(4)?,
        confirm_before_run: row.get(5)?,
        close_panel_on_success: row.get(6)?,
        sort_order: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_run: None,
    })
}

pub(crate) fn map_automation_run_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRunSummary> {
    Ok(AutomationRunSummary {
        id: row.get(0)?,
        status: row.get(1)?,
        exit_code: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        duration_ms: row.get(5)?,
    })
}

pub(crate) fn map_automation_run_detail(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRunDetail> {
    Ok(AutomationRunDetail {
        id: row.get(0)?,
        automation_id: row.get(1)?,
        status: row.get(2)?,
        exit_code: row.get(3)?,
        stdout: row.get(4)?,
        stderr: row.get(5)?,
        stdout_truncated: row.get(6)?,
        stderr_truncated: row.get(7)?,
        started_at: row.get(8)?,
        finished_at: row.get(9)?,
        duration_ms: row.get(10)?,
    })
}
