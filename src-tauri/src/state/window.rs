//! 窗口域状态：面板拖动抑制标记、目标应用标识、主窗口激活方式，
//! 以及 macOS 专属的主面板状态缓存。

use std::sync::{Arc, Mutex};

use crate::models::MainWindowActivation;

/// 窗口域上下文（Task 27 聚合）。
pub struct WindowState {
    /// 主面板正在被拖动（拖动期间抑制面板交互）。
    pub is_dragging_main_window: Arc<Mutex<bool>>,
    /// 面板唤出前的前台应用标识（粘贴回目标）。
    pub target_app_bundle_id: Arc<Mutex<Option<String>>>,
    /// 主窗口应如何激活（普通显示 / 保持当前应用）。
    pub main_window_activation: Arc<Mutex<MainWindowActivation>>,
    /// macOS 原生面板状态缓存（其他平台不存在该字段）。
    #[cfg(target_os = "macos")]
    pub main_panel_state: Arc<Mutex<Option<crate::models::MainPanelState>>>,
}
