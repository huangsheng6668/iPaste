//! macOS NSPanel 原生主面板管理：IPastePanel（非激活面板）生命周期、
//! webview 在宿主窗口与原生面板间借还、原生隐藏/拖拽与激活行为配置。
//! 非 macOS 平台为同名 no-op 回退，window.rs 无条件调用。
//! 本文件的 macOS 分支在 Windows 上不参与编译，以 CI macos-15 验证为准。

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
#[cfg(target_os = "macos")]
use crate::models::{AppState, MainPanelState};
#[cfg(target_os = "macos")]
use crate::paste::run_on_main_thread_for_paste;
use crate::models::MainWindowActivation;

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
pub(crate) fn show_main_window_with_native_panel(
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
pub(crate) fn show_main_window_with_native_panel(
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
pub(crate) fn restore_main_webview_to_host_window(
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
pub(crate) fn restore_main_webview_to_host_window(
    _app: &tauri::AppHandle,
    _window: &tauri::WebviewWindow,
) -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_native_main_panel(app: &tauri::AppHandle) -> Result<bool, String> {
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
pub(crate) fn hide_native_main_panel(_app: &tauri::AppHandle) -> Result<bool, String> {
    Ok(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn is_native_main_panel_visible(app: &tauri::AppHandle) -> bool {
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
pub(crate) fn is_native_main_panel_visible(_app: &tauri::AppHandle) -> bool {
    false
}

#[cfg(target_os = "macos")]
pub(crate) fn hide_main_window_preserving_current_app(
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
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
pub(crate) fn hide_main_window_preserving_current_app(
    window: &tauri::WebviewWindow,
) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn configure_main_window_activation(
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
pub(crate) fn configure_main_window_activation(
    _window: &tauri::WebviewWindow,
    _activation: MainWindowActivation,
) {
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
            let current = {
                let guard = panel_state.lock().map_err(|error| error.to_string())?;
                match *guard {
                    Some(current) if current.visible => current,
                    _ => return Ok(false),
                }
            };

            // performWindowDragWithEvent 会进入嵌套事件循环直到松开鼠标，必须
            // 先释放 panel_state 锁：拖拽期间主线程处理任何需要该锁的任务
            // （如快捷键唤起/隐藏面板）会与自身死锁。
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
