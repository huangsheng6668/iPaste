use std::{thread, time::Duration};
#[cfg(target_os = "macos")]
use std::{ffi::c_int, ffi::c_void, process::Command, time::Instant};

#[cfg(target_os = "macos")]
use objc2::ffi::NSUInteger;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWorkspace,
};
#[cfg(target_os = "macos")]
use objc2_foundation::NSString;
use tauri::Manager;

use crate::models::*;

#[cfg(target_os = "macos")]
const PASTE_FOCUS_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(target_os = "macos")]
const PASTE_FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(30);
#[cfg(target_os = "macos")]
const AX_FOCUS_WAIT_TIMEOUT: Duration = Duration::from_millis(1200);
#[cfg(target_os = "macos")]
const AX_FOCUS_POLL_INTERVAL: Duration = Duration::from_millis(40);
#[cfg(target_os = "macos")]
const KAX_ERROR_APIDISABLED: i32 = -25211;

#[cfg(target_os = "macos")]
type CFTypeRef = *const c_void;
#[cfg(target_os = "macos")]
type CFStringRef = CFTypeRef;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn GetProcessForPID(pid: c_int, psn: *mut ProcessSerialNumber) -> i32;
    fn SetFrontProcessWithOptions(psn: *const ProcessSerialNumber, options: u32) -> i32;
    fn AXUIElementCreateApplication(pid: c_int) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: CFTypeRef,
        attribute: CFStringRef,
        value: CFTypeRef,
    ) -> i32;
    fn CFRelease(value: CFTypeRef);
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFBooleanTrue: CFTypeRef;
}

#[cfg(target_os = "macos")]
const SET_FRONT_PROCESS_FRONT_WINDOW_ONLY: u32 = 1;
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

pub(crate) fn prepare_target_for_paste(
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
pub(crate) fn activate_app_for_paste(app: &tauri::AppHandle, bundle_id: &str) -> Result<(), String> {
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
pub(crate) fn pid_for_bundle_id(bundle_id: &str) -> Option<c_int> {
    let target_bundle_id = NSString::from_str(bundle_id);
    let applications =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&target_bundle_id);
    let application = unsafe { applications.firstObject_unchecked() }?;
    Some(application.processIdentifier() as c_int)
}

#[cfg(target_os = "macos")]
fn ax_attribute_string(name: &str) -> Retained<NSString> {
    NSString::from_str(name)
}

#[cfg(target_os = "macos")]
pub(crate) fn focus_target_app_window(pid: c_int) -> Result<(), String> {
    let ax_app = unsafe { AXUIElementCreateApplication(pid) };
    if ax_app.is_null() {
        return Err("无法自动粘贴：目标应用不在运行，请重新打开 iPaste 面板后再粘贴。".to_string());
    }

    // AX 属性名常量（AXMainWindow / AXFocusedWindow）通过 toll-free bridging 传入
    let focused_attr = ax_attribute_string("AXFocusedWindow");
    let main_attr = ax_attribute_string("AXMainWindow");

    // 探测一次：AX API 被系统禁用（辅助功能权限未生效）时直接提示，避免静默等待超时
    let mut probe: CFTypeRef = std::ptr::null();
    let probe_status = unsafe {
        AXUIElementCopyAttributeValue(ax_app, Retained::as_ptr(&focused_attr) as CFStringRef, &mut probe)
    };
    if probe_status == KAX_ERROR_APIDISABLED {
        unsafe { CFRelease(ax_app) };
        return Err(
            "无法自动粘贴：macOS 辅助功能权限未生效，请在「系统设置 > 隐私与安全性 > 辅助功能」中移除 iPaste 后重新添加，或重启 iPaste 后再试。"
                .to_string(),
        );
    }

    if app_has_focused_window(ax_app, &focused_attr) {
        unsafe { CFRelease(ax_app) };
        return Ok(());
    }

    // 强制把目标应用的主窗口设为聚焦窗口，触发键盘焦点转移
    let mut main_window: CFTypeRef = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(ax_app, Retained::as_ptr(&main_attr) as CFStringRef, &mut main_window)
    };
    if status == 0 && !main_window.is_null() {
        unsafe {
            AXUIElementSetAttributeValue(main_window, Retained::as_ptr(&main_attr) as CFStringRef, kCFBooleanTrue);
            CFRelease(main_window);
        }
    }

    // 轮询等待目标应用真正获得键盘焦点（focused window 就绪）后再粘贴
    let deadline = Instant::now() + AX_FOCUS_WAIT_TIMEOUT;
    while !app_has_focused_window(ax_app, &focused_attr) {
        if Instant::now() >= deadline {
            unsafe { CFRelease(ax_app) };
            return Err(
                "无法自动粘贴：目标应用窗口未能获得键盘焦点，请确认目标窗口可见后重试。"
                    .to_string(),
            );
        }
        thread::sleep(AX_FOCUS_POLL_INTERVAL);
    }

    unsafe { CFRelease(ax_app) };
    Ok(())
}

#[cfg(target_os = "macos")]
fn app_has_focused_window(ax_app: CFTypeRef, focused_attr: &NSString) -> bool {
    let mut focused: CFTypeRef = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(
            ax_app,
            focused_attr as *const NSString as CFStringRef,
            &mut focused,
        )
    };
    if status == 0 && !focused.is_null() {
        unsafe { CFRelease(focused) };
        return true;
    }
    false
}

#[cfg(target_os = "macos")]
pub(crate) fn activate_running_app_for_paste(
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
    let target_bundle_id = NSString::from_str(bundle_id);
    let applications =
        NSRunningApplication::runningApplicationsWithBundleIdentifier(&target_bundle_id);
    let Some(target) = (unsafe { applications.firstObject_unchecked() }) else {
        return Err("无法自动粘贴：目标应用已退出，请重新打开 iPaste 面板后再粘贴。".to_string());
    };

    let _ = target.unhide();
    let pid = target.processIdentifier();

    // 优先使用标准激活（触发目标应用的 NSApplication activate 流程，
    // 窗口才会 makeKeyWindow、键盘焦点才会转移）。SetFrontProcessWithOptions
    // 只把进程置前，不触发标准激活，key window 不会转移，Cmd+V 会投递失败。
    // 注意：activateFromApplication_options 要求当前应用处于激活状态，
    // 因此先尝试激活，失败后才 deactivate 并回退 SetFrontProcessWithOptions。
    let activation_options = NSApplicationActivationOptions(
        NSApplicationActivationOptions::ActivateAllWindows.bits() | (1 as NSUInteger) << 1,
    );
    let current_app = NSRunningApplication::currentApplication();
    let activated = target.activateFromApplication_options(&current_app, activation_options)
        || target.activateWithOptions(activation_options);
    if !activated {
        deactivate_current_application_for_paste();
        if set_front_process_for_pid(pid as c_int).is_ok() {
            return Ok(false);
        }
        return Err("无法自动粘贴：无法切回目标应用，请确认目标窗口仍可用。".to_string());
    }

    Ok(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn deactivate_current_application_for_paste() {
    if let Some(marker) = objc2::MainThreadMarker::new() {
        NSApplication::sharedApplication(marker).deactivate();
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn set_front_process_for_pid(pid: c_int) -> Result<(), String> {
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
pub(crate) fn open_app_bundle_for_paste(bundle_id: &str) -> bool {
    Command::new("open")
        .arg("-b")
        .arg(bundle_id)
        .spawn()
        .map(|_| true)
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub(crate) fn current_frontmost_app_bundle_id_for_paste(app: &tauri::AppHandle) -> Option<String> {
    run_on_main_thread_for_paste(app, current_frontmost_app_bundle_id)
        .ok()
        .flatten()
}

#[cfg(target_os = "macos")]
pub(crate) fn wait_for_frontmost_app(
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
pub(crate) fn current_frontmost_app_bundle_id() -> Option<String> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .and_then(|application| application.bundleIdentifier())
        .map(|bundle_id| bundle_id.to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn current_app_bundle_id(app: &tauri::AppHandle) -> Option<String> {
    NSRunningApplication::currentApplication()
        .bundleIdentifier()
        .map(|bundle_id| bundle_id.to_string())
        .or_else(|| Some(app.config().identifier.clone()))
}

