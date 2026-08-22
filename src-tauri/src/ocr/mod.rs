//! 图片 OCR：状态检测与调度（mod）+ Windows 资源安装器（installer）+
//! Windows Paddle 识别管线（paddle）+
//! macOS Vision 管线（vision）+ 跨平台行内分词（tokens）。

use tauri::Emitter;

use crate::events::EVENT_OCR_INSTALL_PROGRESS;
use crate::models::{ImageOcrResult, OcrInstallProgress, OcrInstallStatus};
#[cfg(not(target_os = "macos"))]
use tauri::Manager;

use crate::DEFAULT_OCR_MODE;
#[cfg(target_os = "macos")]
use vision::MACOS_OCR_ENGINE_ID;

pub(crate) mod installer;

#[cfg(not(target_os = "macos"))]
pub(crate) mod paddle;

#[cfg(target_os = "macos")]
pub(crate) mod vision;

/// 行内分词纯逻辑：macOS Vision 与 Windows Paddle（paddle.rs）共用。
pub(crate) mod tokens;

/// Manga-OCR (mocr) 专用日漫推理桥接器。
pub(crate) mod mocr;
/// Manga-OCR 的 ONNX 本地推理引擎（无 Python 依赖，主路径）。
pub(crate) mod mocr_onnx;
/// Manga-OCR 模型安装器（设置页「日语 · 漫画」模型下载）。
/// Windows 与 macOS（onnx sidecar 分发平台）；Intel Mac 亦可下载（引擎缺失
/// 时识别走回退）。复用的 installer.rs 基础函数已全平台化。
#[cfg(any(windows, target_os = "macos"))]
pub(crate) mod mocr_installer;

fn ocr_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        return "macos-system";
    }

    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    {
        "windows-x64"
    }
    #[cfg(not(any(
        target_os = "macos",
        all(target_os = "windows", target_arch = "x86_64")
    )))]
    {
        "unsupported"
    }
}

pub(crate) fn emit_ocr_install_progress(
    app: &tauri::AppHandle,
    phase: &str,
    file_name: Option<String>,
    downloaded_bytes: u64,
    total_bytes: u64,
) {
    let _ = app.emit(
        EVENT_OCR_INSTALL_PROGRESS,
        OcrInstallProgress {
            phase: phase.to_string(),
            file_name,
            downloaded_bytes,
            total_bytes,
        },
    );
}

#[cfg(target_os = "macos")]
pub(crate) fn macos_ocr_install_status() -> Result<OcrInstallStatus, String> {
    Ok(OcrInstallStatus {
        installed: true,
        engine_id: MACOS_OCR_ENGINE_ID.to_string(),
        engine_version: Some("system".to_string()),
        mode: DEFAULT_OCR_MODE.to_string(),
        platform: ocr_platform().to_string(),
        manifest_url: String::new(),
        install_dir: String::new(),
        downloaded_bytes: 0,
        total_bytes: 0,
        missing_files: Vec::new(),
    })
}

/// 供 commands.rs 使用的跨平台调度入口（原命令体内的 cfg 分支收编于此）。

pub(crate) fn install_status(
    app: &tauri::AppHandle,
    store: &crate::store::Store,
) -> Result<OcrInstallStatus, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, store);
        macos_ocr_install_status()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mode = store.settings()?.ocr_mode;
        installer::ocr_install_status(app, &mode)
    }
}

pub(crate) async fn install_assets(
    app: tauri::AppHandle,
    store: crate::store::Store,
) -> Result<OcrInstallStatus, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = &store;
        emit_ocr_install_progress(&app, "completed", None, 0, 0);
        macos_ocr_install_status()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mode = store.settings()?.ocr_mode;
        tokio::task::spawn_blocking(move || installer::install_ocr_assets_inner(&app, &mode))
            .await
            .map_err(|error| error.to_string())?
    }
}

pub(crate) fn remove_assets(
    app: &tauri::AppHandle,
    store: &crate::store::Store,
) -> Result<OcrInstallStatus, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = (app, store);
        macos_ocr_install_status()
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mode = store.settings()?.ocr_mode;
        let root = installer::ocr_root_dir(app)?;
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        }
        installer::ocr_install_status(app, &mode)
    }
}

// —— Manga-OCR 模型资产管理（设置页下载/删除/状态；onnx 资产服务于
// Windows 与 macOS aarch64 sidecar，Intel Mac 可下载模型但识别走回退）——

pub(crate) fn mocr_install_status(app: &tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        mocr_installer::mocr_install_status(app)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
        Ok(mocr_unsupported_status())
    }
}

pub(crate) async fn install_mocr_assets(app: tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    #[cfg(any(windows, target_os = "macos"))]
    {
        tokio::task::spawn_blocking(move || mocr_installer::install_mocr_assets_inner(&app))
            .await
            .map_err(|error| error.to_string())?
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("Manga-OCR 模型下载不支持当前平台".to_string())
    }
}

/// 删除 mocr 模型：先结束常驻推理进程（Windows 下文件被占用删不掉），再清目录。
pub(crate) async fn remove_mocr_assets(app: tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    mocr::shutdown_server().await;
    #[cfg(any(windows, target_os = "macos"))]
    {
        tokio::task::spawn_blocking(move || mocr_installer::remove_mocr_assets_inner(&app))
            .await
            .map_err(|error| error.to_string())?
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Err("Manga-OCR 模型下载不支持当前平台".to_string())
    }
}

/// 无内置引擎平台的占位状态（Linux 等）。
#[cfg(not(any(windows, target_os = "macos")))]
fn mocr_unsupported_status() -> OcrInstallStatus {
    OcrInstallStatus {
        installed: false,
        engine_id: "mocr".to_string(),
        engine_version: None,
        mode: "mocr".to_string(),
        platform: "unsupported".to_string(),
        manifest_url: String::new(),
        install_dir: String::new(),
        downloaded_bytes: 0,
        total_bytes: 0,
        missing_files: Vec::new(),
    }
}

/// OCR 语言 id → macOS Vision 识别语言（BCP 47）；auto/未知 → None（默认 5 语 + 自动检测）。
/// 平台无关纯函数，Windows 构建下也参与单测。
/// 仅 macOS Vision 管线（vision.rs）调用，非 macOS 非测试构建无调用方，
/// allow(dead_code) 消除平台性警告（与 tokens.rs char_index_to_utf16 同法）。
#[allow(dead_code)]
pub(crate) fn vision_language_locale(language: &str) -> Option<&'static str> {
    match language {
        "zh-Hans" => Some("zh-Hans"),
        "zh-Hant" => Some("zh-Hant"),
        "en" => Some("en-US"),
        "ja" => Some("ja-JP"),
        _ => None,
    }
}

pub(crate) async fn recognize_image(
    app: tauri::AppHandle,
    image_path: String,
    profile: Option<String>,
    language: Option<String>,
) -> Result<ImageOcrResult, String> {
    let language = crate::util::clean_ocr_language(language);
    let is_manga = profile
        .as_deref()
        .map(|p| p.eq_ignore_ascii_case("manga") || p.eq_ignore_ascii_case("japanese"))
        .unwrap_or(false);

    if is_manga {
        // mocr.rs 内部按引擎可用性调度：ONNX sidecar（主）→ Python 常驻（回退）；
        // 全部失败则落入下方 Paddle manga 管线
        let app_handle = app.clone();
        let img_path = image_path.clone();
        if let Ok(mocr_res) = mocr::recognize_image_text_mocr(Some(&app_handle), &img_path).await {
            return Ok(mocr_res);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        tokio::task::spawn_blocking(move || {
            vision::recognize_image_text_macos(image_path, language)
        })
        .await
        .map_err(|error| error.to_string())?
    }

    #[cfg(not(target_os = "macos"))]
    {
        // 本函数签名不含 store（调用方 commands.rs 只有 AppHandle），
        // paddle 管线经 store 读取 ocr_mode，故在此从 AppState 取。
        // AppState 理论上总已托管（setup 先于任何命令）；取不到时降级为
        // 默认模式 fast，仍走同一管线（引擎缓存按 fast 模型路径识别）。
        let store = app
            .try_state::<crate::models::AppState>()
            .map(|state| state.store.clone());
        tokio::task::spawn_blocking(move || match store.as_ref() {
            Some(store) => {
                paddle::recognize_image_text_paddle(&app, store, image_path, profile, language)
            }
            None => paddle::recognize_with_mode(
                &app,
                DEFAULT_OCR_MODE,
                image_path,
                profile,
                language,
            ),
        })
        .await
        .map_err(|error| error.to_string())?
    }
}

#[cfg(test)]
mod language_tests {
    use super::vision_language_locale;

    #[test]
    fn vision_language_locale_maps_supported_ids() {
        assert_eq!(vision_language_locale("zh-Hans"), Some("zh-Hans"));
        assert_eq!(vision_language_locale("zh-Hant"), Some("zh-Hant"));
        assert_eq!(vision_language_locale("en"), Some("en-US"));
        assert_eq!(vision_language_locale("ja"), Some("ja-JP"));
        assert_eq!(vision_language_locale("auto"), None);
        assert_eq!(vision_language_locale("korean"), None);
    }
}
