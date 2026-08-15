use std::path::{Path, PathBuf};
#[cfg(not(target_os = "macos"))]
use std::{fs, process::Command, time::Duration};
#[cfg(not(target_os = "macos"))]
use std::io::{Read, Write};

#[cfg(target_os = "macos")]
use objc2::{
    ffi::NSUInteger,
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject, Bool},
    sel,
};
#[cfg(target_os = "macos")]
use objc2_core_foundation::CGRect;
#[cfg(target_os = "macos")]
use objc2_foundation::{NSArray, NSError, NSRange, NSString, NSURL};
#[cfg(not(target_os = "macos"))]
use reqwest::blocking::Client;
use tauri::Emitter;
#[cfg(not(target_os = "macos"))]
use tauri::Manager;
#[cfg(not(target_os = "macos"))]
use zip::ZipArchive;

use crate::events::*;
use crate::models::*;
#[cfg(not(target_os = "macos"))]
use crate::util::*;
#[cfg(target_os = "macos")]
use crate::DEFAULT_OCR_MODE;

#[cfg(not(target_os = "macos"))]
const OCR_GITHUB_RELEASE_BASE_URL: &str =
    "https://github.com/iPaste-app/iPaste/releases/download/ipaste-ocr-windows-v1/";
#[cfg(not(target_os = "macos"))]
const OCR_R2_BASE_URL: &str = env!("IPASTE_OCR_R2_BASE_URL");
#[cfg(not(target_os = "macos"))]
const UPDATER_R2_ENDPOINT: &str = env!("IPASTE_UPDATER_R2_ENDPOINT");
#[cfg(not(target_os = "macos"))]
const OCR_DIR: &str = "ocr";
#[cfg(not(target_os = "macos"))]
const OCR_ASSET_DIR: &str = "assets";
#[cfg(not(target_os = "macos"))]
const OCR_ENGINE_DIR: &str = "tesseract";
#[cfg(not(target_os = "macos"))]
const OCR_FAST_TOTAL_BYTES: u64 = 37_557_099;
#[cfg(not(target_os = "macos"))]
const OCR_BEST_TOTAL_BYTES: u64 = 59_452_879;
#[cfg(target_os = "macos")]
const MACOS_OCR_ENGINE_ID: &str = "apple-vision";
#[cfg(target_os = "macos")]
const MACOS_OCR_LANGUAGE: &str = "zh-Hans+en";
#[cfg(target_os = "macos")]
const MACOS_OCR_RECOGNITION_LEVEL_ACCURATE: isize = 0;
#[cfg(not(target_os = "macos"))]
pub(crate) fn install_ocr_assets_inner(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    emit_ocr_install_progress(app, "fetchingManifest", None, 0, 0);
    let manifest = fetch_ocr_manifest(&mode)?;

    let asset_dir = ocr_asset_dir(app)?;
    let download_dir = ocr_download_dir(app)?;
    fs::create_dir_all(&asset_dir).map_err(|error| error.to_string())?;
    fs::create_dir_all(&download_dir).map_err(|error| error.to_string())?;
    let total_bytes = manifest_total_bytes(&manifest);
    let mut downloaded_bytes = 0_u64;

    emit_ocr_install_progress(app, "downloading", None, downloaded_bytes, total_bytes);

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|error| error.to_string())?;

    for file in &manifest.engine.files {
        if ocr_manifest_file_installed(app, file)? {
            downloaded_bytes = downloaded_bytes.saturating_add(file.size);
            emit_ocr_install_progress(
                app,
                "downloading",
                Some(file.name.clone()),
                downloaded_bytes.min(total_bytes),
                total_bytes,
            );
            continue;
        }

        let url = format!("{}{}", manifest.engine.base_url, file.path);
        let target_path = ocr_download_target_path(app, file)?;
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let temp_path = target_path.with_extension("download");
        let mut response = client.get(url).send().map_err(|error| error.to_string())?;

        if !response.status().is_success() {
            return Err(format!(
                "{} 下载失败：{}",
                file.name,
                response.status().as_u16()
            ));
        }

        let mut output = fs::File::create(&temp_path).map_err(|error| error.to_string())?;
        let mut buffer = [0_u8; 64 * 1024];
        let file_start_bytes = downloaded_bytes;
        let mut file_bytes = 0_u64;

        loop {
            let read = response
                .read(&mut buffer)
                .map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }

            output
                .write_all(&buffer[..read])
                .map_err(|error| error.to_string())?;
            file_bytes = file_bytes.saturating_add(read as u64);
            emit_ocr_install_progress(
                app,
                "downloading",
                Some(file.name.clone()),
                file_start_bytes.saturating_add(file_bytes).min(total_bytes),
                total_bytes,
            );
        }

        output.flush().map_err(|error| error.to_string())?;
        let hash = file_sha256(&temp_path)?;
        if !hash.eq_ignore_ascii_case(&file.sha256) {
            let _ = fs::remove_file(&temp_path);
            return Err(format!("{} 校验失败", file.name));
        }

        fs::rename(&temp_path, &target_path).map_err(|error| error.to_string())?;
        if file.archive.as_deref() == Some("zip") {
            install_ocr_zip_archive(app, file, &target_path)?;
            let _ = fs::remove_file(&target_path);
        }
        downloaded_bytes = file_start_bytes.saturating_add(file.size);
    }

    write_ocr_manifest_cache(app, &mode, &manifest)?;
    let status = ocr_install_status_for_manifest(app, &manifest, &mode)?;
    emit_ocr_install_progress(
        app,
        "completed",
        None,
        status.downloaded_bytes,
        status.total_bytes,
    );
    Ok(status)
}

#[cfg(not(target_os = "macos"))]
fn fetch_ocr_manifest(mode: &str) -> Result<OcrManifest, String> {
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
fn fetch_ocr_manifest_from_url(
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
fn validate_ocr_manifest(manifest: &OcrManifest, mode: &str) -> Result<(), String> {
    if manifest.engine.id != "tesseract" {
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
        if file.role == "engine" && file.archive.as_deref() != Some("zip") {
            return Err("OCR 引擎需要使用 portable zip 包".to_string());
        }
        if let Some(archive) = &file.archive {
            if archive != "zip" {
                return Err(format!("OCR archive 类型不受支持：{archive}"));
            }
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
                engine_id: "tesseract".to_string(),
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

#[cfg(not(target_os = "macos"))]
fn ocr_install_status_for_manifest(
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
fn manifest_total_bytes(manifest: &OcrManifest) -> u64 {
    manifest.engine.files.iter().map(|file| file.size).sum()
}

#[cfg(not(target_os = "macos"))]
fn ocr_default_total_bytes(mode: &str) -> u64 {
    match mode {
        "best" => OCR_BEST_TOTAL_BYTES,
        _ => OCR_FAST_TOTAL_BYTES,
    }
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_urls(mode: &str) -> Vec<String> {
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
fn ocr_primary_manifest_url(mode: &str) -> String {
    ocr_manifest_urls(mode)
        .into_iter()
        .next()
        .unwrap_or_else(|| ocr_manifest_url_for_base(OCR_GITHUB_RELEASE_BASE_URL, mode))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_url_for_base(base_url: &str, mode: &str) -> String {
    format!("{base_url}ipaste-ocr-windows-x64-{mode}.json")
}

#[cfg(not(target_os = "macos"))]
fn ocr_r2_base_urls() -> Vec<String> {
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

#[cfg(not(target_os = "macos"))]
fn push_optional_base_url(base_urls: &mut Vec<String>, base_url: Option<String>) {
    if let Some(base_url) = base_url {
        push_unique_url(base_urls, base_url);
    }
}

#[cfg(not(target_os = "macos"))]
fn push_unique_url(urls: &mut Vec<String>, url: String) {
    if !urls.iter().any(|existing| existing == &url) {
        urls.push(url);
    }
}

#[cfg(not(target_os = "macos"))]
fn normalize_ocr_base_url(base_url: &str) -> Option<String> {
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

#[cfg(not(target_os = "macos"))]
fn derive_ocr_r2_base_url(endpoint: &str) -> Option<String> {
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
fn ocr_manifest_file_installed(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<bool, String> {
    if file.archive.as_deref() == Some("zip") {
        if file.entries.is_empty() {
            return Ok(false);
        }
        let install_dir = ocr_manifest_install_dir(app, file)?;
        return file
            .entries
            .iter()
            .map(|entry| install_dir.join(entry).exists())
            .try_fold(true, |all_exist, exists| Ok(all_exist && exists));
    }

    let target_path = ocr_manifest_file_path(app, file)?;
    file_is_valid(&target_path, &file.sha256)
}

#[cfg(not(target_os = "macos"))]
fn ocr_download_target_path(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    if file.archive.as_deref() == Some("zip") {
        return Ok(ocr_download_dir(app)?.join(&file.name));
    }

    ocr_manifest_file_path(app, file)
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_file_path(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    if file.role == "language" {
        return Ok(ocr_asset_dir(app)?.join(&file.name));
    }
    Ok(ocr_manifest_install_dir(app, file)?.join(&file.name))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_install_dir(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    let root = ocr_root_dir(app)?;
    let install_dir = file
        .install_dir
        .as_deref()
        .unwrap_or(if file.role == "engine" {
            OCR_ENGINE_DIR
        } else {
            OCR_ASSET_DIR
        });
    validate_relative_path(install_dir)?;
    let resolved = root.join(install_dir);
    ensure_path_within(&root, &resolved)?;
    Ok(resolved)
}

#[cfg(not(target_os = "macos"))]
fn install_ocr_zip_archive(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
    archive_path: &Path,
) -> Result<(), String> {
    let install_dir = ocr_manifest_install_dir(app, file)?;
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&install_dir).map_err(|error| error.to_string())?;

    let archive_file = fs::File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(archive_file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut zipped_file = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(enclosed_name) = zipped_file.enclosed_name().map(PathBuf::from) else {
            return Err("OCR portable zip 包含不安全路径".to_string());
        };
        let output_path = install_dir.join(enclosed_name);
        ensure_path_within(&install_dir, &output_path)?;

        if zipped_file.is_dir() {
            fs::create_dir_all(&output_path).map_err(|error| error.to_string())?;
            continue;
        }

        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = fs::File::create(&output_path).map_err(|error| error.to_string())?;
        std::io::copy(&mut zipped_file, &mut output).map_err(|error| error.to_string())?;
    }

    if !ocr_manifest_file_installed(app, file)? {
        return Err(format!("{} 解压后文件不完整", file.name));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn ensure_path_within(root: &Path, path: &Path) -> Result<(), String> {
    let root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
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
                fs::create_dir_all(parent)
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::Other, error))?;
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

#[cfg(not(target_os = "macos"))]
fn file_is_valid(path: &PathBuf, expected_sha256: &str) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let hash = file_sha256(path)?;
    Ok(hash.eq_ignore_ascii_case(expected_sha256))
}

#[cfg(not(target_os = "macos"))]
fn read_ocr_manifest_cache(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<Option<OcrManifest>, String> {
    let path = ocr_manifest_cache_path(app, mode)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str::<OcrManifest>(&content)
        .map(Some)
        .map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn write_ocr_manifest_cache(
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
pub(crate) fn ocr_root_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| error.to_string())
        .map(|path| path.join(OCR_DIR))
}

#[cfg(not(target_os = "macos"))]
fn ocr_asset_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join(OCR_ASSET_DIR))
}

#[cfg(not(target_os = "macos"))]
fn ocr_engine_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join(OCR_ENGINE_DIR))
}

#[cfg(not(target_os = "macos"))]
fn ocr_download_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join("downloads"))
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_cache_path(app: &tauri::AppHandle, mode: &str) -> Result<PathBuf, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    Ok(ocr_root_dir(app)?.join(format!("manifest-{mode}.json")))
}

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

#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_image_text_inner(
    app: &tauri::AppHandle,
    image_path: String,
) -> Result<ImageOcrResult, String> {
    let image_path = PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }

    let tesseract = find_tesseract_executable(app)?;
    let tessdata_dir = ocr_asset_dir(app)?;
    if !tessdata_dir.join("eng.traineddata").exists()
        || !tessdata_dir.join("chi_sim.traineddata").exists()
    {
        return Err("请先在偏好设置中下载图片 OCR 资源".to_string());
    }

    let output = Command::new(&tesseract)
        .arg(&image_path)
        .arg("stdout")
        .arg("-l")
        .arg("chi_sim+eng")
        .arg("--tessdata-dir")
        .arg(&tessdata_dir)
        .arg("-c")
        .arg("tessedit_create_tsv=1")
        .output()
        .map_err(|error| format!("无法启动 Tesseract：{error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Tesseract 识别失败".to_string()
        } else {
            stderr
        });
    }

    let tsv = String::from_utf8_lossy(&output.stdout);
    let words = parse_tesseract_tsv(&tsv);
    let text = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(ImageOcrResult {
        text,
        engine: tesseract.to_string_lossy().to_string(),
        language: "chi_sim+eng".to_string(),
        words,
    })
}

#[cfg(not(target_os = "macos"))]
fn find_tesseract_executable(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_tesseract = ocr_engine_dir(app)?.join("tesseract.exe");
    if app_data_tesseract.exists() {
        return Ok(app_data_tesseract);
    }

    Err("未找到 Tesseract 引擎。请先在偏好设置中下载图片 OCR 资源。".to_string())
}

#[cfg(not(target_os = "macos"))]
fn parse_tesseract_tsv(tsv: &str) -> Vec<ImageOcrWord> {
    tsv.lines()
        .skip(1)
        .filter_map(parse_tesseract_tsv_line)
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn parse_tesseract_tsv_line(line: &str) -> Option<ImageOcrWord> {
    let columns = line.split('\t').collect::<Vec<_>>();
    if columns.len() < 12 || columns.first()? != &"5" {
        return None;
    }

    let text = columns[11].trim();
    let confidence = columns[10].parse::<f64>().ok()?;
    if text.is_empty() || confidence < 0.0 {
        return None;
    }

    Some(ImageOcrWord {
        text: text.to_string(),
        left: parse_tsv_number(columns[6])?,
        top: parse_tsv_number(columns[7])?,
        width: parse_tsv_number(columns[8])?,
        height: parse_tsv_number(columns[9])?,
        confidence,
        block_index: columns[2].parse::<i64>().ok()?,
        paragraph_index: columns[3].parse::<i64>().ok()?,
        line_index: columns[4].parse::<i64>().ok()?,
        word_index: columns[5].parse::<i64>().ok()?,
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn recognize_image_text_macos(image_path: String) -> Result<ImageOcrResult, String> {
    let image_path = PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }

    let (image_width, image_height) = image::image_dimensions(&image_path)
        .map_err(|error| format!("无法读取图片尺寸：{error}"))?;
    if image_width == 0 || image_height == 0 {
        return Err("图片尺寸无效".to_string());
    }

    autoreleasepool(|_| recognize_image_text_macos_inner(&image_path, image_width, image_height))
}

#[cfg(target_os = "macos")]
fn recognize_image_text_macos_inner(
    image_path: &Path,
    image_width: u32,
    image_height: u32,
) -> Result<ImageOcrResult, String> {
    let url = NSURL::from_file_path(image_path).ok_or_else(|| "无法读取图片路径".to_string())?;
    let request_class = AnyClass::get(c"VNRecognizeTextRequest")
        .ok_or_else(|| "当前 macOS 不支持系统图片 OCR".to_string())?;
    let handler_class = AnyClass::get(c"VNImageRequestHandler")
        .ok_or_else(|| "当前 macOS 不支持系统图片 OCR".to_string())?;

    let request: Retained<AnyObject> = unsafe { msg_send![request_class, new] };
    configure_macos_text_request(&request);

    let handler_alloc: *mut AnyObject = unsafe { msg_send![handler_class, alloc] };
    let handler_raw: *mut AnyObject = unsafe {
        msg_send![
            handler_alloc,
            initWithURL: &*url,
            options: None::<&AnyObject>
        ]
    };
    let handler = unsafe { Retained::from_raw(handler_raw) }
        .ok_or_else(|| "无法初始化系统图片 OCR".to_string())?;
    let requests = NSArray::from_slice(&[&*request]);
    let mut error: Option<Retained<NSError>> = None;
    let performed: Bool = unsafe {
        msg_send![
            &*handler,
            performRequests: &*requests,
            error: &mut error
        ]
    };

    if !performed.as_bool() {
        return Err(error
            .map(|error| format!("系统图片 OCR 识别失败：{error}"))
            .unwrap_or_else(|| "系统图片 OCR 识别失败".to_string()));
    }

    let observations: Option<Retained<NSArray<AnyObject>>> =
        unsafe { msg_send![&*request, results] };
    let Some(observations) = observations else {
        return Ok(ImageOcrResult {
            text: String::new(),
            engine: MACOS_OCR_ENGINE_ID.to_string(),
            language: MACOS_OCR_LANGUAGE.to_string(),
            words: Vec::new(),
        });
    };

    let mut words = Vec::new();
    let mut lines = Vec::new();
    let observation_count = observations.count();
    for observation_index in 0..observation_count {
        let observation = observations.objectAtIndex(observation_index);
        let candidates = macos_top_text_candidates(&observation, 1);
        let Some(candidate) =
            candidates.and_then(|items| (items.count() > 0).then(|| items.objectAtIndex(0)))
        else {
            continue;
        };

        let line_text = macos_recognized_text_string(&candidate);
        if line_text.trim().is_empty() {
            continue;
        }
        lines.push(line_text.clone());

        let line_confidence = macos_recognized_text_confidence(&candidate) as f64 * 100.0;
        let tokens = macos_ocr_tokens(&line_text);
        if tokens.is_empty() {
            if let Some(bounding_box) = macos_recognized_text_bounding_box(
                &candidate,
                NSRange::new(0, candidate_string_utf16_len(&candidate)),
            ) {
                words.push(macos_ocr_word_from_bounding_box(
                    line_text.trim().to_string(),
                    bounding_box,
                    image_width,
                    image_height,
                    line_confidence,
                    observation_index as i64,
                    0,
                    observation_index as i64,
                    0,
                ));
            }
            continue;
        }

        for (word_index, token) in tokens.into_iter().enumerate() {
            let bounding_box =
                macos_recognized_text_bounding_box(&candidate, token.range).or_else(|| {
                    macos_recognized_text_bounding_box(
                        &candidate,
                        NSRange::new(0, candidate_string_utf16_len(&candidate)),
                    )
                });
            if let Some(bounding_box) = bounding_box {
                words.push(macos_ocr_word_from_bounding_box(
                    token.text,
                    bounding_box,
                    image_width,
                    image_height,
                    line_confidence,
                    observation_index as i64,
                    0,
                    observation_index as i64,
                    word_index as i64,
                ));
            }
        }
    }

    Ok(ImageOcrResult {
        text: lines.join("\n"),
        engine: MACOS_OCR_ENGINE_ID.to_string(),
        language: MACOS_OCR_LANGUAGE.to_string(),
        words,
    })
}

#[cfg(target_os = "macos")]
fn configure_macos_text_request(request: &AnyObject) {
    unsafe {
        let _: () = msg_send![
            request,
            setRecognitionLevel: MACOS_OCR_RECOGNITION_LEVEL_ACCURATE
        ];
        let _: () = msg_send![request, setUsesLanguageCorrection: Bool::YES];
        let supports_languages: Bool =
            msg_send![request, respondsToSelector: sel!(setRecognitionLanguages:)];
        if supports_languages.as_bool() {
            let zh = NSString::from_str("zh-Hans");
            let en = NSString::from_str("en-US");
            let languages = NSArray::from_slice(&[&*zh, &*en]);
            let _: () = msg_send![request, setRecognitionLanguages: &*languages];
        }
        let supports_language_detection: Bool =
            msg_send![request, respondsToSelector: sel!(setAutomaticallyDetectsLanguage:)];
        if supports_language_detection.as_bool() {
            let _: () = msg_send![request, setAutomaticallyDetectsLanguage: Bool::YES];
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_top_text_candidates(
    observation: &AnyObject,
    max_candidates: NSUInteger,
) -> Option<Retained<NSArray<AnyObject>>> {
    unsafe { msg_send![observation, topCandidates: max_candidates] }
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_string(candidate: &AnyObject) -> String {
    let value: Retained<NSString> = unsafe { msg_send![candidate, string] };
    value.to_string()
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_confidence(candidate: &AnyObject) -> f32 {
    unsafe { msg_send![candidate, confidence] }
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_bounding_box(candidate: &AnyObject, range: NSRange) -> Option<CGRect> {
    if range.length == 0 {
        return None;
    }

    let mut error: Option<Retained<NSError>> = None;
    let box_observation: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            candidate,
            boundingBoxForRange: range,
            error: &mut error
        ]
    };
    let box_observation = box_observation?;
    let bounding_box: CGRect = unsafe { msg_send![&*box_observation, boundingBox] };

    if error.is_some() || bounding_box.size.width <= 0.0 || bounding_box.size.height <= 0.0 {
        None
    } else {
        Some(bounding_box)
    }
}

#[cfg(target_os = "macos")]
fn macos_ocr_word_from_bounding_box(
    text: String,
    bounding_box: CGRect,
    image_width: u32,
    image_height: u32,
    confidence: f64,
    block_index: i64,
    paragraph_index: i64,
    line_index: i64,
    word_index: i64,
) -> ImageOcrWord {
    let image_width = image_width as f64;
    let image_height = image_height as f64;
    let left = bounding_box.origin.x * image_width;
    let top = (1.0 - bounding_box.origin.y - bounding_box.size.height) * image_height;
    let width = bounding_box.size.width * image_width;
    let height = bounding_box.size.height * image_height;

    ImageOcrWord {
        text,
        left: left.max(0.0),
        top: top.max(0.0),
        width: width.max(1.0),
        height: height.max(1.0),
        confidence,
        block_index,
        paragraph_index,
        line_index,
        word_index,
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct MacOcrToken {
    text: String,
    range: NSRange,
}

#[cfg(target_os = "macos")]
fn macos_ocr_tokens(text: &str) -> Vec<MacOcrToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_start = 0_usize;
    let mut utf16_offset = 0_usize;
    let mut current_is_cjk = false;

    for char in text.chars() {
        let char_len = char.len_utf16();
        if char.is_whitespace() {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            utf16_offset += char_len;
            current_is_cjk = false;
            continue;
        }

        let is_cjk = is_cjk_char(char);
        if current.is_empty() {
            current_start = utf16_offset;
            current_is_cjk = is_cjk;
        } else if is_cjk || current_is_cjk {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            current_start = utf16_offset;
            current_is_cjk = is_cjk;
        }

        current.push(char);
        utf16_offset += char_len;

        if is_cjk {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            current_is_cjk = false;
        }
    }

    push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
    tokens
}

#[cfg(target_os = "macos")]
fn push_macos_ocr_token(
    tokens: &mut Vec<MacOcrToken>,
    current: &mut String,
    start: usize,
    end: usize,
) {
    let value = current.trim();
    if !value.is_empty() && end > start {
        tokens.push(MacOcrToken {
            text: value.to_string(),
            range: NSRange::new(start, end - start),
        });
    }
    current.clear();
}

#[cfg(target_os = "macos")]
fn candidate_string_utf16_len(candidate: &AnyObject) -> usize {
    let value: Retained<NSString> = unsafe { msg_send![candidate, string] };
    value.length()
}

#[cfg(target_os = "macos")]
fn is_cjk_char(char: char) -> bool {
    matches!(
        char as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

#[cfg(not(target_os = "macos"))]
fn parse_tsv_number(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}
