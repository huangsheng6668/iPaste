#[cfg(target_os = "macos")]
use objc2::{
    define_class,
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject, Bool},
    sel, ClassType, MainThreadOnly,
};
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSFloatingWindowLevel,
    NSPanel, NSResponder, NSView, NSWindow, NSWindowAnimationBehavior,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSPoint, NSObjectProtocol, NSRect};
#[cfg(target_os = "windows")]
use tauri::PhysicalSize;
use tauri::{
    utils::config::Color,
    Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder,
};

use crate::models::*;
use crate::util::*;
use crate::{
    current_main_window_activation, remember_main_window_activation, remember_target_app_for_paste,
    DEFAULT_LANGUAGE,
};
#[cfg(target_os = "macos")]
use crate::run_on_main_thread_for_paste;

pub(crate) const MAIN_WINDOW: &str = "main";
pub(crate) const SETTINGS_WINDOW: &str = "settings";
pub(crate) const CLIP_VIEWER_WINDOW_PREFIX: &str = "clip-viewer-";
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
        "ipaste://panel-visibility-changed",
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

pub(crate) fn show_settings_window(app: &tauri::AppHandle) -> Result<(), String> {
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

pub(crate) fn show_clip_viewer_window(
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
#[cfg(target_os = "macos")]
pub(crate) fn start_native_main_panel_drag(app: &tauri::AppHandle) -> Result<bool, String> {
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
pub(crate) fn start_native_main_panel_drag(_app: &tauri::AppHandle) -> Result<bool, String> {
    Ok(false)
}
