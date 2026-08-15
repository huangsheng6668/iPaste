//! 图片 OCR：状态检测与调度（mod）+ Windows 资源安装器（installer）+
//! Windows Tesseract 执行（tesseract）+ macOS Vision 管线（vision）。

use tauri::Emitter;

use crate::events::*;
use crate::models::*;
#[cfg(target_os = "macos")]
use crate::DEFAULT_OCR_MODE;
#[cfg(target_os = "macos")]
use vision::MACOS_OCR_ENGINE_ID;

#[cfg(not(target_os = "macos"))]
pub(crate) mod installer;

#[cfg(not(target_os = "macos"))]
pub(crate) mod tesseract;

#[cfg(target_os = "macos")]
pub(crate) mod vision;

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
        tokio::task::spawn_blocking(move || tesseract::recognize_image_text_inner(&app, image_path))
            .await
            .map_err(|error| error.to_string())?
    }
}
