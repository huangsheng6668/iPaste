//! 图片 OCR：状态检测与调度（mod）+ Windows 资源安装器（installer）+
//! Windows Paddle 识别管线（paddle）+
//! macOS Vision 管线（vision）+ 跨平台行内分词（tokens）+
//! 通用 OpenAI 兼容云 OCR 客户端（openai）。

use tauri::Emitter;

use crate::events::EVENT_OCR_INSTALL_PROGRESS;
use crate::models::{ImageOcrResult, OcrInstallProgress, OcrInstallStatus, OpenaiOcrProfile};
// try_state 等方法来自 Manager trait；云引擎调度分支在 macOS 上也调用，
// 导入不能按平台裁剪
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

/// 通用 OpenAI 兼容云 OCR（视觉模型识图，跨平台）。
pub(crate) mod openai;

/// Manga-OCR (mocr) 专用日漫推理桥接器。
pub(crate) mod mocr;
/// Manga-OCR 模型安装器（设置页「日语 · 漫画」模型下载）。
/// Windows 与 macOS（onnx sidecar 分发平台）；Intel Mac 亦可下载（引擎缺失
/// 时识别走回退）。复用的 installer.rs 基础函数已全平台化。
#[cfg(any(windows, target_os = "macos"))]
pub(crate) mod mocr_installer;
/// Manga-OCR 的 ONNX 本地推理引擎（无 Python 依赖，主路径）。
pub(crate) mod mocr_onnx;

fn ocr_platform() -> &'static str {
    // macOS 下本分支即函数尾表达式（其余分支被 cfg 裁掉），无需 return。
    #[cfg(target_os = "macos")]
    {
        "macos-system"
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
        let mode = store.settings()?.ocr_mode.as_str();
        installer::ocr_install_status(app, mode)
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
        let mode = store.settings()?.ocr_mode.as_str();
        tokio::task::spawn_blocking(move || installer::install_ocr_assets_inner(&app, mode))
            .await
            .map_err(|error| error.to_string())?
    }
}

pub(crate) fn remove_assets(
    app: &tauri::AppHandle,
    store: &crate::store::Store,
) -> Result<OcrInstallStatus, String> {
    clear_result_cache();
    #[cfg(target_os = "macos")]
    {
        let _ = (app, store);
        macos_ocr_install_status()
    }

    #[cfg(not(target_os = "macos"))]
    {
        paddle::clear_engine_cache();
        let mode = store.settings()?.ocr_mode.as_str();
        let root = installer::ocr_root_dir(app)?;
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
        }
        installer::ocr_install_status(app, mode)
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
        let _ = app;
        Err("Manga-OCR 模型下载不支持当前平台".to_string())
    }
}

/// 删除 mocr 模型：先结束常驻推理进程（Windows 下文件被占用删不掉），再清目录。
pub(crate) async fn remove_mocr_assets(app: tauri::AppHandle) -> Result<OcrInstallStatus, String> {
    clear_result_cache();
    mocr::shutdown_server().await;
    #[cfg(any(windows, target_os = "macos"))]
    {
        tokio::task::spawn_blocking(move || mocr_installer::remove_mocr_assets_inner(&app))
            .await
            .map_err(|error| error.to_string())?
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = app;
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

/// 主接口移到首位；其余保持设置中的列表顺序。只调整本次识别的副本，
/// 不改变用户保存的接口顺序与主接口选择。
#[cfg(test)]
fn ordered_openai_profiles(
    mut profiles: Vec<crate::models::OpenaiOcrProfile>,
    active_profile_id: &str,
) -> Vec<crate::models::OpenaiOcrProfile> {
    if let Some(index) = profiles
        .iter()
        .position(|profile| profile.id == active_profile_id)
    {
        let active = profiles.remove(index);
        profiles.insert(0, active);
    }
    profiles
}

pub(crate) const OCR_CACHE_CAPACITY: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct OcrCacheKey {
    pub(crate) image_key: String,
    pub(crate) engine_key: String,
    pub(crate) profile: String,
    pub(crate) language: String,
}

pub(crate) struct OcrResultCache {
    capacity: usize,
    entries: std::collections::VecDeque<(OcrCacheKey, ImageOcrResult)>,
}

impl OcrResultCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: std::collections::VecDeque::with_capacity(capacity),
        }
    }

    pub(crate) fn get(&mut self, key: &OcrCacheKey) -> Option<ImageOcrResult> {
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, v) = self.entries.remove(pos)?;
        let cloned = v.clone();
        self.entries.push_back((k, v));
        Some(cloned)
    }

    pub(crate) fn insert(&mut self, key: OcrCacheKey, result: ImageOcrResult) {
        if result.text.trim().is_empty() || self.capacity == 0 {
            return;
        }
        if let Some(pos) = self.entries.iter().position(|(k, _)| k == &key) {
            self.entries.remove(pos);
        } else if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((key, result));
    }

    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }
}

static OCR_RESULT_CACHE: std::sync::OnceLock<std::sync::Mutex<OcrResultCache>> =
    std::sync::OnceLock::new();

fn result_cache() -> &'static std::sync::Mutex<OcrResultCache> {
    OCR_RESULT_CACHE.get_or_init(|| std::sync::Mutex::new(OcrResultCache::new(OCR_CACHE_CAPACITY)))
}

fn clear_result_cache() {
    if let Ok(mut guard) = result_cache().lock() {
        guard.clear();
    }
}

fn lookup_cached_result(key: &OcrCacheKey) -> Option<ImageOcrResult> {
    result_cache().lock().ok()?.get(key)
}

fn store_cached_result(key: OcrCacheKey, result: &ImageOcrResult) {
    if let Ok(mut guard) = result_cache().lock() {
        guard.insert(key, result.clone());
    }
}

fn build_image_cache_key(image_path: &str) -> String {
    match std::fs::metadata(image_path) {
        Ok(meta) => {
            let len = meta.len();
            let modified_ns = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("{image_path}:{len}:{modified_ns}")
        }
        Err(_) => image_path.to_string(),
    }
}

fn build_openai_engine_cache_key(
    active_profile_id: &str,
    profiles: &[OpenaiOcrProfile],
    prompts: &[crate::models::CloudOcrPromptMessage],
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(active_profile_id.as_bytes());
    hasher.update(b"|");
    for p in profiles {
        hasher.update(p.id.as_bytes());
        hasher.update(b":");
        hasher.update(p.base_url.trim().as_bytes());
        hasher.update(b":");
        hasher.update(p.model.trim().as_bytes());
        hasher.update(b";");
    }
    for msg in prompts {
        hasher.update(msg.role.as_bytes());
        hasher.update(b":");
        hasher.update(msg.content.as_bytes());
        hasher.update(b";");
    }
    let digest = hasher.finalize();
    format!("openai:{digest:x}")
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
    let image_key = build_image_cache_key(&image_path);
    let profile_key = if is_manga {
        "manga".to_string()
    } else {
        "default".to_string()
    };
    let lang_key = language.clone().unwrap_or_else(|| "auto".to_string());

    // 云引擎分支：显式选择的引擎优先（manga 档位映射为日语提示，不走本地 mocr）。
    // 设置读取失败时不落入该分支——沿用本地管线保证识别仍可用。
    if let Some(settings) = app
        .try_state::<crate::models::AppState>()
        .and_then(|state| state.store.settings().ok())
        .filter(|settings| settings.ocr_engine == crate::models::OcrEngine::Openai)
    {
        let cloud = settings.cloud_ocr;
        let cache_key = OcrCacheKey {
            image_key,
            engine_key: build_openai_engine_cache_key(
                &cloud.openai_active_profile_id,
                &cloud.openai_profiles,
                &cloud.openai_prompts,
            ),
            profile: profile_key,
            language: lang_key,
        };
        if let Some(hit) = lookup_cached_result(&cache_key) {
            return Ok(hit);
        }
        let profiles =
            openai::order_profiles_for_request(cloud.openai_profiles, &cloud.openai_active_profile_id);
        let img_path = image_path.clone();
        // manga 档位且语言为空时按日语提示
        let effective_language = if is_manga && language.is_none() {
            Some("ja".to_string())
        } else {
            language.clone()
        };
        let res = tokio::task::spawn_blocking(move || {
            openai::recognize_image_openai_profiles(
                &img_path,
                effective_language.as_deref(),
                &profiles,
                &cloud.openai_prompts,
            )
        })
        .await
        .map_err(|error| error.to_string())??;
        store_cached_result(cache_key, &res);
        return Ok(res);
    }

    if is_manga {
        let mocr_key = OcrCacheKey {
            image_key: image_key.clone(),
            engine_key: "mocr".to_string(),
            profile: profile_key.clone(),
            language: lang_key.clone(),
        };
        if let Some(hit) = lookup_cached_result(&mocr_key) {
            return Ok(hit);
        }
        // mocr.rs 内部按引擎可用性调度：ONNX sidecar（主）→ Python 常驻（回退）；
        // 全部失败则落入下方 Paddle manga 管线
        let app_handle = app.clone();
        let img_path = image_path.clone();
        if let Ok(mocr_res) = mocr::recognize_image_text_mocr(Some(&app_handle), &img_path).await {
            store_cached_result(mocr_key, &mocr_res);
            return Ok(mocr_res);
        }
    }

    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        let local_key = OcrCacheKey {
            image_key,
            engine_key: "macos-vision".to_string(),
            profile: profile_key,
            language: lang_key,
        };
        if let Some(hit) = lookup_cached_result(&local_key) {
            return Ok(hit);
        }
        let res = tokio::task::spawn_blocking(move || {
            vision::recognize_image_text_macos(image_path, language)
        })
        .await
        .map_err(|error| error.to_string())??;
        store_cached_result(local_key, &res);
        Ok(res)
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
        let mode = store
            .as_ref()
            .and_then(|s| s.settings().ok())
            .map(|s| s.ocr_mode.as_str())
            .unwrap_or(DEFAULT_OCR_MODE);
        let local_key = OcrCacheKey {
            image_key,
            engine_key: format!("paddle:{mode}"),
            profile: profile_key,
            language: lang_key,
        };
        if let Some(hit) = lookup_cached_result(&local_key) {
            return Ok(hit);
        }
        let res = tokio::task::spawn_blocking(move || match store.as_ref() {
            Some(store) => {
                paddle::recognize_image_text_paddle(&app, store, image_path, profile, language)
            }
            None => {
                paddle::recognize_with_mode(&app, DEFAULT_OCR_MODE, image_path, profile, language)
            }
        })
        .await
        .map_err(|error| error.to_string())??;
        store_cached_result(local_key, &res);
        Ok(res)
    }
}

#[cfg(test)]
mod language_tests {
    use super::{ordered_openai_profiles, vision_language_locale};
    use super::{OcrCacheKey, OcrResultCache, OCR_CACHE_CAPACITY};
    use crate::models::ImageOcrResult;

    fn sample_key(id: usize) -> OcrCacheKey {
        OcrCacheKey {
            image_key: format!("img-{id}.png:1024:100"),
            engine_key: "paddle:fast".to_string(),
            profile: "default".to_string(),
            language: "auto".to_string(),
        }
    }

    fn sample_result(text: &str) -> ImageOcrResult {
        ImageOcrResult {
            text: text.to_string(),
            engine: "paddle-mnn".to_string(),
            language: "auto".to_string(),
            words: Vec::new(),
        }
    }

    #[test]
    fn ocr_result_cache_hits_and_promotes_lru_and_evicts_oldest() {
        let mut cache = OcrResultCache::new(OCR_CACHE_CAPACITY);
        for i in 0..OCR_CACHE_CAPACITY {
            cache.insert(sample_key(i), sample_result(&format!("text-{i}")));
        }
        // 访问 key 0 使其提升为最新使用
        assert_eq!(cache.get(&sample_key(0)).unwrap().text, "text-0");

        // 插入第 33 条，应淘汰最久未使用的 key 1，保留 key 0
        cache.insert(sample_key(999), sample_result("text-999"));
        assert!(cache.get(&sample_key(1)).is_none(), "最久未访问的 key 1 应被淘汰");
        assert_eq!(cache.get(&sample_key(0)).unwrap().text, "text-0");
        assert_eq!(cache.get(&sample_key(999)).unwrap().text, "text-999");
    }

    #[test]
    fn ocr_result_cache_ignores_empty_text_results() {
        let mut cache = OcrResultCache::new(OCR_CACHE_CAPACITY);
        cache.insert(sample_key(1), sample_result("   "));
        assert!(cache.get(&sample_key(1)).is_none(), "空文本结果不应缓存");
    }

    fn profile(id: &str) -> crate::models::OpenaiOcrProfile {
        crate::models::OpenaiOcrProfile {
            id: id.to_string(),
            name: id.to_string(),
            base_url: format!("https://{id}.example.com/v1"),
            model: id.to_string(),
            api_key: "key".to_string(),
        }
    }

    #[test]
    fn ordered_openai_profiles_puts_active_first_and_keeps_rest_order() {
        let ordered = ordered_openai_profiles(vec![profile("a"), profile("b"), profile("c")], "b");
        assert_eq!(
            ordered
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "a", "c"]
        );
    }

    #[test]
    fn ordered_openai_profiles_tolerates_missing_active() {
        let ordered = ordered_openai_profiles(vec![profile("a"), profile("b")], "missing");
        assert_eq!(
            ordered
                .iter()
                .map(|profile| profile.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

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
