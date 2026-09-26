//! OCR 资源清单：拉取、校验、缓存与下载地址解析（Task 34 从 ocr/installer.rs 拆出）。
//! 全部为 Windows（非 macOS）路径：macOS 走系统 Vision，不安装 Paddle 资源。

use std::path::{Path, PathBuf};
use std::{fs, time::Duration};

use reqwest::blocking::Client;
use tauri::Manager;

use super::super::{emit_ocr_install_progress, ocr_platform};
use super::paths::{ensure_path_within, ocr_root_dir};
use super::{
    OCR_BEST_TOTAL_BYTES, OCR_ENGINE_ID, OCR_FAST_TOTAL_BYTES, OCR_GITHUB_RELEASE_BASE_URL,
    OCR_R2_BASE_URL, UPDATER_R2_ENDPOINT,
};
use crate::models::{OcrInstallStatus, OcrManifest, OcrManifestFile};
use crate::util::{clean_ocr_mode, file_sha256, validate_relative_path};

#[cfg(not(target_os = "macos"))]
pub(super) fn fetch_ocr_manifest(mode: &str) -> Result<OcrManifest, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| error.to_string())?;
    let mut errors = Vec::new();

    for manifest_url in ocr_manifest_urls(mode) {
        match fetch_ocr_manifest_from_url(&client, &manifest_url, mode) {
            Ok(manifest) => return Ok(manifest),
            Err(error) => errors.push(format!("{manifest_url}：{error}")),
        }
    }

    Err(format!("无法获取 OCR 资源信息：{}", errors.join("；")))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn fetch_ocr_manifest_from_url(
    client: &Client,
    manifest_url: &str,
    mode: &str,
) -> Result<OcrManifest, String> {
    let response = client
        .get(manifest_url)
        .send()
        .map_err(|error| error.to_string())?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }

    let manifest = response
        .json::<OcrManifest>()
        .map_err(|error| format!("无法解析 OCR manifest：{error}"))?;
    validate_ocr_manifest(&manifest, mode)?;
    Ok(manifest)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn validate_ocr_manifest(manifest: &OcrManifest, mode: &str) -> Result<(), String> {
    if manifest.engine.id != OCR_ENGINE_ID {
        return Err("OCR manifest 引擎不受支持".to_string());
    }
    if manifest.engine.mode.as_deref().unwrap_or(mode) != mode {
        return Err(format!("OCR manifest 模式不匹配：{mode}"));
    }
    if manifest.engine.platform != ocr_platform() {
        return Err(format!(
            "OCR manifest 平台不匹配：{}",
            manifest.engine.platform
        ));
    }
    if !manifest.engine.base_url.starts_with("https://") {
        return Err("OCR manifest 下载地址不安全".to_string());
    }
    if manifest.engine.files.is_empty() {
        return Err("OCR manifest 没有文件".to_string());
    }
    for file in &manifest.engine.files {
        if file.name.contains('/') || file.name.contains('\\') || file.name.contains("..") {
            return Err(format!("OCR 文件名不安全：{}", file.name));
        }
        if file.path.contains("..") {
            return Err(format!("OCR 文件路径不安全：{}", file.path));
        }
        // v2：仅接受 MNN 模型与字典三类角色，老清单（engine/language 等）直接拒绝
        if !matches!(file.role.as_str(), "det-model" | "rec-model" | "charset") {
            return Err(format!("OCR manifest 文件角色不受支持：{}", file.role));
        }
        // v2：模型为直接下载的裸文件，禁止 zip 等压缩包
        if file.archive.is_some() {
            return Err(format!("OCR 模型文件禁止使用压缩包：{}", file.name));
        }
        if let Some(install_dir) = &file.install_dir {
            validate_relative_path(install_dir)?;
        }
        for entry in &file.entries {
            validate_relative_path(entry)?;
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn ocr_install_status(app: &tauri::AppHandle, mode: &str) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    match read_ocr_manifest_cache(app, &mode)? {
        Some(manifest) => ocr_install_status_for_manifest(app, &manifest, &mode),
        None => {
            let install_dir = ocr_root_dir(app)?;
            Ok(OcrInstallStatus {
                installed: false,
                engine_id: OCR_ENGINE_ID.to_string(),
                engine_version: None,
                mode: mode.clone(),
                platform: ocr_platform().to_string(),
                manifest_url: ocr_primary_manifest_url(&mode),
                install_dir: install_dir.to_string_lossy().to_string(),
                downloaded_bytes: 0,
                total_bytes: ocr_default_total_bytes(&mode),
                missing_files: Vec::new(),
            })
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_install_status_for_manifest(
    app: &tauri::AppHandle,
    manifest: &OcrManifest,
    mode: &str,
) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    let install_dir = ocr_root_dir(app)?;
    let mut downloaded_bytes = 0_u64;
    let mut missing_files = Vec::new();

    for file in &manifest.engine.files {
        if ocr_manifest_file_installed(app, file)? {
            downloaded_bytes = downloaded_bytes.saturating_add(file.size);
        } else {
            missing_files.push(file.name.clone());
        }
    }

    let total_bytes = manifest_total_bytes(manifest);
    Ok(OcrInstallStatus {
        installed: missing_files.is_empty() && !manifest.engine.files.is_empty(),
        engine_id: manifest.engine.id.clone(),
        engine_version: Some(manifest.engine.version.clone()),
        mode: mode.clone(),
        platform: manifest.engine.platform.clone(),
        manifest_url: ocr_primary_manifest_url(&mode),
        install_dir: install_dir.to_string_lossy().to_string(),
        downloaded_bytes,
        total_bytes,
        missing_files,
    })
}

#[cfg(not(target_os = "macos"))]
pub(super) fn manifest_total_bytes(manifest: &OcrManifest) -> u64 {
    manifest.engine.files.iter().map(|file| file.size).sum()
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_default_total_bytes(mode: &str) -> u64 {
    match mode {
        "best" => OCR_BEST_TOTAL_BYTES,
        _ => OCR_FAST_TOTAL_BYTES,
    }
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_manifest_urls(mode: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for base_url in ocr_r2_base_urls() {
        push_unique_url(&mut urls, ocr_manifest_url_for_base(&base_url, mode));
    }
    push_unique_url(
        &mut urls,
        ocr_manifest_url_for_base(OCR_GITHUB_RELEASE_BASE_URL, mode),
    );
    urls
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_primary_manifest_url(mode: &str) -> String {
    ocr_manifest_urls(mode)
        .into_iter()
        .next()
        .unwrap_or_else(|| ocr_manifest_url_for_base(OCR_GITHUB_RELEASE_BASE_URL, mode))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_manifest_url_for_base(base_url: &str, mode: &str) -> String {
    format!("{base_url}ipaste-ocr-windows-x64-{mode}.json")
}

pub(crate) fn ocr_r2_base_urls() -> Vec<String> {
    let mut base_urls = Vec::new();

    if let Ok(base_url) = std::env::var("IPASTE_OCR_R2_BASE_URL") {
        push_optional_base_url(&mut base_urls, normalize_ocr_base_url(&base_url));
    }
    push_optional_base_url(&mut base_urls, normalize_ocr_base_url(OCR_R2_BASE_URL));

    if let Ok(endpoint) = std::env::var("IPASTE_UPDATER_R2_ENDPOINT") {
        push_optional_base_url(&mut base_urls, derive_ocr_r2_base_url(&endpoint));
    }
    push_optional_base_url(&mut base_urls, derive_ocr_r2_base_url(UPDATER_R2_ENDPOINT));

    base_urls
}

pub(super) fn push_optional_base_url(base_urls: &mut Vec<String>, base_url: Option<String>) {
    if let Some(base_url) = base_url {
        push_unique_url(base_urls, base_url);
    }
}

pub(super) fn push_unique_url(urls: &mut Vec<String>, url: String) {
    if !urls.iter().any(|existing| existing == &url) {
        urls.push(url);
    }
}

pub(super) fn normalize_ocr_base_url(base_url: &str) -> Option<String> {
    let base_url = base_url.trim();
    if base_url.is_empty() || !base_url.starts_with("https://") {
        return None;
    }
    let base_url = base_url.split(['?', '#']).next().unwrap_or(base_url);
    Some(if base_url.ends_with('/') {
        base_url.to_string()
    } else {
        format!("{base_url}/")
    })
}

pub(super) fn derive_ocr_r2_base_url(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() || !endpoint.starts_with("https://") {
        return None;
    }
    let endpoint = endpoint
        .split(['?', '#'])
        .next()
        .unwrap_or(endpoint)
        .trim_end_matches('/');
    let parent_index = endpoint.rfind('/')?;
    let parent = &endpoint[..parent_index];
    if parent.len() <= "https://".len() {
        return None;
    }
    Some(format!("{parent}/ocr/"))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_manifest_file_installed(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<bool, String> {
    // v2 模型均为裸文件（validator 已拒绝压缩包），落点即 file.path 描述的布局
    let target_path = ocr_manifest_file_path(app, file)?;
    file_is_valid(&target_path, &file.sha256)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_manifest_file_path(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    // v2：模型/字典均为无压缩包的直接下载，落点即 file.path 描述的
    // ocr/paddle/{mode}/ 布局；路径安全在解析时兜底（缓存清单未经 validate）
    let root = ocr_root_dir(app)?;
    validate_relative_path(&file.path)?;
    let resolved = root.join(&file.path);
    ensure_path_within(&root, &resolved)?;
    Ok(resolved)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn file_is_valid(path: &PathBuf, expected_sha256: &str) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let hash = file_sha256(path)?;
    Ok(hash.eq_ignore_ascii_case(expected_sha256))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn read_ocr_manifest_cache(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<Option<OcrManifest>, String> {
    let path = ocr_manifest_cache_path(app, mode)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let manifest =
        serde_json::from_str::<OcrManifest>(&content).map_err(|error| error.to_string())?;
    // 旧引擎（v1 时代）缓存一律视为无缓存，强制重新拉取 v2 清单，
    // 避免老用户被旧缓存骗成「已安装」
    if !is_usable_cached_manifest(&manifest) {
        return Ok(None);
    }
    Ok(Some(manifest))
}

#[cfg(not(target_os = "macos"))]
pub(super) fn write_ocr_manifest_cache(
    app: &tauri::AppHandle,
    mode: &str,
    manifest: &OcrManifest,
) -> Result<(), String> {
    let path = ocr_manifest_cache_path(app, mode)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
pub(super) fn ocr_manifest_cache_path(app: &tauri::AppHandle, mode: &str) -> Result<PathBuf, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    Ok(ocr_root_dir(app)?.join(format!("manifest-{mode}.json")))
}

/// 缓存清单可用性判定：引擎 id 不是当前 paddle 引擎的缓存一律作废。
#[cfg(not(target_os = "macos"))]
pub(super) fn is_usable_cached_manifest(manifest: &OcrManifest) -> bool {
    manifest.engine.id == OCR_ENGINE_ID
}

