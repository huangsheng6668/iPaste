//! 窗口定位数学与显示器包装：底部纯值函数（fit/point/distance，可离线单测）
//! 承载几何语义；上方的 `tauri::Monitor` 包装取 work_area/position 后委托纯值层
//! （Monitor 无法在测试中构造，故拆出可测的纯数学层）。

#[cfg(target_os = "windows")]
use tauri::PhysicalSize;
use tauri::{Manager, PhysicalPosition};

use crate::models::WindowGeometry;
use crate::util::clamp;
use crate::window::MAIN_WINDOW;

const PANEL_GAP: i32 = 12;
const SCREEN_MARGIN: i32 = 12;
pub(crate) const MAIN_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 560.0,
    height: 620.0,
    min_width: 560.0,
    min_height: 500.0,
    max_width: Some(720.0),
    max_height: None,
};
pub(crate) const SIDE_MAIN_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 720.0,
    height: 620.0,
    min_width: 700.0,
    min_height: 500.0,
    max_width: Some(720.0),
    max_height: None,
};
pub(crate) const SETTINGS_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 760.0,
    height: 520.0,
    min_width: 680.0,
    min_height: 460.0,
    max_width: None,
    max_height: None,
};
pub(crate) const CLIP_VIEWER_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 840.0,
    height: 620.0,
    min_width: 640.0,
    min_height: 460.0,
    max_width: None,
    max_height: None,
};
pub(crate) const LAN_SYNC_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 420.0,
    height: 560.0,
    min_width: 360.0,
    min_height: 480.0,
    max_width: None,
    max_height: None,
};
pub(crate) const OCR_RESULT_WINDOW_GEOMETRY: WindowGeometry = WindowGeometry {
    width: 520.0,
    height: 420.0,
    min_width: 360.0,
    min_height: 260.0,
    max_width: None,
    max_height: None,
};

pub(crate) fn position_window_near_cursor(
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

pub(crate) fn position_window_centered_on_monitor(
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

pub(crate) fn position_clip_viewer_window(
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

pub(crate) fn apply_window_geometry_for_monitor(
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
    fit_size_to_work_area(
        work_area.size.width as i32,
        work_area.size.height as i32,
        size,
        SCREEN_MARGIN,
    )
}

pub(crate) fn monitor_for_point(app: &tauri::AppHandle, x: i32, y: i32) -> Result<tauri::Monitor, String> {
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

pub(crate) fn point_in_monitor(monitor: &tauri::Monitor, x: i32, y: i32) -> bool {
    let position = monitor.position();
    let size = monitor.size();
    let left = position.x;
    let top = position.y;
    let right = left + size.width as i32;
    let bottom = top + size.height as i32;

    point_in_rect(left, top, right, bottom, x, y)
}

fn monitor_distance_squared(monitor: &tauri::Monitor, x: i32, y: i32) -> i64 {
    let position = monitor.position();
    let size = monitor.size();
    let left = position.x as i64;
    let top = position.y as i64;
    let right = left + size.width as i64;
    let bottom = top + size.height as i64;

    distance_squared_to_rect(left, top, right, bottom, x as i64, y as i64)
}

/// 把窗口尺寸压进「工作区减两侧 margin」内；工作区过小时退化为 1px，不返回 0。
pub(crate) fn fit_size_to_work_area(
    work_area_width: i32,
    work_area_height: i32,
    size: (i32, i32),
    margin: i32,
) -> (i32, i32) {
    let max_width = (work_area_width - margin * 2).max(1);
    let max_height = (work_area_height - margin * 2).max(1);
    (size.0.min(max_width), size.1.min(max_height))
}

/// 半开区间命中判定（右/下边界不含），与显示器遍历语义一致。
pub(crate) fn point_in_rect(left: i32, top: i32, right: i32, bottom: i32, x: i32, y: i32) -> bool {
    x >= left && x < right && y >= top && y < bottom
}

/// 点到矩形的最短距离平方（矩形内为 0）；i64 防多屏坐标平方溢出。
pub(crate) fn distance_squared_to_rect(
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    x: i64,
    y: i64,
) -> i64 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_size_clamps_to_work_area_minus_margins() {
        assert_eq!(fit_size_to_work_area(1920, 1080, (800, 600), 8), (800, 600));
        assert_eq!(fit_size_to_work_area(1920, 1080, (4000, 2000), 8), (1904, 1064));
        assert_eq!(fit_size_to_work_area(10, 10, (800, 600), 8), (1, 1));
    }

    #[test]
    fn point_in_rect_half_open() {
        assert!(point_in_rect(0, 0, 100, 100, 0, 0));
        assert!(point_in_rect(0, 0, 100, 100, 99, 99));
        assert!(!point_in_rect(0, 0, 100, 100, 100, 50));
        assert!(!point_in_rect(0, 0, 100, 100, -1, 50));
    }

    #[test]
    fn distance_squared_zero_inside_and_grows_outside() {
        assert_eq!(distance_squared_to_rect(0, 0, 100, 100, 50, 50), 0);
        assert_eq!(distance_squared_to_rect(0, 0, 100, 100, 130, 0), 900);
        assert_eq!(distance_squared_to_rect(0, 0, 100, 100, 0, -20), 400);
        assert_eq!(distance_squared_to_rect(0, 0, 100, 100, 130, -20), 900 + 400);
    }
}
