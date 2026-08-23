use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use tauri::{menu::MenuItem, Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_global_shortcut::ShortcutState;

mod automation;
mod capture;
mod clipboard;
mod commands;
pub(crate) mod error;
pub(crate) mod events;
mod models;
mod ocr;
mod paste;
mod shortcut;
mod store;
mod tray;
mod util;
mod window;
mod cloud;
mod lan_sync;

use crate::capture::start_screenshot_ocr as run_screenshot_ocr_capture;
use crate::clipboard::spawn_clipboard_watcher;
use crate::commands::{
    get_snapshot, list_clips, search_with_fallback, list_categories, list_category_items,
    reorder_categories, reorder_category_items, create_category, create_category_with_clip,
    update_category, delete_category, add_clip_to_category, remove_category_item, delete_clip,
    clear_clips, rename_clip, update_clip_content, set_clip_pinned, copy_clip, set_listening,
    set_append_copy_enabled, update_settings, update_append_copy_timeout, update_shortcut,
    update_ocr_shortcut,
    set_app_shortcut_enabled, update_panel_open_behavior, update_panel_layout, update_ocr_mode,
    update_language, update_cloud_settings, disable_cloud_sync, test_cloud_settings, get_app_info,
    get_ocr_install_status, install_ocr_assets, remove_ocr_assets, recognize_image_text,
    get_mocr_install_status, install_mocr_assets, remove_mocr_assets,
    start_screenshot_ocr, submit_screenshot_selection, cancel_screenshot_ocr,
    get_ocr_result_payload,
    sync_cloud_now, sync_cloud_in_background, list_automations, create_automation,
    update_automation, delete_automation, run_automation, get_automation_run, show_panel,
    show_settings, open_clip_viewer, close_clip_viewer, hide_panel, hide_settings,
    open_accessibility_settings, open_screen_recording_settings, request_screen_capture_permission,
    screen_capture_permission_status, accessibility_permission_status,
    enable_autostart, disable_autostart, is_autostart_enabled,
    set_main_window_dragging, start_main_window_drag, apply_clip,
};
use crate::events::EVENT_SHORTCUT_OPENED;
use crate::lan_sync::commands::{
    lan_create_session, lan_join_by_address, lan_accept_pair, lan_send_clip, lan_send_category,
    lan_request_clip, lan_disconnect, lan_get_state, open_lan_sync, lan_get_port_conflict,
    lan_kill_port_process, lan_quit_app,
};
use crate::lan_sync::LanSessionManager;
use crate::models::{AppendCopyState, AppState, MainWindowActivation};
use crate::paste::{current_main_window_activation, remember_target_app_for_paste};
use crate::shortcut::{register_app_shortcut, shortcut_matches};
use crate::store::Store;
use crate::tray::{
    build_tray, handle_append_copy_menu, handle_pause_capture_menu, handle_settings_menu,
    handle_show_menu,
};
use crate::util::localized_text;
use crate::window::{hide_main_window, show_main_window, MAIN_WINDOW};

pub(crate) const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+V";
pub(crate) const DEFAULT_OCR_SHORTCUT: &str = "CommandOrControl+Shift+O";
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
                    let (active_shortcut, active_ocr_shortcut) = {
                        let panel = state
                            .active_shortcut
                            .lock()
                            .map(|value| value.clone())
                            .ok();
                        let ocr = state
                            .active_ocr_shortcut
                            .lock()
                            .map(|value| value.clone())
                            .ok();
                        match (panel, ocr) {
                            (Some(panel), Some(ocr)) => (panel, ocr),
                            _ => return,
                        }
                    };

                    if shortcut_matches(shortcut, &active_shortcut) {
                        remember_target_app_for_paste(app);
                        let app = app.clone();
                        thread::spawn(move || {
                            // 使用 Activate 模式：native panel（PreserveCurrentApp）模式下
                            // iPaste 从不激活，粘贴时无法通过任何 API 把 key window 转移给
                            // 目标应用（诊断确认 key window 悬空、AX 设置只读、open -b 无效）。
                            // Activate 模式下面板隐藏时系统自动把激活和键盘焦点还给目标应用。
                            let _ = show_main_window(&app, MainWindowActivation::Activate);
                            let _ = app.emit(EVENT_SHORTCUT_OPENED, active_shortcut);
                        });
                        return;
                    }

                    if shortcut_matches(shortcut, &active_ocr_shortcut) {
                        let app = app.clone();
                        thread::spawn(move || {
                            if let Err(error) = run_screenshot_ocr_capture(&app) {
                                eprintln!("screenshot ocr start failed: {error}");
                            }
                        });
                    }
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
            update_ocr_shortcut,
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
            get_mocr_install_status,
            install_mocr_assets,
            remove_mocr_assets,
            recognize_image_text,
            start_screenshot_ocr,
            submit_screenshot_selection,
            cancel_screenshot_ocr,
            get_ocr_result_payload,
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
            open_screen_recording_settings,
            screen_capture_permission_status,
            accessibility_permission_status,
            request_screen_capture_permission,
            enable_autostart,
            disable_autostart,
            is_autostart_enabled,
            set_main_window_dragging,
            start_main_window_drag,
            apply_clip,
            lan_create_session,
            lan_join_by_address,
            lan_accept_pair,
            lan_send_clip,
            lan_send_category,
            lan_request_clip,
            lan_disconnect,
            lan_get_state,
            open_lan_sync,
            lan_get_port_conflict,
            lan_kill_port_process,
            lan_quit_app
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
            let ocr_menu_item = MenuItem::with_id(
                app,
                "screenshot-ocr",
                localized_text(&settings.language, "screenshot_ocr"),
                true,
                Some(settings.ocr_shortcut.as_str()),
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
                active_ocr_shortcut: Arc::new(Mutex::new(settings.ocr_shortcut.clone())),
                ocr_menu_item: ocr_menu_item.clone(),
                is_app_shortcut_enabled: Arc::new(Mutex::new(true)),
                capture_session: Arc::new(Mutex::new(None)),
                ocr_result_payloads: Arc::new(Mutex::new(std::collections::HashMap::new())),
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
            app.manage(std::sync::Arc::new(LanSessionManager::new(
                app.handle().clone(),
            )));
            build_tray(
                app.handle(),
                show_menu_item,
                ocr_menu_item.clone(),
                append_copy_menu_item,
                pause_capture_menu_item,
                settings_menu_item,
                quit_menu_item,
                settings.language.as_str(),
            )?;
            register_app_shortcut(app.handle(), &settings.shortcut)?;
            register_app_shortcut(app.handle(), &settings.ocr_shortcut)?;
            show_main_window(app.handle(), MainWindowActivation::Activate)?;

            let app_handle = app.handle().clone();
            thread::spawn(move || {
                if let Err(error) = crate::capture::overlay::prewarm_overlay_windows(&app_handle) {
                    eprintln!("overlay window prewarm failed: {error}");
                }
            });

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
            "show" => handle_show_menu(app),
            "screenshot-ocr" => {
                let app = app.clone();
                thread::spawn(move || {
                    if let Err(error) = run_screenshot_ocr_capture(&app) {
                        eprintln!("screenshot ocr start failed: {error}");
                    }
                });
            }
            "settings" => handle_settings_menu(app),
            "append-copy" => {
                if let Some(state) = app.try_state::<AppState>() {
                    handle_append_copy_menu(app, &state);
                }
            }
            "pause" => {
                if let Some(state) = app.try_state::<AppState>() {
                    handle_pause_capture_menu(app, &state);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // 常驻推理子进程不会随应用退出自动终止：退出时显式回收，避免孤儿进程占内存
            if let tauri::RunEvent::Exit = event {
                tauri::async_runtime::block_on(crate::ocr::mocr::shutdown_server());
            }
        });
}
