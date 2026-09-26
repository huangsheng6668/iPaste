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

impl WindowState {
    /// 读取"主面板正在拖动"标记。
    pub(crate) fn is_dragging_main_window(&self) -> Result<bool, String> {
        self.is_dragging_main_window
            .lock()
            .map(|value| *value)
            .map_err(|error| error.to_string())
    }

    /// 写入"主面板正在拖动"标记。
    pub(crate) fn set_dragging_main_window(&self, dragging: bool) -> Result<(), String> {
        *self
            .is_dragging_main_window
            .lock()
            .map_err(|error| error.to_string())? = dragging;
        Ok(())
    }

    /// 记录面板唤出前的前台应用标识（锁不可用时静默跳过，与原实现一致）。
    pub(crate) fn set_target_app_bundle_id(&self, bundle_id: Option<String>) {
        if let Ok(mut target) = self.target_app_bundle_id.lock() {
            *target = bundle_id;
        }
    }

    /// 读取目标应用标识；锁中毒 → Err（与原调用点的错误语义一致，内容为 None 时正常返回 None）。
    pub(crate) fn target_app_bundle_id(&self) -> Result<Option<String>, String> {
        self.target_app_bundle_id
            .lock()
            .map(|value| value.clone())
            .map_err(|error| error.to_string())
    }
}
