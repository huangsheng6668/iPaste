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
use crate::events::EVENT_SHORTCUT_OPENED;
use crate::lan_sync::commands::{
    device_delete, device_disconnect, device_list, device_request_clip, device_revoke,
    device_send_category, device_send_clip, device_set_auto_sync, open_lan_sync,
    pairing_cancel_invite, pairing_create_invite, pairing_join, pairing_pending,
    pairing_respond, sync_transport_settings_get, sync_transport_settings_set,
    sync_auto_push_settings_get, sync_auto_push_settings_set,
};
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
pub(crate) const DEFAULT_OCR_ENGINE: &str = "local";
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
            commands::clips::get_snapshot,
            commands::clips::list_clips,
            commands::clips::search_with_fallback,
            commands::clips::delete_clip,
            commands::clips::clear_clips,
            commands::clips::rename_clip,
            commands::clips::update_clip_content,
            commands::clips::set_clip_pinned,
            commands::clips::copy_clip,
            commands::clips::apply_clip,
            commands::categories::list_categories,
            commands::categories::list_category_items,
            commands::categories::reorder_categories,
            commands::categories::reorder_category_items,
            commands::categories::create_category,
            commands::categories::create_category_with_clip,
            commands::categories::update_category,
            commands::categories::delete_category,
            commands::categories::add_clip_to_category,
            commands::categories::remove_category_item,
            commands::settings::set_listening,
            commands::settings::set_append_copy_enabled,
            commands::settings::update_settings,
            commands::settings::update_append_copy_timeout,
            commands::settings::update_shortcut,
            commands::settings::update_ocr_shortcut,
            commands::settings::set_app_shortcut_enabled,
            commands::settings::update_panel_open_behavior,
            commands::settings::update_panel_layout,
            commands::settings::update_ocr_mode,
            commands::settings::update_ocr_engine,
            commands::settings::update_language,
            commands::settings::get_app_info,
            commands::settings::enable_autostart,
            commands::settings::disable_autostart,
            commands::settings::is_autostart_enabled,
            commands::settings::open_accessibility_settings,
            commands::settings::open_screen_recording_settings,
            commands::settings::accessibility_permission_status,
            commands::ocr::get_ocr_install_status,
            commands::ocr::install_ocr_assets,
            commands::ocr::remove_ocr_assets,
            commands::ocr::get_mocr_install_status,
            commands::ocr::install_mocr_assets,
            commands::ocr::remove_mocr_assets,
            commands::ocr::recognize_image_text,
            commands::ocr::update_openai_ocr_config,
            commands::ocr::clear_openai_ocr_config,
            commands::ocr::test_openai_ocr,
            commands::ocr::get_ocr_result_payload,
            commands::ocr::screen_capture_permission_status,
            commands::ocr::request_screen_capture_permission,
            commands::cloud::update_cloud_settings,
            commands::cloud::disable_cloud_sync,
            commands::cloud::test_cloud_settings,
            commands::cloud::sync_cloud_now,
            commands::cloud::sync_cloud_in_background,
            commands::window_cmds::show_panel,
            commands::window_cmds::show_settings,
            commands::window_cmds::hide_panel,
            commands::window_cmds::hide_settings,
            commands::window_cmds::set_main_window_dragging,
            commands::window_cmds::start_main_window_drag,
            commands::window_cmds::open_clip_viewer,
            commands::window_cmds::close_clip_viewer,
            commands::automations::list_automations,
            commands::automations::create_automation,
            commands::automations::update_automation,
            commands::automations::delete_automation,
            commands::automations::run_automation,
            commands::automations::get_automation_run,
            commands::screenshot::start_screenshot_ocr,
            commands::screenshot::submit_screenshot_selection,
            commands::screenshot::cancel_screenshot_ocr,
            device_list,
            device_revoke,
            device_delete,
            device_disconnect,
            device_set_auto_sync,
            pairing_create_invite,
            pairing_cancel_invite,
            pairing_join,
            pairing_respond,
            pairing_pending,
            device_send_clip,
            device_send_category,
            device_request_clip,
            sync_transport_settings_get,
            sync_transport_settings_set,
            sync_auto_push_settings_get,
            sync_auto_push_settings_set,
            open_lan_sync
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

            // 跨设备同步：加载设备身份 → 起 iroh endpoint（中继：自定义优先，否则
            // n0 默认）。setup 是同步闭包而 start 是 async——block_on 对齐现有
            // async 初始化模式。任何一步失败都只记日志、不 manage registry：
            // 应用继续运行，命令层对缺失的 registry 优雅报错（见 DeviceRegistryExt）。
            match lan_sync::identity::load_or_create_device_secret() {
                Ok(secret) => {
                    // 存储的中继地址无效/读取失败时回落 n0 默认——不因一条坏设置
                    // 砖掉整个同步。
                    let relay_mode = match store.sync_relay_url() {
                        Ok(Some(url)) => match url.parse::<iroh::RelayUrl>() {
                            Ok(relay_url) => {
                                iroh::RelayMode::Custom(iroh::RelayMap::from_iter([relay_url]))
                            }
                            Err(error) => {
                                eprintln!(
                                    "[lan-sync] 存储的中继地址无效（{url}），回落 n0 默认：{error}"
                                );
                                iroh::RelayMode::Default
                            }
                        },
                        Ok(None) => iroh::RelayMode::Default,
                        Err(reason) => {
                            eprintln!("[lan-sync] 读取中继设置失败，回落 n0 默认：{reason}");
                            iroh::RelayMode::Default
                        }
                    };
                    let sink = lan_sync::tauri_event_sink(app.handle().clone());
                    let sync_store = store.clone();
                    // 追加复制会话状态与 watcher 共享同一实例：活跃期间 auto 接收
                    // 跳过剪贴板写（防对端内容被 merge 进本地追加缓冲）。
                    let append_state = state.append_copy_state.clone();
                    let registry = tauri::async_runtime::block_on(async move {
                        lan_sync::DeviceLinkRegistry::start(
                            secret,
                            sync_store,
                            sink,
                            relay_mode,
                            append_state,
                        )
                        .await
                    });
                    match registry {
                        Ok(registry) => {
                            app.manage(registry);
                        }
                        Err(reason) => {
                            eprintln!(
                                "[lan-sync] 同步服务启动失败（应用继续运行，同步不可用）：{reason}"
                            );
                        }
                    }
                }
                Err(reason) => {
                    eprintln!(
                        "[lan-sync] 设备身份加载失败（应用继续运行，同步不可用）：{reason}"
                    );
                }
            }

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
                // 跨设备同步：停入站接受循环 + 断开全部链路
                //（endpoint 本体随最后的 Arc 引用释放关闭）
                if let Some(registry) = _app.try_state::<Arc<lan_sync::DeviceLinkRegistry>>() {
                    registry.shutdown();
                }
                tauri::async_runtime::block_on(crate::ocr::mocr::shutdown_server());
            }
        });
}
