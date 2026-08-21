//! 截图 OCR：选区几何（selection）、屏幕截取（screen）、遮罩窗（overlay）、会话编排（本文件）。

pub(crate) mod overlay;
pub(crate) mod screen;
pub(crate) mod selection;

use std::{
    io::Write,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use tauri::{Emitter, Manager};

use crate::error::AppError;
use crate::events::{
    ClipboardCaptured, EVENT_CLIPBOARD_CAPTURED, EVENT_OCR_OVERLAY_SESSION_START,
    EVENT_OCR_SCREENSHOT_ERROR, OcrOverlaySessionStart, OcrScreenshotError,
};
use crate::models::{
    AppState, CapturedClipboardItem, MainWindowActivation, OcrResultPayload, ScreenshotSelection,
};
use crate::util::{hash_bytes, new_id};
use crate::window::{
    hide_main_window, show_main_window, show_ocr_result_window, show_settings_window_with_tab,
    MAIN_WINDOW,
};

/// 主面板隐藏后等待合成器刷新：冻结帧捕获前面板不能入画。
const PANEL_HIDE_SETTLE_MS: u64 = 120;
/// 冻结帧显示文件目录（$APPDATA 下）；会话结束整目录删除。
const OVERLAY_FRAME_DIR: &str = "ocr-overlay";

pub(crate) struct CaptureSession {
    pub(crate) overlay_labels: Vec<String>,
    pub(crate) main_was_visible: bool,
    /// 每显示器整屏冻结帧（触发时捕获）；提交时按索引取出裁剪。
    pub(crate) frozen_frames: Vec<Option<image::RgbaImage>>,
}

/// 冻结帧显示目录：先清空（吞掉崩溃残留）再重建。
fn overlay_frame_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(OVERLAY_FRAME_DIR);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir)
}

/// 冻结帧写盘：BMP 无压缩（像素直拷）。此前用 JPEG 时纯软编码整屏帧单帧可达数秒，
/// 是截图遮罩出现的卡顿主因；BMP 仅作遮罩背景显示，OCR 裁剪仍取内存无损帧。
/// encoder 逐像素小写入，必须套大缓冲聚合系统调用，否则 4K 帧写入仍需数秒。
fn write_frame_bmp(path: &std::path::Path, frame: &image::RgbaImage) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    let mut writer = std::io::BufWriter::with_capacity(1024 * 1024, file);
    let mut encoder = image::codecs::bmp::BmpEncoder::new(&mut writer);
    encoder
        .encode(
            frame.as_raw(),
            frame.width(),
            frame.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| format!("冻结帧编码失败：{error}"))?;
    writer
        .flush()
        .map_err(|error| format!("冻结帧写盘失败：{error}"))
}

fn frame_capture_failed(app: &tauri::AppHandle, session: &Arc<Mutex<Option<CaptureSession>>>) {
    let _ = app.emit(
        EVENT_OCR_SCREENSHOT_ERROR,
        OcrScreenshotError {
            code: "screenCaptureFailed".to_string(),
        },
    );
    let main_was_visible = session
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().map(|session| session.main_was_visible))
        .unwrap_or(false);
    end_session(app, session, true);
    if !main_was_visible {
        // 面板隐藏时（快捷键/托盘触发）toast 无处渲染：强制唤出主面板承载失败反馈
        let _ = show_main_window(app, MainWindowActivation::Activate);
    }
}

fn preflight(app: &tauri::AppHandle, state: &AppState) -> Result<(), &'static str> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, state);
        if !screen::has_screen_capture_permission() {
            return Err("screenRecordingPermission");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        match crate::ocr::install_status(app, &state.store) {
            Ok(status) if status.platform == "unsupported" => return Err("ocrUnsupported"),
            Ok(status) if !status.installed => return Err("ocrModelMissing"),
            Ok(_) => {}
            Err(_) => return Err("ocrModelMissing"),
        }
    }

    Ok(())
}

fn preflight_failed(app: &tauri::AppHandle, code: &str) {
    let _ = app.emit(
        EVENT_OCR_SCREENSHOT_ERROR,
        OcrScreenshotError { code: code.to_string() },
    );
    let tab = match code {
        "screenRecordingPermission" => Some("permissions"),
        "ocrModelMissing" => Some("ocr"),
        _ => None,
    };
    if let Some(tab) = tab {
        let _ = show_settings_window_with_tab(app, Some(tab));
    }
}

/// 结束会话：隐藏遮罩窗口（保持实例常驻）。restore_main 为 true 时（取消/失败路径）恢复主面板触发前可见性；
/// 成功路径传 false——结果窗接管焦点，主面板保持隐藏（规格约定）。
fn end_session(
    app: &tauri::AppHandle,
    session: &Arc<Mutex<Option<CaptureSession>>>,
    restore_main: bool,
) {
    let Some(session) = session.lock().ok().and_then(|mut guard| guard.take()) else {
        return;
    };
    overlay::hide_overlay_windows(app, &session.overlay_labels);
    if restore_main && session.main_was_visible {
        let _ = show_main_window(app, MainWindowActivation::Activate);
    }
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::remove_dir_all(dir.join(OVERLAY_FRAME_DIR));
    }
}

pub(crate) fn start_screenshot_ocr(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };

    if let Err(code) = preflight(app, &state) {
        preflight_failed(app, code);
        return Ok(());
    }

    let main_was_visible = app
        .get_webview_window(MAIN_WINDOW)
        .map(|window| window.is_visible().unwrap_or(false))
        .unwrap_or(false);

    {
        let mut session = state
            .capture_session
            .lock()
            .map_err(|error| error.to_string())?;
        if session.is_some() {
            return Ok(()); // 已在截图会话中，忽略重复触发
        }
        // 先占位再建窗/同步：避免持锁阻塞
        *session = Some(CaptureSession {
            overlay_labels: Vec::new(),
            main_was_visible,
            frozen_frames: Vec::new(),
        });
    }

    if main_was_visible {
        // 失败也必须收尾会话：占位已写入，直接 ? 返回会卡死后续所有触发。
        if let Err(error) = hide_main_window(app) {
            end_session(app, &state.capture_session, true);
            return Err(error);
        }
        // 冻结帧捕获前等合成器把主面板从画面里刷掉
        thread::sleep(Duration::from_millis(PANEL_HIDE_SETTLE_MS));
    }

    let monitors = match app.available_monitors() {
        Ok(monitors) => monitors,
        Err(error) => {
            end_session(app, &state.capture_session, true);
            return Err(error.to_string());
        }
    };
    if monitors.is_empty() {
        end_session(app, &state.capture_session, true);
        return Err("未找到可用屏幕".to_string());
    }

    let labels = match overlay::sync_overlay_windows(app, &monitors) {
        Ok(labels) => labels,
        Err(error) => {
            end_session(app, &state.capture_session, true);
            return Err(error);
        }
    };

    let frames = match screen::capture_all_monitor_frames(&monitors) {
        Ok(frames) => frames,
        Err(error) => {
            frame_capture_failed(app, &state.capture_session);
            eprintln!("frozen frame capture failed: {error}");
            return Ok(());
        }
    };

    let frame_dir = match overlay_frame_dir(app) {
        Ok(dir) => dir,
        Err(error) => {
            end_session(app, &state.capture_session, true);
            return Err(error);
        }
    };

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut frozen_frames = Vec::with_capacity(frames.len());
    for (index, frame) in frames.into_iter().enumerate() {
        let path = frame_dir.join(format!("frozen-{index}.bmp"));
        if let Err(error) = write_frame_bmp(&path, &frame) {
            frame_capture_failed(app, &state.capture_session);
            eprintln!("frozen frame encode failed: {error}");
            return Ok(());
        }
        let _ = app.emit(
            EVENT_OCR_OVERLAY_SESSION_START,
            OcrOverlaySessionStart {
                monitor_index: index,
                frame_path: path.to_string_lossy().to_string(),
                timestamp,
            },
        );
        frozen_frames.push(Some(frame));
    }

    *state
        .capture_session
        .lock()
        .map_err(|error| error.to_string())? = Some(CaptureSession {
        overlay_labels: labels.clone(),
        main_was_visible,
        frozen_frames,
    });

    if let Err(error) = overlay::show_overlay_windows(app, &labels) {
        end_session(app, &state.capture_session, true);
        return Err(error);
    }

    Ok(())
}

pub(crate) fn cancel_screenshot_ocr(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(state) = app.try_state::<AppState>() else {
        return Ok(());
    };
    end_session(app, &state.capture_session, true);
    Ok(())
}

pub(crate) async fn submit_screenshot_selection(
    app: tauri::AppHandle,
    selection: ScreenshotSelection,
) -> Result<(), AppError> {
    let Some(state) = app.try_state::<AppState>() else {
        return Err(AppError::internal("应用状态不可用"));
    };
    // 提前克隆 Arc 句柄：避免跨 await 持有 State 借用
    let (capture_session, payloads, store) = (
        state.capture_session.clone(),
        state.ocr_result_payloads.clone(),
        state.store.clone(),
    );

    // 提前隐藏遮罩窗口以提供即时响应
    if let Ok(guard) = capture_session.lock() {
        if let Some(session) = guard.as_ref() {
            overlay::hide_overlay_windows(&app, &session.overlay_labels);
        }
    }

    let monitor = match app
        .available_monitors()
        .map_err(|error| AppError::internal(error.to_string()))
        .and_then(|monitors| {
            monitors
                .get(selection.monitor_index)
                .cloned()
                .ok_or_else(|| AppError::internal("无效的显示器索引"))
        }) {
        Ok(monitor) => monitor,
        Err(error) => {
            end_session(&app, &capture_session, true);
            return Err(error);
        }
    };

    // 1) 从会话取出该显示器的冻结帧（触发时捕获的整屏画面）
    let frame = capture_session
        .lock()
        .ok()
        .and_then(|mut guard| {
            guard.as_mut().and_then(|session| {
                session
                    .frozen_frames
                    .get_mut(selection.monitor_index)
                    .and_then(|frame| frame.take())
            })
        });
    let frame = match frame {
        Some(frame) => frame,
        None => {
            end_session(&app, &capture_session, true);
            return Err(AppError::internal("冻结帧不可用"));
        }
    };

    // 2) 物理 px 收敛（以冻结帧自身尺寸为准）；过小视为取消
    let rect = selection::to_physical_rect(
        &selection,
        monitor.scale_factor(),
        frame.width(),
        frame.height(),
    );
    if selection::is_too_small(rect) {
        end_session(&app, &capture_session, true);
        return Ok(());
    }

    // 3) 裁剪冻结帧 + PNG（无二次截屏、无等待；失败仍需收尾会话）
    let png = match tokio::task::spawn_blocking(move || {
        let rect = screen::clamp_rect_to_image(rect, &frame);
        let cropped = image::imageops::crop_imm(&frame, rect.x, rect.y, rect.width, rect.height)
            .to_image();
        screen::png_bytes(cropped)
    })
    .await
    .map_err(|error| AppError::internal(error.to_string()))
    .and_then(|result| result.map_err(AppError::from))
    {
        Ok(png) => png,
        Err(error) => {
            end_session(&app, &capture_session, true);
            return Err(error);
        }
    };

    // 4) 入库（复用剪贴板图片条目链路：PNG 落 clip-images，text = 文件路径）
    let item = CapturedClipboardItem {
        clip_type: "image".to_string(),
        content_hash: hash_bytes(&png),
        preview_text: format!("{} x {}", rect.width, rect.height),
        text: String::new(),
        image_bytes: Some(png),
        display_name: None,
    };
    let inserted = store.insert_captured_item(item).map_err(|error| {
        end_session(&app, &capture_session, true);
        AppError::from(error)
    })?;
    let Some((clip, clip_total_count, was_inserted)) = inserted else {
        end_session(&app, &capture_session, true);
        return Ok(());
    };
    let _ = app.emit(
        EVENT_CLIPBOARD_CAPTURED,
        ClipboardCaptured {
            clip: clip.clone(),
            clip_total_count,
            was_inserted,
        },
    );

    // 5) 结果载荷（单活跃：清空后写入）+ 打开结果窗
    let token = new_id();
    if let Ok(mut map) = payloads.lock() {
        map.clear();
        map.insert(
            token.clone(),
            OcrResultPayload {
                image_path: clip.text.clone(),
                item_id: clip.id.clone(),
                monitor_index: selection.monitor_index,
            },
        );
    }

    // 6) 结束会话（成功路径 restore_main = false，主面板保持隐藏）
    end_session(&app, &capture_session, false);
    show_ocr_result_window(&app, &token, selection.monitor_index).map_err(AppError::from)?;
    Ok(())
}
