//! Manga-OCR (mocr) 模型安装器：manifest 驱动下载 manga-ocr-base 权重到
//! `app_data/ocr/mocr/models/`（mocr.rs find_mocr_model_path 的最高优先级路径）。
//!
//! 支持 Windows 与 macOS（aarch64 提供内置 onnx sidecar；Intel Mac 可下载模型但识别走系统 OCR / Python 回退）。
//! 与 paddle 安装器（installer.rs）同构：R2 优先 + GitHub Release 兜底的清单源、
//! Pages 文件源、sha256 门禁的临时文件落盘。差异点：
//! - 无 fast/best 模式概念，清单/缓存文件名固定 mocr；
//! - 状态检查只做「存在 + 字节数一致」（主权重约 440MB，全量哈希会卡设置页）；
//!   完整性在下载完成时以 sha256 门禁保证；
//! - 所有清单/下载 URL 校验 host：仅 http/https，拒绝 localhost/环回/私有/保留地址。

use std::io::{Read, Write};
use std::path::PathBuf;
use std::{fs, time::Duration};

use reqwest::blocking::Client;
use tauri::Emitter;

use super::installer::{ensure_path_within, ocr_r2_base_urls, ocr_root_dir};
use crate::models::{OcrInstallStatus, OcrManifest, OcrManifestFile};
use crate::util::{file_sha256, validate_relative_path};

pub(crate) const MOCR_ENGINE_ID: &str = "mocr";
/// GitHub Release 兜底清单源（与 scripts/ocr-models README §5 发布流程一致）。
const MOCR_GITHUB_RELEASE_BASE_URL: &str =
    "https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-mocr-v1/";
/// 未取到清单时的体积兜底（onnx 三件套实测总量，清单为权威值）。
const MOCR_DEFAULT_TOTAL_BYTES: u64 = 460_790_482;
/// 模型落点（相对 ocr 根目录）：ocr/mocr/models/。
const MOCR_MODEL_SUBDIR: &str = "mocr/models";

/// 仅允许 http/https，且拒绝 localhost、环回、私有、保留地址（含 IPv6 等价物）。
/// 清单 base_url 与逐文件下载 URL 均须过此校验，防止被篡改的清单把请求导向内网。
pub(crate) fn ensure_public_http_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| format!("非法 URL：{error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("仅允许 http/https 链接：{url}"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("URL 缺少 host：{url}"))?;
    if host.eq_ignore_ascii_case("localhost") {
        return Err(format!("禁止访问 localhost：{url}"));
    }
    // IPv6 字面量在 host_str() 中带方括号（如 "[::1]"），剥离后再按 IP 解析；
    // 解析失败即视为域名放行
    let bare_host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = bare_host.parse::<std::net::IpAddr>() {
        if ip.is_loopback() || ip.is_unspecified() || is_private_or_reserved(ip) {
            return Err(format!("禁止访问内网/保留地址：{url}"));
        }
    }
    Ok(())
}

fn is_private_or_reserved(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_private() || v4.is_link_local() || v4.is_broadcast() || v4.is_documentation()
        }
        std::net::IpAddr::V6(v6) => v6.is_loopback() || v6.is_unspecified() || is_unique_local(v6),
    }
}

fn is_unique_local(v6: std::net::Ipv6Addr) -> bool {
    (v6.segments()[0] & 0xfe00) == 0xfc00
}

fn mocr_manifest_urls() -> Vec<String> {
    let mut urls = Vec::new();
    for base in ocr_r2_base_urls() {
        let url = format!("{base}ipaste-ocr-mocr-v1.json");
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls.push(format!("{MOCR_GITHUB_RELEASE_BASE_URL}ipaste-ocr-mocr-v1.json"));
    urls
}

fn mocr_primary_manifest_url() -> String {
    mocr_manifest_urls().remove(0)
}

fn mocr_manifest_cache_path(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(ocr_root_dir(app)?.join("manifest-mocr.json"))
}

fn read_mocr_manifest_cache(app: &tauri::AppHandle) -> Result<Option<OcrManifest>, String> {
    let path = mocr_manifest_cache_path(app)?;
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let manifest = serde_json::from_str::<OcrManifest>(&content).map_err(|error| error.to_string())?;
    if manifest.engine.id != MOCR_ENGINE_ID {
        return Ok(None);
    }
    Ok(Some(manifest))
}

fn write_mocr_manifest_cache(
    app: &tauri::AppHandle,
    manifest: &OcrManifest,
) -> Result<(), String> {
    let path = mocr_manifest_cache_path(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let content = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(path, content).map_err(|error| error.to_string())
}

fn emit_mocr_progress(
    app: &tauri::AppHandle,
    phase: &str,
    file_name: Option<String>,
    downloaded_bytes: u64,
    total_bytes: u64,
) {
    let _ = app.emit(
        crate::events::EVENT_MOCR_INSTALL_PROGRESS,
        crate::models::OcrInstallProgress {
            phase: phase.to_string(),
            file_name,
            downloaded_bytes,
            total_bytes,
        },
    );
}

fn validate_mocr_manifest(manifest: &OcrManifest) -> Result<(), String> {
    if manifest.engine.id != MOCR_ENGINE_ID {
        return Err("Manga-OCR manifest 引擎不受支持".to_string());
    }
    if manifest.engine.files.is_empty() {
        return Err("Manga-OCR manifest 文件列表为空".to_string());
    }
    ensure_public_http_url(&manifest.engine.base_url)?;
    for file in &manifest.engine.files {
        validate_relative_path(&file.path)?;
        if let Some(url) = &file.url {
            ensure_public_http_url(url)?;
        }
    }
    Ok(())
}

fn mocr_manifest_file_path(
    app: &tauri::AppHandle,
    file: &OcrManifestFile,
) -> Result<PathBuf, String> {
    let root = ocr_root_dir(app)?;
    validate_relative_path(&file.path)?;
    let resolved = root.join(&file.path);
    ensure_path_within(&root, &resolved)?;
    Ok(resolved)
}

/// 状态检查：存在 + 字节数一致（主权重约 440MB，不做全量哈希）。
fn mocr_file_present(path: &std::path::Path, expected_size: u64) -> bool {
    path.is_file()
        && fs::metadata(path)
            .map(|meta| meta.len() == expected_size)
            .unwrap_or(false)
}

fn mocr_install_status_for_manifest(
    app: &tauri::AppHandle,
    manifest: &OcrManifest,
) -> Result<OcrInstallStatus, String> {
    let install_dir = ocr_root_dir(app)?;
    let mut downloaded_bytes = 0_u64;
    let mut missing_files = Vec::new();

    for file in &manifest.engine.files {
        let path = mocr_manifest_file_path(app, file)?;
        if mocr_file_present(&path, file.size) {
            downloaded_bytes = downloaded_bytes.saturating_add(file.size);
        } else {
            missing_files.push(file.name.clone());
        }
    }

    let total_bytes = manifest.engine.files.iter().map(|file| file.size).sum();
    Ok(OcrInstallStatus {
        installed: missing_files.is_empty(),
        engine_id: manifest.engine.id.clone(),
        engine_version: Some(manifest.engine.version.clone()),
        mode: MOCR_ENGINE_ID.to_string(),
        platform: manifest.engine.platform.clone(),
        manifest_url: mocr_primary_manifest_url(),
        install_dir: install_dir
            .join(MOCR_MODEL_SUBDIR)
            .to_string_lossy()
            .to_string(),
        downloaded_bytes,
        total_bytes,
        missing_files,
    })
}

fn mocr_install_status_unavailable(app: &tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    Ok(OcrInstallStatus {
        installed: false,
        engine_id: MOCR_ENGINE_ID.to_string(),
        engine_version: None,
        mode: MOCR_ENGINE_ID.to_string(),
        platform: super::ocr_platform().to_string(),
        manifest_url: mocr_primary_manifest_url(),
        install_dir: ocr_root_dir(app)?
            .join(MOCR_MODEL_SUBDIR)
            .to_string_lossy()
            .to_string(),
        downloaded_bytes: 0,
        total_bytes: MOCR_DEFAULT_TOTAL_BYTES,
        missing_files: Vec::new(),
    })
}

pub(crate) fn mocr_install_status(app: &tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    match read_mocr_manifest_cache(app)? {
        Some(manifest) => mocr_install_status_for_manifest(app, &manifest),
        None => mocr_install_status_unavailable(app),
    }
}

fn fetch_mocr_manifest() -> Result<OcrManifest, String> {
    let client = Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|error| error.to_string())?;
    let mut errors = Vec::new();

    for manifest_url in mocr_manifest_urls() {
        if let Err(error) = ensure_public_http_url(&manifest_url) {
            errors.push(format!("{manifest_url}：{error}"));
            continue;
        }
        let response = match client.get(&manifest_url).send() {
            Ok(response) => response,
            Err(error) => {
                errors.push(format!("{manifest_url}：{error}"));
                continue;
            }
        };
        if !response.status().is_success() {
            errors.push(format!("{manifest_url}：HTTP {}", response.status().as_u16()));
            continue;
        }
        match response.json::<OcrManifest>() {
            Ok(manifest) => {
                validate_mocr_manifest(&manifest)?;
                return Ok(manifest);
            }
            Err(error) => errors.push(format!("{manifest_url}：无法解析 manifest：{error}")),
        }
    }

    Err(format!("无法获取 Manga-OCR 资源信息：{}", errors.join("；")))
}

pub(crate) fn install_mocr_assets_inner(app: &tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    emit_mocr_progress(app, "fetchingManifest", None, 0, 0);
    let manifest = fetch_mocr_manifest()?;

    let total_bytes = manifest.engine.files.iter().map(|file| file.size).sum();
    let mut downloaded_bytes = 0_u64;
    emit_mocr_progress(app, "downloading", None, downloaded_bytes, total_bytes);

    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .map_err(|error| error.to_string())?;

    for file in &manifest.engine.files {
        let target_path = mocr_manifest_file_path(app, file)?;
        if mocr_file_present(&target_path, file.size) {
            downloaded_bytes = downloaded_bytes.saturating_add(file.size);
            emit_mocr_progress(
                app,
                "downloading",
                Some(file.name.clone()),
                downloaded_bytes.min(total_bytes),
                total_bytes,
            );
            continue;
        }

        // 大文件（如 445MB 权重）由清单的 url 覆盖直指 Release 扁平资产；
        // 其余按 Pages 相对路径拼接
        let url = match &file.url {
            Some(url) => url.clone(),
            None => format!("{}{}", manifest.engine.base_url, file.path),
        };
        ensure_public_http_url(&url)?;
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
            let read = response.read(&mut buffer).map_err(|error| error.to_string())?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).map_err(|error| error.to_string())?;
            file_bytes = file_bytes.saturating_add(read as u64);
            emit_mocr_progress(
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
        downloaded_bytes = file_start_bytes.saturating_add(file.size);
    }

    write_mocr_manifest_cache(app, &manifest)?;
    let status = mocr_install_status_for_manifest(app, &manifest)?;
    emit_mocr_progress(
        app,
        "completed",
        None,
        status.downloaded_bytes,
        status.total_bytes,
    );
    Ok(status)
}

/// 删除 mocr 资产。调用方须先结束常驻推理进程（Windows 下模型文件被占用无法删除），
/// 本函数再兜底等价清理（同步等待 kill 完成）。
pub(crate) fn remove_mocr_assets_inner(app: &tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    let root = ocr_root_dir(app)?;
    let mocr_dir = root.join("mocr");
    if mocr_dir.exists() {
        fs::remove_dir_all(&mocr_dir).map_err(|error| error.to_string())?;
    }
    let cache = mocr_manifest_cache_path(app)?;
    if cache.exists() {
        let _ = fs::remove_file(&cache);
    }
    mocr_install_status_unavailable(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_url_validation_rejects_private_hosts() {
        assert!(ensure_public_http_url("https://huangsheng6668.github.io/iPaste/ocr/").is_ok());
        assert!(ensure_public_http_url("http://example.com/file.bin").is_ok());

        for url in [
            "https://localhost/file",
            "https://127.0.0.1/file",
            "http://10.0.0.5/file",
            "http://192.168.1.4/file",
            "http://172.16.0.9/file",
            "http://169.254.1.1/file",
            "http://[::1]/file",
            "http://[fc00::1]/file",
            "http://0.0.0.0/file",
            "ftp://example.com/file",
            "file:///C:/windows/system32/config",
        ] {
            assert!(ensure_public_http_url(url).is_err(), "应拒绝：{url}");
        }
    }

    #[test]
    fn url_without_host_is_rejected() {
        // 真实无 host / 非法 URL 必须被拒。
        // 注：WHATWG 规范下 "https:///no-host" 会解析为 host="no-host"、path="/"
        //（第三斜杠被吸收），属合法公网域名，应放行而非拒绝。
        assert!(ensure_public_http_url("not a url").is_err());
        assert!(ensure_public_http_url("").is_err());
        assert!(ensure_public_http_url("https:///no-host").is_ok());
    }

    #[test]
    fn mocr_manifest_urls_prefer_r2_then_github() {
        let urls = mocr_manifest_urls();
        assert!(urls.iter().all(|url| url.ends_with("ipaste-ocr-mocr-v1.json")));
        assert_eq!(urls.last().unwrap(), "https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-mocr-v1/ipaste-ocr-mocr-v1.json");
    }

    #[test]
    fn mocr_manifest_validation_requires_mocr_engine_and_safe_paths() {
        let manifest = |id: &str, path: &str| OcrManifest {            engine: crate::models::OcrManifestEngine {
                id: id.to_string(),
                version: "1.0.0".to_string(),
                platform: "any".to_string(),
                mode: None,
                base_url: "https://huangsheng6668.github.io/iPaste/ocr/".to_string(),
                files: vec![OcrManifestFile {
                    role: "model".to_string(),
                    name: "config.json".to_string(),
                    path: path.to_string(),
                    size: 1,
                    sha256: "00".repeat(32),
                    url: None,
                    archive: None,
                    install_dir: None,
                    entries: Vec::new(),
                }],
            },
        };

        assert!(validate_mocr_manifest(&manifest("mocr", "mocr/models/config.json")).is_ok());
        assert!(validate_mocr_manifest(&manifest("paddle", "mocr/models/config.json")).is_err());
        assert!(validate_mocr_manifest(&manifest("mocr", "../escape.json")).is_err());

        // url 覆盖必须同样是公网 http/https（防清单被篡改导流内网）
        let mut with_url = manifest("mocr", "mocr/models/pytorch_model.bin");
        with_url.engine.files[0].url =
            Some("https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-mocr-v1/mocr-models-pytorch_model.bin".to_string());
        assert!(validate_mocr_manifest(&with_url).is_ok());
        with_url.engine.files[0].url = Some("http://192.168.0.1/weights.bin".to_string());
        assert!(validate_mocr_manifest(&with_url).is_err());
    }
}
