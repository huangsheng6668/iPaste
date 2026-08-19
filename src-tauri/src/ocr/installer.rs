use std::path::{Path, PathBuf};
use std::{fs, time::Duration};
use std::io::{Read, Write};

use reqwest::blocking::Client;
use tauri::Manager;

use super::{emit_ocr_install_progress, ocr_platform};
use crate::models::{OcrInstallStatus, OcrManifest, OcrManifestFile};
use crate::util::{clean_ocr_mode, file_sha256, validate_relative_path};

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
/// v2 安装器引擎标识：manifest.engine.id 与缓存失效判定均以此为基准。
#[cfg(not(target_os = "macos"))]
pub(crate) const OCR_ENGINE_ID: &str = "paddle";
/// 单个 OCR 模式的模型文件布局（app-data 下相对 ocr 根目录）。
/// fast/best 各自独立目录，切模式即重新下载。
#[cfg(not(target_os = "macos"))]
pub(crate) const OCR_MODEL_DIR: &str = "paddle"; // → ocr/paddle/{mode}/
#[cfg(not(target_os = "macos"))]
const OCR_CHARSET_FILE: &str = "ppocr_keys_v5.txt";
// Task 0 实测的默认体积兜底（manifest 拉取后以 manifest 为权威）
#[cfg(not(target_os = "macos"))]
const OCR_FAST_TOTAL_BYTES: u64 = 10_903_450;
#[cfg(not(target_os = "macos"))]
const OCR_BEST_TOTAL_BYTES: u64 = 21_384_230;

#[cfg(not(target_os = "macos"))]
pub(crate) fn install_ocr_assets_inner(
    app: &tauri::AppHandle,
    mode: &str,
) -> Result<OcrInstallStatus, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    emit_ocr_install_progress(app, "fetchingManifest", None, 0, 0);
    let manifest = fetch_ocr_manifest(&mode)?;

    // 进入下载循环前清理旧版残留（v1 安装器的引擎目录与下载/资产目录）；
    // 清理失败不阻塞 v2 安装，模型文件按 file.path 落入独立的 ocr/paddle/{mode}/ 目录
    for legacy_path in legacy_ocr_paths(app) {
        if legacy_path.exists() {
            let _ = fs::remove_dir_all(&legacy_path);
        }
    }

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
        let target_path = ocr_manifest_file_path(app, file)?;
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
    // v2 模型均为裸文件（validator 已拒绝压缩包），落点即 file.path 描述的布局
    let target_path = ocr_manifest_file_path(app, file)?;
    file_is_valid(&target_path, &file.sha256)
}

#[cfg(not(target_os = "macos"))]
fn ocr_manifest_file_path(
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
fn ocr_manifest_cache_path(app: &tauri::AppHandle, mode: &str) -> Result<PathBuf, String> {
    let mode = clean_ocr_mode(mode.to_string())?;
    Ok(ocr_root_dir(app)?.join(format!("manifest-{mode}.json")))
}

/// 缓存清单可用性判定：引擎 id 不是当前 paddle 引擎的缓存一律作废。
#[cfg(not(target_os = "macos"))]
fn is_usable_cached_manifest(manifest: &OcrManifest) -> bool {
    manifest.engine.id == OCR_ENGINE_ID
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{OcrManifest, OcrManifestEngine, OcrManifestFile};

    fn manifest_for(engine_id: &str, role: &str) -> OcrManifest {
        OcrManifest {
            engine: OcrManifestEngine {
                id: engine_id.to_string(),
                version: "1".to_string(),
                platform: "windows-x64".to_string(),
                mode: None,
                base_url: "https://example.com/".to_string(),
                files: vec![OcrManifestFile {
                    role: role.to_string(),
                    name: "det.mnn".to_string(),
                    path: "paddle/fast/det.mnn".to_string(),
                    size: 1,
                    sha256: "00".repeat(32),
                    archive: None,
                    install_dir: None,
                    entries: Vec::new(),
                }],
            },
        }
    }

    #[test]
    fn validate_accepts_paddle_model_manifest() {
        assert!(validate_ocr_manifest(&manifest_for("paddle", "det-model"), "fast").is_ok());
    }

    #[test]
    fn validate_rejects_legacy_engine_manifest() {
        // 老清单（非当前引擎 id）直接拒绝，防止 R2 上 v1 URL 误配
        assert!(validate_ocr_manifest(&manifest_for("v1-engine", "det-model"), "fast").is_err());
    }

    #[test]
    fn validate_rejects_unknown_role() {
        assert!(validate_ocr_manifest(&manifest_for("paddle", "engine"), "fast").is_err());
    }

    #[test]
    fn validate_rejects_zip_role_for_models() {
        let mut m = manifest_for("paddle", "det-model");
        m.engine.files[0].archive = Some("zip".to_string());
        assert!(validate_ocr_manifest(&m, "fast").is_err());
    }

    #[test]
    fn cached_manifest_with_wrong_engine_is_ignored() {
        // 缓存失效判定抽成纯函数后测试：
        // fn is_usable_cached_manifest(m: &OcrManifest) -> bool { m.engine.id == OCR_ENGINE_ID }
        assert!(!is_usable_cached_manifest(&manifest_for("v1-engine", "det-model")));
        assert!(is_usable_cached_manifest(&manifest_for("paddle", "det-model")));
    }

    /// 测试用临时根目录：ensure_path_within 会按需创建目录，不能用 "/ocr"
    /// 这类宿主机盘符根路径（会在仓库外留垃圾，且依赖盘符根目录可写）。
    /// 每个测试独立 tag，避免并行测试互相清理对方目录。
    fn temp_test_root(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ipaste-installer-test-{tag}-{}", std::process::id()))
    }

    #[test]
    fn paddle_model_paths_are_mode_scoped() {
        // 用 tauri::test 拿不到 AppHandle 时，测纯路径函数：
        // fn paddle_model_paths_under(root: &Path, mode: &str) -> Result<PaddleModelPaths, String>
        let root = temp_test_root("scoped");
        let paths = paddle_model_paths_under(&root, "fast").expect("fast 模式路径合法");
        assert!(paths.det.ends_with("paddle/fast/det.mnn"));
        assert!(paths.rec.ends_with("paddle/fast/rec.mnn"));
        assert!(paths.charset.ends_with("paddle/fast/ppocr_keys_v5.txt"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn paddle_model_paths_rejects_unsafe_mode() {
        // clean_ocr_mode 在任何文件系统调用前即拒绝非法 mode
        let root = temp_test_root("unsafe");
        assert!(paddle_model_paths_under(&root, "../evil").is_err());
        let _ = fs::remove_dir_all(&root);
    }
}
