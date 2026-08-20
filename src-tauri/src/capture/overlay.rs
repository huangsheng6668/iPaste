//! 截图遮罩窗：每显示器一个置顶透明覆盖窗，供前端框选。

use tauri::{utils::config::Color, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::window::{point_in_monitor, OCR_OVERLAY_WINDOW_PREFIX};

pub(crate) fn create_overlay_windows(app: &tauri::AppHandle) -> Result<Vec<String>, String> {
    let monitors = app.available_monitors().map_err(|error| error.to_string())?;
    if monitors.is_empty() {
        return Err("未找到可用屏幕".to_string());
    }

    let mut labels = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
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
        // builder.position 是逻辑坐标，混合 DPI 下不可靠：建窗后按物理坐标钉住
        window
            .set_position(*monitor.position())
            .map_err(|error| error.to_string())?;
        window
            .set_size(tauri::PhysicalSize::new(
                monitor.size().width,
                monitor.size().height,
            ))
            .map_err(|error| error.to_string())?;
        labels.push(label);
    }

    for label in &labels {
        if let Some(window) = app.get_webview_window(label) {
            window.show().map_err(|error| error.to_string())?;
            window
                .set_always_on_top(true)
                .map_err(|error| error.to_string())?;
        }
    }

    // 焦点给光标所在显示器的遮罩，Esc 立即生效
    if let Ok(cursor) = app.cursor_position() {
        let focus_label = focus_label_for(
            app,
            cursor.x.round() as i32,
            cursor.y.round() as i32,
            &labels,
        );
        if let Some(window) = app.get_webview_window(&focus_label) {
            let _ = window.set_focus();
        }
    }
    Ok(labels)
}

fn focus_label_for(app: &tauri::AppHandle, x: i32, y: i32, labels: &[String]) -> String {
    let Ok(monitors) = app.available_monitors() else {
        return labels[0].clone();
    };
    for (index, monitor) in monitors.iter().enumerate() {
        if point_in_monitor(monitor, x, y) {
            if let Some(label) = labels.get(index) {
                return label.clone();
            }
        }
    }
    labels[0].clone()
}
