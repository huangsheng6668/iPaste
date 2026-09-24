// commands/ocr.rs — OCR 资源安装/识别/OpenAI 端点配置/屏幕录制权限
use crate::error::AppError;
use crate::models::{
    AppSettings, AppState, CloudOcrPromptMessage, ImageOcrResult, OcrInstallStatus,
    OcrResultPayload,
};
use crate::util::{clean_api_key, clean_openai_base_url, clean_openai_model};

use super::apply_settings_update;

#[tauri::command]
pub(crate) fn get_ocr_install_status(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::install_status(&_app, &_state.store).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn install_ocr_assets(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::install_assets(_app, _state.store.clone())
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn remove_ocr_assets(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, AppState>,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::remove_assets(&_app, &_state.store).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn get_mocr_install_status(
    _app: tauri::AppHandle,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::mocr_install_status(&_app).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn install_mocr_assets(
    _app: tauri::AppHandle,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::install_mocr_assets(_app)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn remove_mocr_assets(
    _app: tauri::AppHandle,
) -> Result<OcrInstallStatus, AppError> {
    crate::ocr::remove_mocr_assets(_app)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn recognize_image_text(
    _app: tauri::AppHandle,
    image_path: String,
    profile: Option<String>,
    language: Option<String>,
) -> Result<ImageOcrResult, AppError> {
    crate::ocr::recognize_image(_app, image_path, profile, language)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_openai_ocr_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    base_url: String,
    model: String,
    api_key: String,
    prompts: Option<Vec<CloudOcrPromptMessage>>,
) -> Result<AppSettings, AppError> {
    apply_settings_update(
        &app,
        state
            .store
            .update_openai_ocr_config(base_url, model, api_key, prompts),
    )
}

#[tauri::command]
pub(crate) fn clear_openai_ocr_config(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, AppError> {
    apply_settings_update(&app, state.store.clear_openai_ocr_config())
}

/// 设置页「测试」按钮：上传空白小图跑一次视觉模型转写，验证 Base URL /
/// 模型 / Key 的组合可用（不校验转写内容）。
#[tauri::command]
pub(crate) async fn test_openai_ocr(
    base_url: String,
    model: String,
    api_key: String,
    prompts: Option<Vec<CloudOcrPromptMessage>>,
) -> Result<bool, AppError> {
    let base_url = clean_openai_base_url(base_url)?;
    let model = clean_openai_model(model)?;
    let api_key = clean_api_key(api_key).map_err(|_| "请输入 API Key".to_string())?;
    tokio::task::spawn_blocking(move || {
        crate::ocr::openai::test_connection(&base_url, &model, &api_key, prompts.as_deref())
    })
    .await
    .map_err(|error| AppError::internal(error.to_string()))?
    .map(|_| true)
    .map_err(AppError::internal)
}

#[tauri::command]
pub(crate) fn get_ocr_result_payload(
    state: tauri::State<'_, AppState>,
    token: String,
) -> Result<OcrResultPayload, AppError> {
    state
        .ocr_result_payloads
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .remove(&token)
        .ok_or_else(|| AppError::internal("结果载荷不存在或已过期"))
}

#[tauri::command]
pub(crate) fn screen_capture_permission_status() -> Result<bool, AppError> {
    Ok(crate::capture::screen::screen_capture_permission_granted())
}

#[tauri::command]
pub(crate) fn request_screen_capture_permission() -> Result<bool, AppError> {
    Ok(crate::capture::screen::request_screen_capture_permission())
}
