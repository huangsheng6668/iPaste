// commands/screenshot.rs — 截图 OCR 会话生命周期
use crate::error::AppError;
use crate::models::ScreenshotSelection;

#[tauri::command]
pub(crate) async fn start_screenshot_ocr(app: tauri::AppHandle) -> Result<(), AppError> {
    tauri::async_runtime::spawn_blocking(move || crate::capture::start_screenshot_ocr(&app))
        .await
        .map_err(|error| AppError::internal(error.to_string()))?
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn submit_screenshot_selection(
    app: tauri::AppHandle,
    selection: ScreenshotSelection,
) -> Result<(), AppError> {
    crate::capture::submit_screenshot_selection(app, selection).await
}

#[tauri::command]
pub(crate) fn cancel_screenshot_ocr(app: tauri::AppHandle) -> Result<(), AppError> {
    crate::capture::cancel_screenshot_ocr(&app).map_err(AppError::from)
}
