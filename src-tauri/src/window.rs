//! 多窗口编排：主面板显隐/激活流、辅助窗口（设置/放大预览/设备同步/OCR 结果）
//! 的泛化创建与显示。定位数学在 `positioning`，macOS NSPanel 原生管理在 `macos_panel`。

pub(crate) mod macos_panel;
pub(crate) mod positioning;

use tauri::{
    utils::config::Color,
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};

use crate::events::{EVENT_PANEL_VISIBILITY_CHANGED, PanelVisibilityChanged};
use crate::models::{AppState, MainWindowActivation, WindowGeometry};
use crate::util::{localized_text, percent_encode_component};
use crate::{
    paste::{
        current_main_window_activation, remember_main_window_activation,
        remember_target_app_for_paste,
    },
    DEFAULT_LANGUAGE,
};

use macos_panel::{
    configure_main_window_activation, hide_main_window_preserving_current_app,
    hide_native_main_panel, is_native_main_panel_visible, restore_main_webview_to_host_window,
    show_main_window_with_native_panel,
};
use positioning::{
    apply_window_geometry_for_monitor, position_clip_viewer_window,
    position_window_centered_on_monitor, position_window_near_cursor, CLIP_VIEWER_WINDOW_GEOMETRY,
    LAN_SYNC_WINDOW_GEOMETRY, MAIN_WINDOW_GEOMETRY, OCR_RESULT_WINDOW_GEOMETRY,
    SETTINGS_WINDOW_GEOMETRY, SIDE_MAIN_WINDOW_GEOMETRY,
};

pub(crate) const MAIN_WINDOW: &str = "main";
pub(crate) const SETTINGS_WINDOW: &str = "settings";
pub(crate) const CLIP_VIEWER_WINDOW_PREFIX: &str = "clip-viewer-";
pub(crate) const LAN_SYNC_WINDOW: &str = "lan-sync";
/// 截图 OCR 结果窗标签前缀：按 token 拼唯一标签（clip-viewer 同法），
/// 规避同标签销毁-重建竞态（destroy 为异步消息，同标签 get-or-create 会复用垂死窗口）。
pub(crate) const OCR_RESULT_WINDOW_PREFIX: &str = "ocr-result-";
pub(crate) const OCR_OVERLAY_WINDOW_PREFIX: &str = "ocr-overlay-";

pub(crate) fn current_main_window_geometry(app: &tauri::AppHandle) -> WindowGeometry {
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

pub(crate) fn apply_main_window_layout_geometry(app: &tauri::AppHandle, layout: &str) -> Result<(), String> {
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

pub(crate) fn show_main_window(
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
        EVENT_PANEL_VISIBILITY_CHANGED,
        PanelVisibilityChanged {
            visible: true,
            preserves_current_app: effective_activation == MainWindowActivation::PreserveCurrentApp,
            native_panel,
        },
    );
    Ok(())
}

pub(crate) fn hide_main_window(app: &tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window(MAIN_WINDOW)
        .ok_or_else(|| "未找到主面板".to_string())?;
    let activation = current_main_window_activation(app);
    let native_panel = activation == MainWindowActivation::PreserveCurrentApp
        && is_native_main_panel_visible(app);
    let _ = app.emit(
        EVENT_PANEL_VISIBILITY_CHANGED,
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

/// 辅助窗口（设置/放大/LAN）的创建与显示配置。
/// 三个窗口原各有一份逐行同构的「取显示器→get-or-create→定位→show→再定位→focus」，
/// 现收敛为一份泛化实现。
struct AuxiliaryWindowConfig {
    label: String,
    url: String,
    title: String,
    geometry: WindowGeometry,
    decorations: bool,
    always_on_top: bool,
    near_main_window: bool,
    monitor_index: Option<usize>,
}

fn show_auxiliary_window(
    app: &tauri::AppHandle,
    config: AuxiliaryWindowConfig,
) -> Result<(), String> {
    // viewer（near_main_window）定位只依赖主窗口自身（position_clip_viewer_window），
    // 无需查询显示器；其余辅助窗口才取「主窗口所在显示器」用于居中。
    let main_monitor = if config.near_main_window {
        None
    } else {
        app.get_webview_window(MAIN_WINDOW)
            .and_then(|window| window.current_monitor().ok().flatten())
            .or_else(|| app.primary_monitor().ok().flatten())
    };
    let window = if let Some(window) = app.get_webview_window(&config.label) {
        window
    } else {
        let mut builder = WebviewWindowBuilder::new(
            app,
            &config.label,
            WebviewUrl::App(config.url.as_str().into()),
        )
        .title(&config.title)
        .inner_size(config.geometry.width, config.geometry.height)
        .min_inner_size(config.geometry.min_width, config.geometry.min_height)
        .resizable(true)
        .visible(false);
        if !config.decorations {
            builder = builder.decorations(false);
        }
        if config.always_on_top {
            builder = builder.always_on_top(true);
        }
        builder.build().map_err(|error| error.to_string())?
    };

    position_auxiliary_window(app, &window, &main_monitor, &config)?;
    window.show().map_err(|error| error.to_string())?;
    position_auxiliary_window(app, &window, &main_monitor, &config)?;
    if config.always_on_top {
        window
            .set_always_on_top(true)
            .map_err(|error| error.to_string())?;
    }
    window.set_focus().map_err(|error| error.to_string())?;
    Ok(())
}

fn position_auxiliary_window(
    app: &tauri::AppHandle,
    window: &tauri::WebviewWindow,
    main_monitor: &Option<tauri::Monitor>,
    config: &AuxiliaryWindowConfig,
) -> Result<(), String> {
    if let Some(index) = config.monitor_index {
        if let Some(monitor) = app
            .available_monitors()
            .ok()
            .and_then(|monitors| monitors.get(index).cloned())
        {
            position_window_centered_on_monitor(window, &monitor, config.geometry)?;
            return Ok(());
        }
    }
    if config.near_main_window {
        position_clip_viewer_window(app, window)?;
        return Ok(());
    }
    if let Some(monitor) = main_monitor {
        position_window_centered_on_monitor(window, monitor, config.geometry)?;
    } else {
        window.center().map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub(crate) fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
    show_settings_window_with_tab(app, None)
}

/// 预检失败等场景直达指定设置 Tab（"ocr" / "permissions"）。
pub(crate) fn show_settings_window_with_tab(
    app: &tauri::AppHandle,
    tab: Option<&str>,
) -> Result<(), String> {
    let language = app
        .try_state::<AppState>()
        .and_then(|state| state.store.settings().ok())
        .map(|settings| settings.language)
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string());
    let tab_suffix = tab
        .filter(|tab| ["ocr", "permissions"].contains(tab))
        .map(|tab| format!("&tab={tab}"))
        .unwrap_or_default();
    let _ = hide_main_window(app);
    show_auxiliary_window(
        app,
        AuxiliaryWindowConfig {
            label: SETTINGS_WINDOW.to_string(),
            url: format!("index.html?window=settings{tab_suffix}"),
            title: localized_text(&language, "settings_title").to_string(),
            geometry: SETTINGS_WINDOW_GEOMETRY,
            decorations: true,
            always_on_top: false,
            near_main_window: false,
            monitor_index: None,
        },
    )
}

pub(crate) fn show_clip_viewer_window(
    app: &tauri::AppHandle,
    label: String,
    title: String,
    auto_recognize: bool,
) -> Result<(), String> {
    if !label.starts_with(CLIP_VIEWER_WINDOW_PREFIX) {
        return Err("无效的放大窗口标签".to_string());
    }

    // auto_recognize：主面板一键 OCR 入口——打开窗口即自动开始识别图片文字
    let url = format!(
        "index.html?window=clip-viewer&label={}{}",
        percent_encode_component(&label),
        if auto_recognize { "&auto-recognize=1" } else { "" }
    );
    show_auxiliary_window(
        app,
        AuxiliaryWindowConfig {
            label,
            url,
            title,
            geometry: CLIP_VIEWER_WINDOW_GEOMETRY,
            decorations: false,
            always_on_top: true,
            near_main_window: true,
            monitor_index: None,
        },
    )
}

/// 截图 OCR 结果窗：销毁旧结果窗后按「前缀+token」唯一标签重建（单活跃会话）。
pub(crate) fn show_ocr_result_window(
    app: &tauri::AppHandle,
    token: &str,
    monitor_index: usize,
) -> Result<(), String> {
    // destroy() 是 fire-and-forget 事件循环消息：同标签立即 get-or-create 会复用
    // 垂死窗口（旧 token URL）再被排队销毁。改用唯一标签规避；旧窗口按前缀
    // 匹配清理即可，无需等待其销毁完成（新标签不受管理器内陈旧条目影响）。
    let label = format!("{OCR_RESULT_WINDOW_PREFIX}{token}");
    for window in app.webview_windows().values() {
        if window.label().starts_with(OCR_RESULT_WINDOW_PREFIX) {
            let _ = window.destroy();
        }
    }
    let language = app
        .try_state::<AppState>()
        .and_then(|state| state.store.settings().ok())
        .map(|settings| settings.language)
        .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string());
    // 结果窗是 manga profile 的唯一入口：打开即后台预热推理进程，
    // 用户点击「日语 · 漫画」时模型多半已加载完毕（失败静默，不影响冷启动回退）。
    let prewarm_app = app.clone();
    tauri::async_runtime::spawn(async move {
        crate::ocr::mocr::prewarm_server(&prewarm_app).await;
    });
    show_auxiliary_window(
        app,
        AuxiliaryWindowConfig {
            label,
            url: format!(
                "index.html?window=ocr-result&token={}",
                percent_encode_component(token)
            ),
            title: localized_text(&language, "screenshot_ocr").to_string(),
            geometry: OCR_RESULT_WINDOW_GEOMETRY,
            decorations: false,
            always_on_top: true,
            near_main_window: false,
            monitor_index: Some(monitor_index),
        },
    )
}

pub(crate) fn open_lan_sync_window(app: &tauri::AppHandle) -> Result<(), String> {
    show_auxiliary_window(
        app,
        AuxiliaryWindowConfig {
            label: LAN_SYNC_WINDOW.to_string(),
            url: "index.html?window=lan-sync".to_string(),
            title: "iPaste · Device Sync".to_string(),
            geometry: LAN_SYNC_WINDOW_GEOMETRY,
            decorations: false,
            always_on_top: true,
            near_main_window: false,
            monitor_index: None,
        },
    )
}
