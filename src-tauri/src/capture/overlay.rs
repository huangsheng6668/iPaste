//! 截图遮罩窗：多显示器透明覆盖窗口池管理与生命周期（预热/同步/展示/隐藏）。

use tauri::{utils::config::Color, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::window::{point_in_monitor, OCR_OVERLAY_WINDOW_PREFIX};

/// 构造单个显示器的遮罩覆盖窗口（无边框、置顶、透明、初始化隐藏）。
fn build_overlay_window(
    app: &tauri::AppHandle,
    index: usize,
    monitor: &tauri::Monitor,
) -> Result<tauri::WebviewWindow, String> {
    let label = format!("{OCR_OVERLAY_WINDOW_PREFIX}{index}");
    let url = format!("index.html?window=ocr-overlay&monitor={index}");
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App(url.into()))
        .title("iPaste Screenshot OCR")
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
        .map_err(|error| error.to_string())?;

    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
    let _ = window.set_position(*monitor.position());
    let _ = window.set_size(tauri::PhysicalSize::new(
        monitor.size().width,
        monitor.size().height,
    ));
    Ok(window)
}

/// 在后台预热创建遮罩窗口池（遍历当前所有可用显示器，静默建窗并保持隐藏）。
pub(crate) fn prewarm_overlay_windows(app: &tauri::AppHandle) -> Result<(), String> {
    let monitors = app.available_monitors().map_err(|error| error.to_string())?;
    for (index, monitor) in monitors.iter().enumerate() {
        let label = format!("{OCR_OVERLAY_WINDOW_PREFIX}{index}");
        if app.get_webview_window(&label).is_none() {
            let _ = build_overlay_window(app, index, monitor)?;
        }
    }
    Ok(())
}

/// 同步遮罩窗口池与当前显示器拓扑：
/// 1. 销毁超出当前显示器数量的多余窗口（如外接屏拔出）；
/// 2. 补建缺失的遮罩窗口（如外接屏插入）；
/// 3. 更新所有有效窗口的物理位置与尺寸（适配分辨率/布局变化）；
/// 4. 返回当前所有有效遮罩窗口的 label 列表。
pub(crate) fn sync_overlay_windows(
    app: &tauri::AppHandle,
    monitors: &[tauri::Monitor],
) -> Result<Vec<String>, String> {
    if monitors.is_empty() {
        return Err("未找到可用屏幕".to_string());
    }

    // 销毁超出当前显示器数量的多余窗口（例如拔掉外接屏）
    for (label, window) in app.webview_windows() {
        if let Some(suffix) = label.strip_prefix(OCR_OVERLAY_WINDOW_PREFIX) {
            if let Ok(index) = suffix.parse::<usize>() {
                if index >= monitors.len() {
                    let _ = window.destroy();
                }
            }
        }
    }

    let mut labels = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
        let label = format!("{OCR_OVERLAY_WINDOW_PREFIX}{index}");
        let window = match app.get_webview_window(&label) {
            Some(window) => window,
            None => build_overlay_window(app, index, monitor)?,
        };
        let _ = window.set_position(*monitor.position());
        let _ = window.set_size(tauri::PhysicalSize::new(
            monitor.size().width,
            monitor.size().height,
        ));
        labels.push(label);
    }

    Ok(labels)
}

/// 显示所有指定的遮罩窗口并置顶，将焦点赋予鼠标指针所在的屏幕窗口。
pub(crate) fn show_overlay_windows(
    app: &tauri::AppHandle,
    labels: &[String],
) -> Result<(), String> {
    for label in labels {
        if let Some(window) = app.get_webview_window(label) {
            window.show().map_err(|error| error.to_string())?;
            window.set_always_on_top(true).map_err(|error| error.to_string())?;
        }
    }

    if let Ok(cursor) = app.cursor_position() {
        let focus_label = focus_label_for(
            app,
            cursor.x.round() as i32,
            cursor.y.round() as i32,
            labels,
        );
        if let Some(window) = app.get_webview_window(&focus_label) {
            let _ = window.set_focus();
        }
    }
    Ok(())
}

/// 隐藏所有指定的遮罩窗口（保持实例常驻，不销毁）。
pub(crate) fn hide_overlay_windows(app: &tauri::AppHandle, labels: &[String]) {
    for label in labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.hide();
        }
    }
}

/// 兼容旧入口（待编排层切换后可移除）。
#[allow(dead_code)]
pub(crate) fn create_overlay_windows(
    app: &tauri::AppHandle,
    _frame_paths: &[String],
) -> Result<Vec<String>, String> {
    let monitors = app.available_monitors().map_err(|error| error.to_string())?;
    let labels = sync_overlay_windows(app, &monitors)?;
    show_overlay_windows(app, &labels)?;
    Ok(labels)
}

/// 计算鼠标坐标落在哪台显示器上，返回对应遮罩窗口的 label；若未命中则回退到首个 label。
fn focus_label_for(app: &tauri::AppHandle, x: i32, y: i32, labels: &[String]) -> String {
    let Ok(monitors) = app.available_monitors() else {
        return labels.first().cloned().unwrap_or_default();
    };
    for (index, monitor) in monitors.iter().enumerate() {
        if point_in_monitor(monitor, x, y) {
            if let Some(label) = labels.get(index) {
                return label.clone();
            }
        }
    }
    labels.first().cloned().unwrap_or_default()
}
