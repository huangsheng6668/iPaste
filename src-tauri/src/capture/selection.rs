//! 截图选区几何：逻辑像素 → 物理像素换算与边界收敛（纯函数，单测覆盖）。

use crate::models::ScreenshotSelection;

pub(crate) const MIN_SELECTION_PHYSICAL: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhysicalRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// 方向无关归一化：任意拖拽方向转为正宽高的左上角坐标。
pub(crate) fn normalize_selection(
    left: f64,
    top: f64,
    width: f64,
    height: f64,
) -> (f64, f64, f64, f64) {
    let x = if width < 0.0 { left + width } else { left };
    let y = if height < 0.0 { top + height } else { top };
    (x, y, width.abs(), height.abs())
}

/// CSS 逻辑像素选区 × 显示器缩放 → 显示器内物理像素矩形，clamp 到显示器边界。
/// 原点向下取整、尺寸向上取整：宁可多截 1px，不丢边界文字。
pub(crate) fn to_physical_rect(
    selection: &ScreenshotSelection,
    scale_factor: f64,
    monitor_width: u32,
    monitor_height: u32,
) -> PhysicalRect {
    let scale = if scale_factor.is_finite() && scale_factor > 0.1 { scale_factor } else { 1.0 };
    let (x, y, w, h) = normalize_selection(
        selection.left,
        selection.top,
        selection.width,
        selection.height,
    );
    let x = ((x * scale).floor().max(0.0) as u32).min(monitor_width);
    let y = ((y * scale).floor().max(0.0) as u32).min(monitor_height);
    let width = ((w * scale).ceil().max(0.0) as u32).min(monitor_width.saturating_sub(x));
    let height = ((h * scale).ceil().max(0.0) as u32).min(monitor_height.saturating_sub(y));
    PhysicalRect { x, y, width, height }
}

pub(crate) fn is_too_small(rect: PhysicalRect) -> bool {
    rect.width < MIN_SELECTION_PHYSICAL || rect.height < MIN_SELECTION_PHYSICAL
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selection(left: f64, top: f64, width: f64, height: f64) -> ScreenshotSelection {
        ScreenshotSelection { monitor_index: 0, left, top, width, height }
    }

    #[test]
    fn normalize_selection_supports_all_drag_directions() {
        assert_eq!(
            normalize_selection(10.0, 20.0, 30.0, 40.0),
            (10.0, 20.0, 30.0, 40.0)
        );
        assert_eq!(
            normalize_selection(40.0, 60.0, -30.0, -40.0),
            (10.0, 20.0, 30.0, 40.0)
        );
        assert_eq!(
            normalize_selection(40.0, 20.0, -30.0, 40.0),
            (10.0, 20.0, 30.0, 40.0)
        );
    }

    #[test]
    fn to_physical_rect_scales_and_clamps_to_monitor() {
        let rect = to_physical_rect(&selection(0.0, 0.0, 100.0, 50.0), 1.5, 200, 200);
        assert_eq!(rect, PhysicalRect { x: 0, y: 0, width: 150, height: 75 });

        // 越界选区收敛到显示器边界内
        let rect = to_physical_rect(&selection(90.0, 90.0, 100.0, 100.0), 1.0, 100, 100);
        assert_eq!(rect, PhysicalRect { x: 90, y: 90, width: 10, height: 10 });
    }

    #[test]
    fn to_physical_rect_floors_origin_and_ceils_size() {
        let rect = to_physical_rect(&selection(10.4, 10.6, 5.1, 5.1), 1.0, 100, 100);
        assert_eq!(rect, PhysicalRect { x: 10, y: 10, width: 6, height: 6 });
    }

    #[test]
    fn too_small_detection_uses_physical_pixels() {
        assert!(is_too_small(PhysicalRect { x: 0, y: 0, width: 7, height: 100 }));
        assert!(is_too_small(PhysicalRect { x: 0, y: 0, width: 100, height: 7 }));
        assert!(!is_too_small(PhysicalRect { x: 0, y: 0, width: 8, height: 8 }));
    }
}
