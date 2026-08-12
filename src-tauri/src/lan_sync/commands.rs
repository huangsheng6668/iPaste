//! Tauri 命令层：把 `lan_sync` 模块（Tasks 1-5）暴露为前端可调用的 `#[tauri::command]`。
//!
//! 共八个命令；`open_lan_sync_window`（窗口接入）留给 Task 11。
//!
//! 状态机概览（详见 `mod.rs` 的 `LanSessionManager`）：
//! - `lan_create_session`：Idle → Hosting（host 模式，TCP listener）
//! - `lan_join_by_address`：Idle → WaitingPair（guest 模式）
//! - `lan_accept_pair`：WaitingPair 的 host 侧用户决定；通过预存的 oneshot 通知
//! - `lan_send_clip` / `lan_request_clip`：仅 Connected 态有效，发 ControlMsg
//! - `lan_disconnect`：任意非 Idle 态都允许；Connected 走 control_tx 让 session loop
//!   自清理；Hosting/WaitingPair 直接 abort host 任务 + reset_to_idle
//! - `lan_get_state`：纯查询，任意态可用

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::clipboard::{clipboard_read_to_payload, read_current_clipboard};
use crate::lan_sync::client::{join_by_address, join_scanned, tcp_scan};
use crate::lan_sync::port::{get_port_conflict, kill_port_process};
use crate::lan_sync::protocol::LAN_TCP_BASE_PORT;
use crate::lan_sync::server::start_host;
use crate::lan_sync::*;
use crate::models::*;

/// 生成 6 位大写匹配码（取 uuid v4 前 6 位）。
fn random_code() -> String {
    crate::new_id()[..6].to_uppercase()
}

#[tauri::command]
pub(crate) async fn lan_create_session(
    app: AppHandle,
    state: State<'_, AppState>,
    code: Option<String>,
) -> Result<LanSessionInfo, String> {
    let manager = app.lan_manager();
    // 已有进行中的会话（Hosting / WaitingPair / Connected）则拒绝新建。
    if manager.status_is_connected_or_hosting() {
        return Err("已有进行中的会话".to_string());
    }
    let code = code
        .map(|c| c.trim().to_string())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(random_code);
    // start_host 内部会把状态置为 Hosting 并存好 control_tx/rx + host_tasks。
    // 端口被占用时 start_host 返回错误；这里查占用进程把信息并入错误，前端据此弹窗。
    if let Err(error) = start_host(app.clone(), Arc::clone(&manager), state.store.clone(), code).await {
        let port = LAN_TCP_BASE_PORT;
        let detail = get_port_conflict(port)
            .ok()
            .flatten()
            .map(|c| format!("端口 {port} 被 {}（PID {}）占用", c.name, c.pid))
            .unwrap_or_else(|| format!("端口 {port} 被占用"));
        return Err(format!("{detail}。{error}"));
    }
    Ok(manager.snapshot())
}

#[tauri::command]
pub(crate) async fn lan_join_by_address(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    code: String,
) -> Result<(), String> {
    let manager = app.lan_manager();
    join_by_address(Arc::clone(&manager), state.store.clone(), address, code).await;
    Ok(())
}

#[tauri::command]
pub(crate) fn lan_accept_pair(app: AppHandle, accept: bool) -> Result<(), String> {
    let manager = app.lan_manager();
    if let Some(tx) = manager.take_pair_decision_tx() {
        let _ = tx.send(accept);
        Ok(())
    } else {
        Err("当前没有待确认的加入请求".to_string())
    }
}

#[tauri::command]
pub(crate) async fn lan_send_clip(
    app: AppHandle,
    state: State<'_, AppState>,
    source: ClipSource,
) -> Result<(), String> {
    let manager = app.lan_manager();
    // 构造待发送的 (clip_type, payload_bytes)。
    let (clip_type, payload) = match source {
        ClipSource::Current => {
            let opt = clipboard_read_to_payload(read_current_clipboard()?)?;
            opt.ok_or_else(|| "当前剪贴板为空".to_string())?
        }
        ClipSource::Item { id } => {
            let conn = state.store.connect()?;
            let clip = state.store.get_clip_with_conn(&conn, &id)?;
            // 图片条目的 text 字段存储为 data url（见 store/clips.rs::insert_captured_item）。
            (clip.clip_type, clip.text.into_bytes())
        }
    };
    let Some(tx) = manager.control_tx() else {
        return Err("未连接".to_string());
    };
    tx.send(ControlMsg::SendClip { clip_type, payload })
        .await
        .map_err(|_| "会话已关闭".to_string())
}

#[tauri::command]
pub(crate) async fn lan_request_clip(app: AppHandle) -> Result<(), String> {
    let manager = app.lan_manager();
    let Some(tx) = manager.control_tx() else {
        return Err("未连接".to_string());
    };
    tx.send(ControlMsg::RequestClip)
        .await
        .map_err(|_| "会话已关闭".to_string())
}

#[tauri::command]
pub(crate) async fn lan_disconnect(app: AppHandle) -> Result<(), String> {
    let manager = app.lan_manager();
    match manager.snapshot().status {
        LanStatus::Connected => {
            // Connected 态：发 Disconnect 给 session loop，由它清理 + reset_to_idle。
            if let Some(tx) = manager.control_tx() {
                let _ = tx.send(ControlMsg::Disconnect).await;
            }
        }
        LanStatus::Hosting | LanStatus::WaitingPair => {
            // Host 侧：abort accept 任务以释放端口，再 reset。
            // Guest 侧的 WaitingPair：abort_host_tasks 是 no-op（host_tasks=None）；
            // 此时可能有一个 in-flight 握手任务会后续覆写状态——已知 MVP 限制。
            manager.abort_host_tasks();
            manager.reset_to_idle("已断开".to_string());
        }
        LanStatus::Idle => {
            // 已 Idle：真正的 no-op，不 emit 任何事件（避免伪造 disconnected）。
        }
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn lan_get_state(app: AppHandle) -> Result<LanSessionInfo, String> {
    Ok(app.lan_manager().snapshot())
}

#[tauri::command]
pub(crate) async fn open_lan_sync(app: AppHandle) -> Result<(), String> {
    crate::window::open_lan_sync_window(&app)
}

#[tauri::command]
pub(crate) async fn lan_scan_devices(_app: AppHandle, _timeout_secs: u64) -> Result<Vec<LanDevice>, String> {
    Ok(tcp_scan().await)
}

#[tauri::command]
pub(crate) async fn lan_join_scanned(
    app: AppHandle,
    state: State<'_, AppState>,
    addr: String,
) -> Result<(), String> {
    let manager = app.lan_manager();
    join_scanned(manager, state.store.clone(), addr).await;
    Ok(())
}

/// 查询固定端口 `LAN_TCP_BASE_PORT` 的占用进程（用于 UI 提示与一键 kill）。
#[tauri::command]
pub(crate) fn lan_get_port_conflict() -> Result<Option<PortConflict>, String> {
    get_port_conflict(LAN_TCP_BASE_PORT)
}

/// 结束占用固定端口的进程（前端弹窗确认后调用）。
#[tauri::command]
pub(crate) fn lan_kill_port_process(pid: u32) -> Result<(), String> {
    kill_port_process(pid)
}

/// 退出整个 App（前端在「占用进程是自身残留实例」等场景下调用）。
#[tauri::command]
pub(crate) fn lan_quit_app(app: AppHandle) {
    app.exit(0);
}
