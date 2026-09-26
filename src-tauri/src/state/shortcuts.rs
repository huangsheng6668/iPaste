//! 全局快捷键域状态：面板/截图快捷键的当前注册值与总开关。

use std::sync::{Arc, Mutex};

/// 快捷键域上下文（Task 27 聚合）。
pub struct ShortcutState {
    /// 当前面板唤出快捷键（已注册成功的值）。
    pub active_shortcut: Arc<Mutex<String>>,
    /// 当前截图 OCR 快捷键。
    pub active_ocr_shortcut: Arc<Mutex<String>>,
    /// 应用级快捷键总开关。
    pub is_app_shortcut_enabled: Arc<Mutex<bool>>,
}

impl ShortcutState {
    /// 当前面板快捷键（锁中毒 → Err，与原调用点一致）。
    pub(crate) fn active_shortcut(&self) -> Result<String, String> {
        self.active_shortcut
            .lock()
            .map(|value| value.clone())
            .map_err(|error| error.to_string())
    }

    /// 写入面板快捷键。
    pub(crate) fn set_active_shortcut(&self, shortcut: &str) -> Result<(), String> {
        *self
            .active_shortcut
            .lock()
            .map_err(|error| error.to_string())? = shortcut.to_string();
        Ok(())
    }

    /// 当前截图 OCR 快捷键。
    pub(crate) fn active_ocr_shortcut(&self) -> Result<String, String> {
        self.active_ocr_shortcut
            .lock()
            .map(|value| value.clone())
            .map_err(|error| error.to_string())
    }

    /// 写入截图 OCR 快捷键。
    pub(crate) fn set_active_ocr_shortcut(&self, shortcut: &str) -> Result<(), String> {
        *self
            .active_ocr_shortcut
            .lock()
            .map_err(|error| error.to_string())? = shortcut.to_string();
        Ok(())
    }

    /// 应用级快捷键总开关。
    pub(crate) fn is_app_shortcut_enabled(&self) -> Result<bool, String> {
        self.is_app_shortcut_enabled
            .lock()
            .map(|value| *value)
            .map_err(|error| error.to_string())
    }

    /// 写入应用级快捷键总开关。
    pub(crate) fn set_app_shortcut_enabled(&self, enabled: bool) -> Result<(), String> {
        *self
            .is_app_shortcut_enabled
            .lock()
            .map_err(|error| error.to_string())? = enabled;
        Ok(())
    }
}
