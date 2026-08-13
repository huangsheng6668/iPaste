use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use arboard::{Clipboard, Error as ClipboardError, ImageData};
use base64::{engine::general_purpose, Engine as _};
use enigo::{
    Direction::{Click, Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use image::{ImageBuffer, ImageEncoder, Rgba};
use tauri::Emitter;

use crate::models::*;
use crate::store::Store;
use crate::util::*;

#[cfg(target_os = "windows")]
use windows_sys::Win32::System::DataExchange::GetClipboardSequenceNumber;

#[cfg(target_os = "macos")]
use objc2_app_kit::NSPasteboard;

const IMAGE_FILE_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "gif", "ico"];

pub(crate) fn spawn_clipboard_watcher(
    app: tauri::AppHandle,
    store: Store,
    is_listening: Arc<Mutex<bool>>,
    append_copy_state: Arc<Mutex<AppendCopyState>>,
    last_clipboard_change_id: Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: Arc<Mutex<Option<String>>>,
) {
    thread::spawn(move || loop {
        let enabled = is_listening.lock().map(|value| *value).unwrap_or(false);
        if !enabled {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        let before_change_id = clipboard_change_id();

        match read_clipboard_item() {
            Ok(ClipboardRead::Item(item)) => {
                let after_change_id = clipboard_change_id();
                if before_change_id.is_some()
                    && after_change_id.is_some()
                    && before_change_id != after_change_id
                {
                    thread::sleep(Duration::from_millis(120));
                    continue;
                }

                let change_id = after_change_id.or(before_change_id);
                if !should_capture_clipboard_item(
                    change_id,
                    &item.content_hash,
                    &last_clipboard_change_id,
                    &last_clipboard_hash,
                ) {
                    thread::sleep(Duration::from_millis(700));
                    continue;
                }

                let capture_result = capture_append_copy_item(
                    &store,
                    &append_copy_state,
                    &last_clipboard_change_id,
                    &last_clipboard_hash,
                    &item,
                )
                .and_then(|append_copy_clip| match append_copy_clip {
                    Some(result) => Ok(Some(result)),
                    None => store.insert_captured_item(item),
                });

                match capture_result {
                    Ok(Some((clip, clip_total_count, was_inserted))) => {
                        let _ = app.emit(
                            "ipaste://clipboard-captured",
                            ClipboardCaptured {
                                clip,
                                clip_total_count,
                                was_inserted,
                            },
                        );
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = app.emit("ipaste://capture-error", error);
                    }
                }
            }
            Ok(ClipboardRead::Empty) => {}
            Ok(ClipboardRead::Occupied) => {}
            Err(error) => {
                let _ = app.emit("ipaste://capture-error", error);
            }
        }

        thread::sleep(Duration::from_millis(700));
    });
}

fn capture_append_copy_item(
    store: &Store,
    append_copy_state: &Arc<Mutex<AppendCopyState>>,
    last_clipboard_change_id: &Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: &Arc<Mutex<Option<String>>>,
    item: &CapturedClipboardItem,
) -> Result<Option<(ClipItem, usize, bool)>, String> {
    if item.clip_type == "image" || item.text.trim().is_empty() {
        return Ok(None);
    }

    let (clip_id, session_id, next_text) = {
        let append_copy = append_copy_state
            .lock()
            .map_err(|error| error.to_string())?;

        if !append_copy.is_enabled {
            return Ok(None);
        }

        let Some(session_id) = append_copy.session_id.clone() else {
            return Ok(None);
        };

        (
            append_copy.clip_id.clone(),
            session_id,
            append_copy_text(&append_copy.text, &item.text),
        )
    };

    let (clip, clip_total_count, was_inserted) =
        store.upsert_append_copy_item(clip_id, &session_id, next_text.clone())?;
    write_clipboard_text(&next_text)?;
    remember_current_clipboard_marker(
        last_clipboard_change_id,
        last_clipboard_hash,
        Some(hash_text(&next_text)),
    );

    if let Ok(mut append_copy) = append_copy_state.lock() {
        if append_copy.is_enabled && append_copy.session_id.as_deref() == Some(session_id.as_str())
        {
            append_copy.clip_id = Some(clip.id.clone());
            append_copy.text = next_text;
        }
    }

    Ok(Some((clip, clip_total_count, was_inserted)))
}

fn append_copy_text(current: &str, next: &str) -> String {
    let next = next.trim();
    let current = current.trim_end_matches(|value| value == '\r' || value == '\n');

    if current.is_empty() {
        next.to_string()
    } else {
        format!("{current}\n{next}")
    }
}

/// LAN 同步等模块读取当前剪贴板的入口（保持 `read_clipboard_item` 私有，
/// 以免影响 watcher 调用路径）。
pub(crate) fn read_current_clipboard() -> Result<ClipboardRead, String> {
    read_clipboard_item()
}

/// 把当前剪贴板读取结果转换为 LAN 协议要发的 `(clip_type, payload_bytes)`。
/// 图片条目转成 `data:image/png;base64,...` 形式的 UTF-8 字节流；文本条目直接
/// 转 UTF-8 字节。`Empty`/`Occupied` 返回 `Ok(None)`，由调用方决定如何处理。
pub(crate) fn clipboard_read_to_payload(
    read: ClipboardRead,
) -> Result<Option<(String, Vec<u8>)>, String> {
    match read {
        ClipboardRead::Item(item) => {
            let text = if item.clip_type == "image" {
                let png = item
                    .image_bytes
                    .as_ref()
                    .ok_or_else(|| "无图片数据".to_string())?;
                let b64 = general_purpose::STANDARD.encode(png);
                format!("data:image/png;base64,{b64}")
            } else {
                item.text
            };
            Ok(Some((item.clip_type, text.into_bytes())))
        }
        _ => Ok(None),
    }
}

/// 判断剪贴板读取错误是否表示「该格式在当前剪贴板中不可用」，
/// 调用方应 fallback 到另一种格式（如 get_image 失败 → 试 get_text）而非直接报错。
///
/// - `ContentNotAvailable`：剪贴板里没有该格式的内容（标准 fallback 信号）。
/// - `ConversionFailure`：剪贴板有内容但无法转换成请求的格式（例如文本剪贴板
///   调 `get_image`）。macOS 上文本剪贴板调 `get_image` 常返回此变体而非
///   `ContentNotAvailable`，此前未容错会导致 watcher 抛出英文错误
///   "could not be converted to the appropriate format"。
fn format_not_available(error: &ClipboardError) -> bool {
    matches!(
        error,
        ClipboardError::ContentNotAvailable | ClipboardError::ConversionFailure
    )
}

fn read_clipboard_item() -> Result<ClipboardRead, String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;

    #[cfg(target_os = "macos")]
    if let Some(read) = read_clipboard_image_file(&mut clipboard)? {
        return Ok(read);
    }

    match clipboard.get_image() {
        Ok(image) => return captured_item_from_image(image).map(ClipboardRead::Item),
        Err(error) if format_not_available(&error) => {}
        Err(ClipboardError::ClipboardOccupied) => return Ok(ClipboardRead::Occupied),
        Err(error) => return Err(error.to_string()),
    }

    #[cfg(not(target_os = "macos"))]
    if let Some(read) = read_clipboard_image_file(&mut clipboard)? {
        return Ok(read);
    }

    match clipboard.get_text() {
        Ok(text) => {
            let normalized = text.trim();
            if normalized.is_empty() {
                return Ok(ClipboardRead::Empty);
            }

            return Ok(ClipboardRead::Item(CapturedClipboardItem {
                clip_type: detect_clip_type(normalized),
                content_hash: hash_text(normalized),
                preview_text: preview(normalized),
                text: normalized.to_string(),
                image_bytes: None,
            }));
        }
        Err(error) if format_not_available(&error) => {}
        Err(ClipboardError::ClipboardOccupied) => return Ok(ClipboardRead::Occupied),
        Err(error) => return Err(error.to_string()),
    }

    Ok(ClipboardRead::Empty)
}

fn read_clipboard_image_file(clipboard: &mut Clipboard) -> Result<Option<ClipboardRead>, String> {
    match clipboard.get().file_list() {
        Ok(paths) => captured_item_from_file_list(&paths).map(|item| item.map(ClipboardRead::Item)),
        Err(error) if format_not_available(&error) => Ok(None),
        Err(ClipboardError::ClipboardOccupied) => Ok(Some(ClipboardRead::Occupied)),
        Err(error) => Err(error.to_string()),
    }
}

fn should_capture_clipboard_item(
    change_id: Option<u64>,
    content_hash: &str,
    last_clipboard_change_id: &Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: &Arc<Mutex<Option<String>>>,
) -> bool {
    let last_change_id = last_clipboard_change_id
        .lock()
        .map(|last| *last)
        .unwrap_or(None);
    let last_hash = last_clipboard_hash
        .lock()
        .map(|last| last.clone())
        .unwrap_or(None);
    let same_hash = last_hash.as_deref() == Some(content_hash);

    if let Some(id) = change_id {
        if last_change_id == Some(id) && same_hash {
            return false;
        }

        if let Ok(mut last) = last_clipboard_change_id.lock() {
            *last = Some(id);
        }
        if let Ok(mut last_hash) = last_clipboard_hash.lock() {
            *last_hash = Some(content_hash.to_string());
        }
        return true;
    }

    if same_hash {
        return false;
    }

    last_clipboard_hash
        .lock()
        .map(|mut last| {
            *last = Some(content_hash.to_string());
            true
        })
        .unwrap_or(true)
}

pub(crate) fn remember_current_clipboard_marker(
    last_clipboard_change_id: &Arc<Mutex<Option<u64>>>,
    last_clipboard_hash: &Arc<Mutex<Option<String>>>,
    content_hash: Option<String>,
) {
    if let Some(id) = clipboard_change_id() {
        if let Ok(mut last) = last_clipboard_change_id.lock() {
            *last = Some(id);
        }
    }

    if let Some(hash) = content_hash {
        if let Ok(mut last) = last_clipboard_hash.lock() {
            *last = Some(hash);
        }
    }
}

#[cfg(target_os = "windows")]
fn clipboard_change_id() -> Option<u64> {
    let value = unsafe { GetClipboardSequenceNumber() };
    (value != 0).then_some(value as u64)
}

#[cfg(target_os = "macos")]
fn clipboard_change_id() -> Option<u64> {
    let value = NSPasteboard::generalPasteboard().changeCount();
    (value >= 0).then_some(value as u64)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn clipboard_change_id() -> Option<u64> {
    None
}

pub(crate) fn write_clipboard_text(text: &str) -> Result<(), String> {
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard.set_text(text).map_err(|error| error.to_string())
}

pub(crate) fn write_clipboard_image(data_url: &str) -> Result<(), String> {
    let image = image_from_source(data_url)?;
    let mut clipboard = Clipboard::new().map_err(|error| error.to_string())?;
    clipboard
        .set_image(image)
        .map_err(|error| error.to_string())
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

fn captured_item_from_image(image: ImageData<'static>) -> Result<CapturedClipboardItem, String> {
    let width = image.width;
    let height = image.height;
    let bytes = image.bytes.into_owned();
    let png = image_png_bytes(width, height, bytes)?;
    let hash = hash_bytes(&png);

    Ok(CapturedClipboardItem {
        clip_type: "image".to_string(),
        content_hash: hash,
        preview_text: format!("{} x {}", width, height),
        text: String::new(),
        image_bytes: Some(png),
    })
}

fn captured_item_from_file_list(
    paths: &[PathBuf],
) -> Result<Option<CapturedClipboardItem>, String> {
    let [path] = paths else {
        return Ok(None);
    };

    if !is_supported_image_file_path(path) || !path.is_file() {
        return Ok(None);
    }

    image_from_source_path(path)
        .and_then(captured_item_from_image)
        .map(Some)
}

fn is_supported_image_file_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            IMAGE_FILE_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
        .unwrap_or(false)
}

pub(crate) fn captured_item_from_payload(
    clip_type: &str,
    text: &str,
) -> Result<Option<CapturedClipboardItem>, String> {
    if clip_type == "image" {
        let image = image_from_source(text)?;
        return captured_item_from_image(image).map(Some);
    }

    let normalized = text.trim();
    if normalized.is_empty() {
        return Ok(None);
    }

    let clip_type = match clip_type {
        "text" | "link" | "color" | "html" | "file" => clip_type.to_string(),
        _ => detect_clip_type(normalized),
    };

    Ok(Some(CapturedClipboardItem {
        clip_type,
        content_hash: hash_text(normalized),
        preview_text: preview(normalized),
        text: normalized.to_string(),
        image_bytes: None,
    }))
}

fn image_png_bytes(width: usize, height: usize, rgba: Vec<u8>) -> Result<Vec<u8>, String> {
    let expected_len = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "图片尺寸过大".to_string())?;
    if rgba.len() != expected_len {
        return Err("图片数据不完整".to_string());
    }

    let image = ImageBuffer::<Rgba<u8>, _>::from_raw(width as u32, height as u32, rgba)
        .ok_or_else(|| "无法读取图片数据".to_string())?;
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            image.as_raw(),
            width as u32,
            height as u32,
            image::ColorType::Rgba8.into(),
        )
        .map_err(|error| error.to_string())?;

    Ok(png)
}

pub(crate) fn image_bytes_from_data_url(data_url: &str) -> Result<Vec<u8>, String> {
    let (_, encoded) = data_url
        .split_once(";base64,")
        .ok_or_else(|| "不支持的图片格式".to_string())?;
    general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())
}

fn image_from_source(source: &str) -> Result<ImageData<'static>, String> {
    let bytes = if source.starts_with("data:image/") {
        image_bytes_from_data_url(source)?
    } else {
        return image_from_source_path(Path::new(source));
    };

    image_from_bytes(&bytes)
}

fn image_from_source_path(path: &Path) -> Result<ImageData<'static>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    image_from_bytes(&bytes)
}

fn image_from_bytes(bytes: &[u8]) -> Result<ImageData<'static>, String> {
    let decoded = image::load_from_memory(&bytes)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    let width = decoded.width() as usize;
    let height = decoded.height() as usize;

    Ok(ImageData {
        width,
        height,
        bytes: decoded.into_raw().into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_not_available_treats_content_unavailable_as_fallback() {
        assert!(format_not_available(&ClipboardError::ContentNotAvailable));
    }

    #[test]
    fn format_not_available_treats_conversion_failure_as_fallback() {
        // macOS 上文本剪贴板调 get_image 会返回 ConversionFailure 而非 ContentNotAvailable；
        // 必须把它当 fallback 信号，否则 watcher 会向用户抛英文错误。
        assert!(format_not_available(&ClipboardError::ConversionFailure));
    }
}
