// commands/ocr.rs — OCR 资源安装/识别/OpenAI 端点配置/屏幕录制权限
use crate::error::AppError;
use crate::models::{
    AppSettings, AppState, CloudOcrPromptMessage, ImageOcrResult, OcrInstallStatus,
    OcrResultPayload, OpenaiOcrProfile,
};
use crate::util::{clean_openai_base_url, clean_openai_model};

use super::apply_settings_update;
use tauri::Manager;

/// recognize_image_text 路径越界时的统一拒绝文案。
const OCR_IMAGE_PATH_REJECTED: &str = "仅支持应用数据目录内的剪贴板图片与截图";

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

/// OCR 识别路径白名单（安全加固第 5 项）：仅放行 `$APPDATA/clip-images/**` 与
/// `$APPDATA/ocr-overlay/**`——两类合法调用方（图片查看器 / 截图冻结帧）之外，
/// 阻断 webview 被攻破时的任意本地图片读取原语。canonicalize 失败按越界拒绝
///（fail-closed）；前缀比较基于 canonicalize 后的 Path::starts_with，杜绝
/// `clip-images-evil/` 类字符串前缀伪造。
fn ensure_recognizable_image_path_impl(
    data_root: &std::path::Path,
    image_path: &std::path::Path,
) -> Result<(), String> {
    // 白名单目录两侧都过 canonicalize：Windows 上 canonicalize 返回 `\\?\`
    // 前缀的 verbatim 路径，与未归一的目录前缀组件不等，合法路径会被误拒。
    let Ok(root) = std::fs::canonicalize(data_root) else {
        return Err(OCR_IMAGE_PATH_REJECTED.to_string());
    };
    let Ok(resolved) = std::fs::canonicalize(image_path) else {
        return Err(OCR_IMAGE_PATH_REJECTED.to_string());
    };
    let allowed = [root.join("clip-images"), root.join("ocr-overlay")];
    if allowed.iter().any(|dir| resolved.starts_with(dir)) {
        Ok(())
    } else {
        Err(OCR_IMAGE_PATH_REJECTED.to_string())
    }
}

fn ensure_recognizable_image_path(
    app: &tauri::AppHandle,
    image_path: &str,
) -> Result<(), AppError> {
    let root = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::internal(format!("无法定位应用数据目录：{e}")))?;
    ensure_recognizable_image_path_impl(&root, std::path::Path::new(image_path))
        .map_err(AppError::internal)
}

#[tauri::command]
pub(crate) async fn recognize_image_text(
    app: tauri::AppHandle,
    image_path: String,
    profile: Option<String>,
    language: Option<String>,
) -> Result<ImageOcrResult, AppError> {
    ensure_recognizable_image_path(&app, &image_path)?;
    crate::ocr::recognize_image(app, image_path, profile, language)
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
pub(crate) fn save_openai_ocr_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    profile: OpenaiOcrProfile,
    prompts: Option<Vec<CloudOcrPromptMessage>>,
) -> Result<AppSettings, AppError> {
    apply_settings_update(&app, state.store.save_openai_ocr_profile(profile, prompts))
}

#[tauri::command]
pub(crate) fn set_openai_ocr_active_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<AppSettings, AppError> {
    apply_settings_update(&app, state.store.set_openai_ocr_active_profile(profile_id))
}

#[tauri::command]
pub(crate) fn delete_openai_ocr_profile(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    profile_id: String,
) -> Result<AppSettings, AppError> {
    apply_settings_update(&app, state.store.delete_openai_ocr_profile(profile_id))
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
/// `profile_id` 为正在编辑的配置 id：哨兵按它解析存储的真实 Key，避免把
/// 激活配置的凭据发给所选配置的接口地址（串 Key）。
#[tauri::command]
pub(crate) async fn test_openai_ocr(
    state: tauri::State<'_, AppState>,
    profile_id: String,
    base_url: String,
    model: String,
    api_key: String,
    prompts: Option<Vec<CloudOcrPromptMessage>>,
) -> Result<bool, AppError> {
    let base_url = clean_openai_base_url(base_url)?;
    let model = clean_openai_model(model)?;
    // 掩码哨兵 = 输入框未改动：按正在编辑的配置 id 读取其存储的真实 Key。
    // 新建草稿（空 id）没有属于自己的凭据，哨兵属于异常输入，直接报错。
    let api_key = if crate::util::is_masked_secret(&api_key) {
        if profile_id.trim().is_empty() {
            return Err(AppError::internal("OCR 接口 API Key 未配置，请重新输入"));
        }
        state
            .store
            .openai_ocr_profile_secret(&profile_id)
            .map_err(AppError::from)?
    } else {
        api_key.trim().to_string()
    };
    // 解析后 Key 为空且接口地址非本地端点：拒绝执行，避免把空凭据发给云端
    // 得到误导性的「连接成功」（本地 vLLM/Ollama 等无鉴权端点仍允许空 Key）。
    if api_key.is_empty() && !crate::ocr::openai::is_local_noauth_endpoint(&base_url) {
        return Err(AppError::internal("OCR 接口 API Key 未配置，请重新输入"));
    }
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
        .ocr
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

#[cfg(test)]
mod image_path_tests {
    use super::ensure_recognizable_image_path_impl;

    fn setup(tag: &str) -> std::path::PathBuf {
        // tempdir 无既有依赖：std::env::temp_dir + 按（测试函数名）唯一子目录，
        // 并行测试不互踩；各测试自行清理。
        let root =
            std::env::temp_dir().join(format!("ipaste-ocr-path-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(root.join("clip-images")).unwrap();
        std::fs::create_dir_all(root.join("ocr-overlay")).unwrap();
        std::fs::write(root.join("clip-images/a.png"), b"png").unwrap();
        std::fs::write(root.join("ocr-overlay/frozen-0.bmp"), b"bmp").unwrap();
        root
    }

    #[test]
    fn allows_clip_images_and_ocr_overlay() {
        let root = setup("allows");
        assert!(
            ensure_recognizable_image_path_impl(&root, &root.join("clip-images/a.png")).is_ok()
        );
        assert!(
            ensure_recognizable_image_path_impl(&root, &root.join("ocr-overlay/frozen-0.bmp"))
                .is_ok()
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_outside_and_prefix_spoof() {
        let root = setup("rejects");
        // 白名单外绝对路径
        let outside = std::env::temp_dir().join(format!("evil-{}.png", std::process::id()));
        std::fs::write(&outside, b"png").unwrap();
        assert!(ensure_recognizable_image_path_impl(&root, &outside).is_err());
        std::fs::remove_file(&outside).ok();
        // 字符串前缀伪造：clip-images-evil/ 目录
        let evil_dir = root.join("clip-images-evil");
        std::fs::create_dir_all(&evil_dir).unwrap();
        std::fs::write(evil_dir.join("x.png"), b"png").unwrap();
        assert!(ensure_recognizable_image_path_impl(&root, &evil_dir.join("x.png")).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn rejects_missing_path_fail_closed() {
        let root = setup("missing");
        // canonicalize 失败（文件不存在）→ 按越界拒绝（fail-closed）
        assert!(
            ensure_recognizable_image_path_impl(&root, &root.join("clip-images/ghost.png"))
                .is_err()
        );
        std::fs::remove_dir_all(&root).ok();
    }
}
