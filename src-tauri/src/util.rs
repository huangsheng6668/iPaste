use std::path::{Path, PathBuf};

#[cfg(not(target_os = "macos"))]
use std::fs;
#[cfg(not(target_os = "macos"))]
use std::io::Read;

use chrono::{SecondsFormat, Utc};
use sha2::{Digest, Sha256};
use tauri_plugin_global_shortcut::Shortcut;

use crate::{
    APPEND_COPY_TIMEOUT_OPTIONS, DISABLE_APPEND_COPY_LABEL, ENABLE_APPEND_COPY_LABEL,
    PAUSE_CAPTURE_LABEL, RESUME_CAPTURE_LABEL, RETENTION_OPTIONS,
};

pub(crate) fn clamp(value: i32, min: i32, max: i32) -> i32 {
    let max = max.max(min);
    value.max(min).min(max)
}

pub(crate) fn detect_clip_type(text: &str) -> String {
    let lower = text.trim().to_lowercase();
    if is_color(text) {
        "color".to_string()
    } else if lower.starts_with("http://") || lower.starts_with("https://") {
        "link".to_string()
    } else {
        "text".to_string()
    }
}

pub(crate) fn is_color(text: &str) -> bool {
    let value = text.trim();
    if let Some(hex) = value.strip_prefix('#') {
        return (hex.len() == 3 || hex.len() == 6 || hex.len() == 8)
            && hex.chars().all(|char| char.is_ascii_hexdigit());
    }

    value.starts_with("rgb(") || value.starts_with("rgba(")
}

pub(crate) fn preview(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(180).collect()
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(crate) fn hash_text(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(crate) fn clean_category_name(name: String) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        Err("请输入分类名称".to_string())
    } else if name.chars().count() > 40 {
        Err("分类名称不能超过 40 个字符".to_string())
    } else {
        Ok(name)
    }
}

pub(crate) fn clean_display_name(name: Option<String>) -> Result<Option<String>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    let name = name.trim().to_string();
    if name.is_empty() {
        Ok(None)
    } else if name.chars().count() > 80 {
        Err("条目名称不能超过 80 个字符".to_string())
    } else {
        Ok(Some(name))
    }
}

pub(crate) fn clean_shortcut(shortcut: String) -> Result<String, String> {
    let shortcut = shortcut.split_whitespace().collect::<String>();
    if shortcut.is_empty() {
        return Err("请输入快捷键".to_string());
    }

    let parsed = shortcut
        .parse::<Shortcut>()
        .map_err(|_| "快捷键格式无效，请同时按下修饰键和一个按键".to_string())?;
    if parsed.mods.is_empty() {
        return Err("快捷键需要包含 Ctrl、Cmd、Alt 或 Shift 等修饰键".to_string());
    }

    Ok(shortcut)
}

pub(crate) fn clean_retention_days(days: i64) -> Result<i64, String> {
    if RETENTION_OPTIONS.contains(&days) {
        Ok(days)
    } else {
        Err("请选择有效的数据保留时长".to_string())
    }
}

pub(crate) fn clean_append_copy_timeout_minutes(minutes: i64) -> Result<i64, String> {
    if APPEND_COPY_TIMEOUT_OPTIONS.contains(&minutes) {
        Ok(minutes)
    } else {
        Err("请选择有效的追加复制自动关闭时间".to_string())
    }
}

pub(crate) fn clean_panel_open_behavior(behavior: String) -> Result<String, String> {
    let behavior = behavior.trim();
    if behavior == "history" || behavior == "last_selected" {
        Ok(behavior.to_string())
    } else {
        Err("请选择有效的主窗口默认激活状态".to_string())
    }
}

pub(crate) fn clean_panel_layout(layout: String) -> Result<String, String> {
    let layout = layout.trim();
    if layout == "top" || layout == "side" {
        Ok(layout.to_string())
    } else {
        Err("请选择有效的主窗口布局".to_string())
    }
}

pub(crate) fn clean_ocr_mode(mode: String) -> Result<String, String> {
    let mode = mode.trim();
    if mode == "fast" || mode == "best" {
        Ok(mode.to_string())
    } else {
        Err("请选择有效的图片 OCR 模式".to_string())
    }
}

pub(crate) fn clean_language(language: String) -> Result<String, String> {
    let language = language.trim();
    if matches!(language, "en" | "zh-CN" | "ja" | "ko" | "es" | "fr" | "de") {
        Ok(language.to_string())
    } else {
        Err("Please choose a valid language".to_string())
    }
}

pub(crate) fn localized_text(language: &str, key: &str) -> &'static str {
    match (language, key) {
        ("zh-CN", "open_ipaste") => "打开 iPaste",
        ("ja", "open_ipaste") => "iPaste を開く",
        ("ko", "open_ipaste") => "iPaste 열기",
        ("es", "open_ipaste") => "Abrir iPaste",
        ("fr", "open_ipaste") => "Ouvrir iPaste",
        ("de", "open_ipaste") => "iPaste öffnen",
        (_, "open_ipaste") => "Open iPaste",
        ("zh-CN", "settings") => "设置...",
        ("ja", "settings") => "設定...",
        ("ko", "settings") => "설정...",
        ("es", "settings") => "Ajustes...",
        ("fr", "settings") => "Réglages...",
        ("de", "settings") => "Einstellungen...",
        (_, "settings") => "Settings...",
        ("zh-CN", "quit_ipaste") => "退出 iPaste",
        ("ja", "quit_ipaste") => "iPaste を終了",
        ("ko", "quit_ipaste") => "iPaste 종료",
        ("es", "quit_ipaste") => "Salir de iPaste",
        ("fr", "quit_ipaste") => "Quitter iPaste",
        ("de", "quit_ipaste") => "iPaste beenden",
        (_, "quit_ipaste") => "Quit iPaste",
        ("zh-CN", "tray_tooltip") => "iPaste 剪贴板管理器",
        ("ja", "tray_tooltip") => "iPaste クリップボードマネージャー",
        ("ko", "tray_tooltip") => "iPaste 클립보드 관리자",
        ("es", "tray_tooltip") => "Gestor del portapapeles iPaste",
        ("fr", "tray_tooltip") => "Gestionnaire de presse-papiers iPaste",
        ("de", "tray_tooltip") => "iPaste Zwischenablage-Manager",
        (_, "tray_tooltip") => "iPaste Clipboard Manager",
        ("zh-CN", "settings_title") => "iPaste 设置",
        ("ja", "settings_title") => "iPaste 設定",
        ("ko", "settings_title") => "iPaste 설정",
        ("es", "settings_title") => "Ajustes de iPaste",
        ("fr", "settings_title") => "Réglages iPaste",
        ("de", "settings_title") => "iPaste Einstellungen",
        (_, "settings_title") => "iPaste Settings",
        ("zh-CN", "pause_capture") => PAUSE_CAPTURE_LABEL,
        ("ja", "pause_capture") => "キャプチャを一時停止",
        ("ko", "pause_capture") => "캡처 일시 중지",
        ("es", "pause_capture") => "Pausar captura",
        ("fr", "pause_capture") => "Suspendre la capture",
        ("de", "pause_capture") => "Erfassung pausieren",
        (_, "pause_capture") => "Pause capture",
        ("zh-CN", "resume_capture") => RESUME_CAPTURE_LABEL,
        ("ja", "resume_capture") => "キャプチャを再開",
        ("ko", "resume_capture") => "캡처 다시 시작",
        ("es", "resume_capture") => "Reanudar captura",
        ("fr", "resume_capture") => "Reprendre la capture",
        ("de", "resume_capture") => "Erfassung fortsetzen",
        (_, "resume_capture") => "Resume capture",
        ("zh-CN", "enable_append_copy") => ENABLE_APPEND_COPY_LABEL,
        ("ja", "enable_append_copy") => "追記コピーを有効化",
        ("ko", "enable_append_copy") => "이어붙여 복사 켜기",
        ("es", "enable_append_copy") => "Activar copia acumulada",
        ("fr", "enable_append_copy") => "Activer la copie ajoutée",
        ("de", "enable_append_copy") => "Anhängekopie aktivieren",
        (_, "enable_append_copy") => "Enable append copy",
        ("zh-CN", "disable_append_copy") => DISABLE_APPEND_COPY_LABEL,
        ("ja", "disable_append_copy") => "追記コピーを無効化",
        ("ko", "disable_append_copy") => "이어붙여 복사 끄기",
        ("es", "disable_append_copy") => "Desactivar copia acumulada",
        ("fr", "disable_append_copy") => "Désactiver la copie ajoutée",
        ("de", "disable_append_copy") => "Anhängekopie deaktivieren",
        (_, "disable_append_copy") => "Disable append copy",
        _ => "iPaste",
    }
}

pub(crate) fn clean_api_address(address: String) -> Result<String, String> {
    let address = address.trim().trim_end_matches('/').to_string();
    if address.is_empty() {
        Err("请输入云同步 API 地址".to_string())
    } else if !(address.starts_with("http://") || address.starts_with("https://")) {
        Err("云同步 API 地址需要以 http:// 或 https:// 开头".to_string())
    } else {
        Ok(address)
    }
}

pub(crate) fn clean_api_key(api_key: String) -> Result<String, String> {
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        Err("请输入云同步 API Key".to_string())
    } else {
        Ok(api_key)
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn validate_relative_path(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || value.contains('\\')
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(format!("OCR manifest 路径不安全：{value}"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn file_sha256(path: &PathBuf) -> Result<String, String> {
    let mut file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}
