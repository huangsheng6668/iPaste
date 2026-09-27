//! OCR 资源路径解析与越界防护（Task 34 从 ocr/installer.rs 拆出）。

// 全平台保留 `ensure_path_within` / `ocr_root_dir`（mocr_installer 复用），
// Paddle 布局函数在 macOS 上被 cfg 裁掉，其专用 import 随之未使用。
#![allow(unused_imports)]

use std::fs;
use std::path::{Path, PathBuf};

use tauri::Manager;

use super::OCR_DIR;
#[cfg(not(target_os = "macos"))]
use super::{OCR_ASSET_DIR, OCR_CHARSET_FILE, OCR_MODEL_DIR};
use crate::util::{clean_ocr_mode, validate_relative_path};

pub(crate) fn ensure_path_within(root: &Path, path: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root).map_err(std::io::Error::other)?;
            root.canonicalize()
        })
        .map_err(|error| error.to_string())?;
    let path = if path.exists() {
        path.canonicalize().map_err(|error| error.to_string())?
    } else {
        let parent = path
            .parent()
            .ok_or_else(|| "OCR 路径无父目录".to_string())?;
        let parent = parent
            .canonicalize()
            .or_else(|_| {
                fs::create_dir_all(parent).map_err(std::io::Error::other)?;
                parent.canonicalize()
            })
            .map_err(|error| error.to_string())?;
        parent.join(
            path.file_name()
                .ok_or_else(|| "OCR 路径无文件名".to_string())?,
        )
    };

    if path.starts_with(root) {
        Ok(())
    } else {
        Err("OCR 路径越界".to_string())
    }
}

pub(crate) fn ocr_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| error.to_string())
        .map(|path| path.join(OCR_DIR))
}

/// 单个 OCR 模式的模型文件布局（app-data 下相对 ocr 根目录）。
#[cfg(not(target_os = "macos"))]
pub(crate) struct PaddleModelPaths {
    pub(crate) det: PathBuf,
    pub(crate) rec: PathBuf,
    pub(crate) charset: PathBuf,
}

/// 纯路径版布局函数：返回 ocr/paddle/{mode}/det.mnn、rec.mnn、ppocr_keys_v5.txt。
/// mode 必须过 clean_ocr_mode，非法值（含 "../evil"）返回 Err；
/// 再经 validate_relative_path + ensure_path_within 双重防越界。
#[cfg(not(target_os = "macos"))]
pub(crate) fn paddle_model_paths_under(
    root: &Path,
    mode: &str,
) -> Result<PaddleModelPaths, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    let mode_dir_relative = format!("{OCR_MODEL_DIR}/{mode}");
    validate_relative_path(&mode_dir_relative)?;
    let mode_dir = root.join(mode_dir_relative);
    ensure_path_within(root, &mode_dir)?;
    Ok(PaddleModelPaths {
        det: mode_dir.join("det.mnn"),
        rec: mode_dir.join("rec.mnn"),
        charset: mode_dir.join(OCR_CHARSET_FILE),
    })
}

/// paddle.rs::ensure_engine 消费：返回 app-data 下的模型文件布局。
#[cfg(not(target_os = "macos"))]
pub(crate) fn paddle_model_paths(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<PaddleModelPaths, String> {
    paddle_model_paths_under(&ocr_root_dir(app)?, mode)
}

/// 旧版 v1 安装器的残留目录：存在即代表需要清理。
/// 返回 v1 引擎目录、downloads、assets 三个路径。
#[cfg(not(target_os = "macos"))]
pub(crate) fn legacy_ocr_paths(app: &tauri::AppHandle) -> Vec<PathBuf> {
    let Ok(root) = ocr_root_dir(app) else {
        return Vec::new();
    };
    vec![
        // v2 之前安装器的引擎目录名：历史字面量，与当前引擎无关
        root.join("tesseract"),
        root.join("downloads"),
        root.join(OCR_ASSET_DIR),
    ]
}
