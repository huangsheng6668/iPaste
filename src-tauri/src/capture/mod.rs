//! 截图 OCR：选区几何（selection）、屏幕截取（screen）、遮罩窗（overlay）、会话编排（本文件）。

pub(crate) mod overlay;
pub(crate) mod screen;
pub(crate) mod selection;

use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use tauri::{Emitter, Manager};

use crate::error::AppError;
use crate::events::{
    ClipboardCaptured, EVENT_CLIPBOARD_CAPTURED, EVENT_OCR_SCREENSHOT_ERROR, OcrScreenshotError,
};
use crate::models::{
    AppState, CapturedClipboardItem, MainWindowActivation, OcrResultPayload, ScreenshotSelection,
};
use crate::util::{hash_bytes, new_id};
use crate::window::{
    hide_main_window, show_main_window, show_ocr_result_window, show_settings_window_with_tab,
    MAIN_WINDOW,
};

/// 遮罩隐藏后等待合成器刷新，避免把遮罩截进图片。
const OVERLAY_HIDE_SETTLE_MS: u64 = 150;

pub(crate) struct CaptureSession {
    pub(crate) overlay_labels: Vec<String>,
    pub(crate) main_was_visible: bool,
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

/// 结束会话：销毁遮罩。restore_main 为 true 时（取消/失败路径）恢复主面板触发前可见性；
/// 成功路径传 false——结果窗接管焦点，主面板保持隐藏（规格约定）。
fn end_session(
    app: &tauri::AppHandle,
    session: &Arc<Mutex<Option<CaptureSession>>>,
    restore_main: bool,
) {
    let Some(session) = session.lock().ok().and_then(|mut guard| guard.take()) else {
        return;
    };
    for label in &session.overlay_labels {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.destroy();
        }
    }
    if restore_main && session.main_was_visible {
        let _ = show_main_window(app, MainWindowActivation::Activate);
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
        // 先占位再建窗：建窗/隐窗可能等待主线程，不能持锁阻塞
        *session = Some(CaptureSession {
            overlay_labels: Vec::new(),
            main_was_visible,
        });
    }

    if main_was_visible {
        // 失败也必须收尾会话：占位已写入，直接 ? 返回会卡死后续所有触发。
        if let Err(error) = hide_main_window(app) {
            end_session(app, &state.capture_session, true);
            return Err(error);
        }
    }

    let labels = match overlay::create_overlay_windows(app) {
        Ok(labels) => labels,
        Err(error) => {
            end_session(app, &state.capture_session, true);
            return Err(error);
        }
    };

    *state
        .capture_session
        .lock()
        .map_err(|error| error.to_string())? = Some(CaptureSession {
        overlay_labels: labels,
        main_was_visible,
    });
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

    // 1) 截屏前先隐藏全部遮罩（保留窗口，销毁留给会话收尾统一做）
    {
        let labels: Vec<String> = capture_session
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .as_ref()
                    .map(|session| session.overlay_labels.clone())
            })
            .unwrap_or_default();
        for label in &labels {
            if let Some(window) = app.get_webview_window(label) {
                let _ = window.hide();
            }
        }
    }

    // 2) 物理 px 收敛；过小视为取消
    let rect = selection::to_physical_rect(
        &selection,
        monitor.scale_factor(),
        monitor.size().width,
        monitor.size().height,
    );
    if selection::is_too_small(rect) {
        end_session(&app, &capture_session, true);
        return Ok(());
    }

    // 3) 截屏 + 裁剪 + PNG（阻塞工作放 spawn_blocking）
    let capture_monitor = monitor.clone();
    let captured = tokio::task::spawn_blocking(move || {
        thread::sleep(Duration::from_millis(OVERLAY_HIDE_SETTLE_MS));
        screen::capture_monitor_region(&capture_monitor, rect)
    })
    .await
    .map_err(|error| AppError::internal(error.to_string()))
    .and_then(|result| result.map_err(AppError::from));
    let png = match captured {
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

    // 6) 结束会话（成功路径 restore_main = false，主面板保持隐藏）——在返回前销毁遮罩，
    //    invoke 响应可能随遮罩 webview 销毁而丢失，属预期（后续步骤全由 Rust 完成）
    end_session(&app, &capture_session, false);
    show_ocr_result_window(&app, &token, selection.monitor_index).map_err(AppError::from)?;
    Ok(())
}
