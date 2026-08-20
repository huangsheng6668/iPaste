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

#[cfg(not(target_os = "macos"))]
pub(crate) mod installer;

#[cfg(not(target_os = "macos"))]
pub(crate) mod paddle;

#[cfg(target_os = "macos")]
pub(crate) mod vision;

/// 行内分词纯逻辑：macOS Vision 与 Windows Paddle（paddle.rs）共用。
pub(crate) mod tokens;

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

pub(crate) async fn recognize_image(
    app: tauri::AppHandle,
    image_path: String,
    profile: Option<String>,
) -> Result<ImageOcrResult, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        tokio::task::spawn_blocking(move || vision::recognize_image_text_macos(image_path))
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
            Some(store) => paddle::recognize_image_text_paddle(&app, store, image_path, profile),
            None => paddle::recognize_with_mode(&app, DEFAULT_OCR_MODE, image_path, profile),
        })
        .await
        .map_err(|error| error.to_string())?
    }
}
