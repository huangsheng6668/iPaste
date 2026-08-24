//! Tauri 命令层占位：v4 TCP 会话命令（lan_create_session/lan_join_by_address/
//! lan_send_clip 等）已随协议 v5 迁移移除，Task 8 以 iroh 会话命令重写本模块。
//!
//! 目前仅保留 `open_lan_sync`：主面板 TopBar 与命令面板仍需打开 lan-sync 窗口
//! （窗口内为占位 UI，Task 10 重写）。窗口接入的完整收口在 Task 11。

use tauri::AppHandle;

use crate::error::AppError;

#[tauri::command]
pub(crate) async fn open_lan_sync(app: AppHandle) -> Result<(), AppError> {
    crate::window::open_lan_sync_window(&app).map_err(AppError::from)
}
