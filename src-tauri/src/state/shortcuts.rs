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
