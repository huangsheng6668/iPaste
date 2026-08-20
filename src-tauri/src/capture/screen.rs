//! 屏幕截取：xcap 显示器匹配 + 区域裁剪 + PNG 编码；macOS 屏幕录制权限预检。

use image::RgbaImage;
use xcap::Monitor;

use crate::capture::selection::PhysicalRect;

#[inline]
fn is_position_match(actual_x: i32, actual_y: i32, target_x: i32, target_y: i32) -> bool {
    (actual_x - target_x).abs() <= 2 && (actual_y - target_y).abs() <= 2
}

/// 一次性获取所有 xcap 显示器并与传入的 Tauri 显示器列表进行坐标匹配与截屏。
pub(crate) fn capture_all_monitor_frames(
    monitors: &[tauri::Monitor],
) -> Result<Vec<RgbaImage>, String> {
    let xcap_monitors = Monitor::all().map_err(|error| format!("枚举显示器失败：{error}"))?;
    let mut frames = Vec::with_capacity(monitors.len());

    for target in monitors {
        let position = target.position();
        let matched = xcap_monitors
            .iter()
            .find(|m| {
                let (Ok(x), Ok(y)) = (m.x(), m.y()) else {
                    return false;
                };
                is_position_match(x, y, position.x, position.y)
            })
            .ok_or_else(|| format!("未找到匹配的显示器: ({}, {})", position.x, position.y))?;

        let frame = matched
            .capture_image()
            .map_err(|error| format!("截屏失败：{error}"))?;
        frames.push(frame);
    }
    Ok(frames)
}

pub(crate) fn clamp_rect_to_image(mut rect: PhysicalRect, image: &RgbaImage) -> PhysicalRect {
    rect.width = rect.width.min(image.width().saturating_sub(rect.x));
    rect.height = rect.height.min(image.height().saturating_sub(rect.y));
    rect
}

/// 触发侧整屏冻结帧捕获：在任何遮罩窗存在之前调用，硬件加速视频此时仍正常合成。
pub(crate) fn capture_monitor_frame(monitor: &tauri::Monitor) -> Result<RgbaImage, String> {
    let mut frames = capture_all_monitor_frames(std::slice::from_ref(monitor))?;
    frames.pop().ok_or_else(|| "截屏结果为空".to_string())
}

pub(crate) fn png_bytes(image: RgbaImage) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut buffer), image::ImageFormat::Png)
        .map_err(|error| format!("PNG 编码失败：{error}"))?;
    Ok(buffer)
}

#[cfg(target_os = "macos")]
pub(crate) fn has_screen_capture_permission() -> bool {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGRequestScreenCaptureAccess() -> bool;
    }
    unsafe {
        if CGPreflightScreenCaptureAccess() {
            return true;
        }
        // 未决定时触发系统授权弹框；已拒绝时返回 false，由错误事件引导去设置页
        CGRequestScreenCaptureAccess()
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn has_screen_capture_permission() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn clamp_rect_to_image_bounds() {
        let image = RgbaImage::new(100, 100);
        let rect = clamp_rect_to_image(
            PhysicalRect { x: 90, y: 90, width: 50, height: 50 },
            &image,
        );
        assert_eq!(
            rect,
            PhysicalRect { x: 90, y: 90, width: 10, height: 10 }
        );
    }

    #[test]
    fn clamp_rect_to_image_completely_outside() {
        let image = RgbaImage::new(100, 100);
        let rect = clamp_rect_to_image(
            PhysicalRect { x: 120, y: 120, width: 50, height: 50 },
            &image,
        );
        assert_eq!(
            rect,
            PhysicalRect { x: 120, y: 120, width: 0, height: 0 }
        );
    }

    #[test]
    fn crop_and_png_round_trip() {
        let mut image = RgbaImage::new(10, 10);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            *pixel = Rgba([x as u8, y as u8, 7, 255]);
        }
        let rect = clamp_rect_to_image(
            PhysicalRect { x: 2, y: 3, width: 4, height: 5 },
            &image,
        );
        let cropped = image::imageops::crop_imm(&image, rect.x, rect.y, rect.width, rect.height).to_image();
        let bytes = png_bytes(cropped).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (4, 5));
        assert_eq!(decoded.get_pixel(0, 0), &Rgba([2, 3, 7, 255]));
        assert_eq!(decoded.get_pixel(3, 4), &Rgba([5, 7, 7, 255]));
    }

    #[test]
    fn position_matching_tolerance() {
        assert!(is_position_match(100, 200, 100, 200));
        assert!(is_position_match(102, 198, 100, 200));
        assert!(is_position_match(98, 202, 100, 200));
        assert!(!is_position_match(103, 200, 100, 200));
        assert!(!is_position_match(100, 203, 100, 200));
        assert!(!is_position_match(97, 200, 100, 200));
        assert!(!is_position_match(100, 197, 100, 200));
    }
}
