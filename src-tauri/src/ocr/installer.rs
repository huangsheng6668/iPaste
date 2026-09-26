#![allow(unused_imports)]

use std::path::{Path, PathBuf};
use std::{fs, time::Duration};
use std::io::{Read, Write};

use reqwest::blocking::Client;
use tauri::Manager;

use super::{emit_ocr_install_progress, ocr_platform};
use crate::models::{OcrInstallStatus, OcrManifest, OcrManifestFile};
use crate::util::{clean_ocr_mode, file_sha256, validate_relative_path};

#[cfg(not(target_os = "macos"))]
/// GitHub Release 兜底清单源：仓库必须是实际发版仓库（与 tauri.conf.json
/// 的 updater endpoint 同源），tag 必须与 scripts/ocr-models/README.md §4
/// 发布流程创建的 `ipaste-ocr-windows-v2` 一致（v1 tag 下是旧 tesseract
/// 清单，validate_ocr_manifest 会拒绝）。
const OCR_GITHUB_RELEASE_BASE_URL: &str =
    "https://github.com/huangsheng6668/iPaste/releases/download/ipaste-ocr-windows-v2/";
const OCR_R2_BASE_URL: &str = env!("IPASTE_OCR_R2_BASE_URL");
const UPDATER_R2_ENDPOINT: &str = env!("IPASTE_UPDATER_R2_ENDPOINT");
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
const OCR_FAST_TOTAL_BYTES: u64 = 10_885_068;
#[cfg(not(target_os = "macos"))]
const OCR_BEST_TOTAL_BYTES: u64 = 21_365_848;

mod manifest;
mod paths;

// 保持原 `ocr::installer::xxx` 调用路径不变（显式再导出，不做 glob 再导出）。
pub(crate) use manifest::{ocr_install_status, ocr_r2_base_urls};
pub(crate) use paths::{
    ensure_path_within, legacy_ocr_paths, ocr_root_dir, paddle_model_paths,
    paddle_model_paths_under,
};

#[cfg(not(target_os = "macos"))]
use manifest::{
    fetch_ocr_manifest, is_usable_cached_manifest, manifest_total_bytes,
    ocr_install_status_for_manifest, ocr_manifest_file_installed, ocr_manifest_file_path,
    validate_ocr_manifest, write_ocr_manifest_cache,
};

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


#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    use super::*;
    use crate::models::{OcrManifest, OcrManifestEngine, OcrManifestFile};

    fn manifest_for(engine_id: &str, role: &str) -> OcrManifest {
        OcrManifest {
            engine: OcrManifestEngine {
                id: engine_id.to_string(),
                version: "1".to_string(),
                // validate_ocr_manifest 与 ocr_platform() 比对，测试必须用当前平台值，
                // 否则在 Linux CI 上因 "unsupported" != "windows-x64" 失败
                platform: super::ocr_platform().to_string(),
                mode: None,
                base_url: "https://example.com/".to_string(),
                files: vec![OcrManifestFile {
                    role: role.to_string(),
                    name: "det.mnn".to_string(),
                    path: "paddle/fast/det.mnn".to_string(),
                    size: 1,
                    sha256: "00".repeat(32),
                    url: None,
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
