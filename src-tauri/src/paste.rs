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
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use tauri::Manager;

use crate::error::AppError;
use crate::models::{AppState, MainWindowActivation};
#[cfg(target_os = "macos")]
use crate::models::ProcessSerialNumber;

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
    fn AXUIElementCreateSystemWide() -> CFTypeRef;
    fn AXUIElementGetPid(element: CFTypeRef, pid: *mut c_int) -> i32;
    fn AXIsProcessTrusted() -> bool;
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

/// 把粘贴键投递给之前聚焦的目标应用（面板的隐藏/恢复由命令层负责，
/// 避免 paste → window 的模块循环依赖；window.rs 已依赖 paste.rs）。
/// apply_clip 的粘贴段编排（原 commands.rs 内联）。
pub(crate) fn paste_to_previous_app(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<(), AppError> {
    let target_app_bundle_id = state
        .target_app_bundle_id
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .clone();

    prepare_target_for_paste(app, target_app_bundle_id.clone()).map_err(AppError::from)?;

    // 等待并确认目标应用获得键盘焦点，避免 Cmd+V 投递到未就绪的窗口
    #[cfg(target_os = "macos")]
    if let Some(bundle_id) = target_app_bundle_id.as_deref() {
        if let Some(pid) = pid_for_bundle_id(bundle_id) {
            focus_target_app_window(pid).map_err(AppError::from)?;
        }
    }

    send_paste_shortcut().map_err(AppError::from)?;

    Ok(())
}

pub(crate) fn send_paste_shortcut() -> Result<(), String> {
    let mut enigo = Enigo::new(&Settings::default()).map_err(permission_error)?;

    #[cfg(target_os = "macos")]
    {
        enigo.key(Key::Meta, Press).map_err(permission_error)?;
        let paste_result = enigo.key(Key::Other(9), Click).map_err(permission_error);
        let release_result = enigo.key(Key::Meta, Release).map_err(permission_error);
        paste_result?;
        release_result?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        enigo.key(Key::Control, Press).map_err(permission_error)?;
        let paste_result = enigo
            .key(Key::Unicode('v'), Click)
            .map_err(permission_error);
        let release_result = enigo.key(Key::Control, Release).map_err(permission_error);
        paste_result?;
        release_result?;
    }

    Ok(())
}

fn permission_error(error: impl ToString) -> String {
    let message = error.to_string();
    if message.to_lowercase().contains("permission") {
        "无法自动粘贴：请在 macOS「系统设置 > 隐私与安全性 > 辅助功能」中允许当前安装的 iPaste 控制电脑。若已授权，请移除旧的 iPaste 项后重新添加当前 App。"
            .to_string()
    } else {
        message
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn activate_app_for_paste(app: &tauri::AppHandle, bundle_id: &str) -> Result<(), String> {
    if Some(bundle_id) == current_app_bundle_id(app).as_deref() {
        thread::sleep(Duration::from_millis(180));
        return Ok(());
    }

    if Some(bundle_id) == current_frontmost_app_bundle_id_for_paste(app).as_deref() {
        // 目标应用已是 frontmost（native panel 模式下始终如此），但键盘焦点
        // （key window）在面板隐藏后悬空（诊断确认 focused=None）。
        // activateFromApplication_options 要求 sender（iPaste）处于激活状态，
        // native panel 模式下不满足；改用 Launch Services（open -b，等同点击
        // Dock 图标）触发目标应用的标准激活流程，让窗口 makeKeyWindow。
        let _ = open_app_bundle_for_paste(bundle_id);
        let _ = wait_for_frontmost_app(app, bundle_id, PASTE_FOCUS_TIMEOUT);
        thread::sleep(Duration::from_millis(120));
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
    // 系统级 AX 元素：键盘焦点（key window）由系统级 AXFocusedApplication 决定，
    // 目标应用自己的 AXFocusedWindow 无法反映系统键盘焦点。
    let system_wide = unsafe { AXUIElementCreateSystemWide() };
    if system_wide.is_null() {
        return Err("无法自动粘贴：无法创建系统辅助功能句柄。".to_string());
    }
    let focused_application_attr = ax_attribute_string("AXFocusedApplication");

    // 探测一次：AX API 被系统禁用（辅助功能权限未生效）时直接提示，避免静默等待超时
    let mut probe: CFTypeRef = std::ptr::null();
    let probe_status = unsafe {
        AXUIElementCopyAttributeValue(
            system_wide,
            Retained::as_ptr(&focused_application_attr) as CFStringRef,
            &mut probe,
        )
    };
    if probe_status == KAX_ERROR_APIDISABLED {
        unsafe { CFRelease(system_wide) };
        return Err(
            "无法自动粘贴：macOS 辅助功能权限未生效，请在「系统设置 > 隐私与安全性 > 辅助功能」中移除 iPaste 后重新添加，或重启 iPaste 后再试。"
                .to_string(),
        );
    }
    if !probe.is_null() {
        unsafe { CFRelease(probe) };
    }

    let ax_app = unsafe { AXUIElementCreateApplication(pid) };
    if ax_app.is_null() {
        unsafe { CFRelease(system_wide) };
        return Err("无法自动粘贴：目标应用不在运行，请重新打开 iPaste 面板后再粘贴。".to_string());
    }

    // 轮询等待系统键盘焦点（AXFocusedApplication）转移到目标应用；
    // 未就绪时通过设置 AXFocusedApplication 强制转移
    let deadline = Instant::now() + AX_FOCUS_WAIT_TIMEOUT;
    loop {
        if system_focused_pid(system_wide, &focused_application_attr) == Some(pid) {
            break;
        }
        if Instant::now() >= deadline {
            unsafe {
                CFRelease(ax_app);
                CFRelease(system_wide);
            }
            return Err(
                "无法自动粘贴：目标应用窗口未能获得键盘焦点，请确认目标窗口可见后重试。"
                    .to_string(),
            );
        }
        unsafe {
            AXUIElementSetAttributeValue(
                system_wide,
                Retained::as_ptr(&focused_application_attr) as CFStringRef,
                ax_app,
            )
        };
        thread::sleep(AX_FOCUS_POLL_INTERVAL);
    }

    unsafe {
        CFRelease(ax_app);
        CFRelease(system_wide);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn system_focused_pid(system_wide: CFTypeRef, focused_application_attr: &NSString) -> Option<c_int> {
    let mut focused_app: CFTypeRef = std::ptr::null();
    let status = unsafe {
        AXUIElementCopyAttributeValue(
            system_wide,
            focused_application_attr as *const NSString as CFStringRef,
            &mut focused_app,
        )
    };
    if status != 0 || focused_app.is_null() {
        return None;
    }

    let mut focused_pid: c_int = 0;
    let pid_status = unsafe { AXUIElementGetPid(focused_app, &mut focused_pid) };
    unsafe { CFRelease(focused_app) };
    (pid_status == 0).then_some(focused_pid)
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

