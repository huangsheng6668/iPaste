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

use base64::Engine as _;
use tauri::{AppHandle, State};

use crate::clipboard::{clipboard_read_to_payload, image_bytes_from_data_url, read_current_clipboard};
use crate::lan_sync::client::join_by_address;
use crate::lan_sync::port::{get_port_conflict, kill_port_process, verify_port_owner};
use crate::lan_sync::protocol::{normalize_pair_code, LAN_TCP_BASE_PORT};
use crate::lan_sync::server::start_host;
use crate::lan_sync::*;
use crate::models::*;
use crate::Store;

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
    let code = normalize_pair_code(code)?;
    // start_host 内部会把状态置为 Hosting 并存好 control_tx/rx + host_tasks。
    // 端口被占用时 start_host 返回错误；这里查占用进程把信息并入错误，前端据此弹窗。
    if let Err(error) = start_host(Arc::clone(&manager), state.store.clone(), code).await {
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
    // 与 lan_create_session 同款守门：已有进行中的会话（含 WaitingPair）则拒绝，
    // 防止并发 join 互相覆写控制通道。
    if manager.status_is_connected_or_hosting() {
        return Err("已有进行中的会话".to_string());
    }
    // 握手与会话循环放后台任务执行（不随命令返回而结束）；句柄存入 manager，
    // 断开/reset 时 abort——消灭「用户已断开、残留握手任务继续跑完再弹
    // join 失败」的迟到错误事件。
    let join = tokio::spawn(join_by_address(
        Arc::clone(&manager),
        state.store.clone(),
        address,
        code,
    ));
    manager.set_join_task(join);
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

/// 待发送条目（clip_type, payload, category_name, category_color, display_name）。
/// category_name 为 None 表示历史/无分组条目；Some 表示分组条目（接收端按名称匹配分组）。
type SendItem = (String, Vec<u8>, Option<String>, Option<String>, Option<String>);

#[tauri::command]
pub(crate) async fn lan_send_clip(
    app: AppHandle,
    state: State<'_, AppState>,
    source: ClipSource,
) -> Result<(), String> {
    let manager = app.lan_manager();
    let (clip_type, payload, category_name, category_color, display_name) = match source {
        ClipSource::Current => {
            let opt = clipboard_read_to_payload(read_current_clipboard()?)?;
            let (ct, data) = opt.ok_or_else(|| "当前剪贴板为空".to_string())?;
            (ct, data, None, None, None)
        }
        ClipSource::Item { id } => {
            // 历史条目若已加入某个分类，一并携带分类名/颜色：接收端会按名称匹配或
            // 新建同名分类并把条目放入该分类（用户期望的「条目 + 分类一起同步」）。
            // 未入分类的普通历史条目保持旧行为（category_name = None）。
            build_item_send(&state.store, &id)?
        }
        ClipSource::CategoryItem { id, category_id } => {
            build_category_item_send(&state.store, &id, &category_id)?
        }
    };
    let Some(tx) = manager.control_tx() else {
        return Err("未连接".to_string());
    };
    tx.send(ControlMsg::SendClip { clip_type, payload, category_name, category_color, display_name })
        .await
        .map_err(|_| "会话已关闭".to_string())
}

/// 整组发送：把某分组下的全部条目一次性推给对端（`lan_send_category`）。
///
/// 流程：`BatchStart` → 逐条 `SendClip`（携带分组名/颜色 + 条目重命名）→ `BatchEnd`。
/// 单条 payload 构建失败（如图片文件缺失）跳过并计数，不中断整组；
/// 通道关闭（会话已断）时立即中止并报错。完成后 emit `lan-category-sent` 汇总事件。
#[tauri::command]
pub(crate) async fn lan_send_category(
    app: AppHandle,
    state: State<'_, AppState>,
    category_id: String,
) -> Result<LanCategorySent, String> {
    let manager = app.lan_manager();
    let Some(tx) = manager.control_tx() else {
        return Err("未连接".to_string());
    };

    let conn = state.store.connect()?;
    let category = state.store.get_category_with_conn(&conn, &category_id)?;
    let items = state
        .store
        .list_category_items_for_category_with_conn(&conn, &category_id)?;
    if items.is_empty() {
        return Err("该分组没有可发送的条目".to_string());
    }

    let item_count = items.len().min(u32::MAX as usize) as u32;
    tx.send(ControlMsg::BatchStart {
        category_name: category.name.clone(),
        category_color: Some(category.color.clone()),
        item_count,
    })
    .await
    .map_err(|_| "会话已关闭".to_string())?;

    let mut sent: u32 = 0;
    let mut failed: u32 = 0;
    for item in &items {
        match build_send_payload(&item.clip_type, &item.text) {
            Ok(payload) => {
                if tx
                    .send(ControlMsg::SendClip {
                        clip_type: item.clip_type.clone(),
                        payload,
                        category_name: Some(category.name.clone()),
                        category_color: Some(category.color.clone()),
                        display_name: item.display_name.clone(),
                    })
                    .await
                    .is_err()
                {
                    return Err("会话已关闭".to_string());
                }
                sent += 1;
            }
            Err(reason) => {
                eprintln!("[lan-sync] 整组发送跳过条目 {}：{reason}", item.id);
                failed += 1;
            }
        }
    }

    if tx.send(ControlMsg::BatchEnd).await.is_err() {
        return Err("会话已关闭".to_string());
    }

    manager.emit_category_sent(category.name.clone(), sent, failed);
    Ok(LanCategorySent {
        category_name: category.name,
        sent,
        failed,
    })
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
            // reset_to_idle 会 abort 在途的 join 任务（manager.join_task），
            // 残留握手任务不会再覆写状态或弹迟到的 join-failed。
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

/// 查询固定端口 `LAN_TCP_BASE_PORT` 的占用进程（用于 UI 提示与一键 kill）。
#[tauri::command]
pub(crate) fn lan_get_port_conflict() -> Result<Option<PortConflict>, String> {
    get_port_conflict(LAN_TCP_BASE_PORT)
}

/// 结束占用固定端口的进程（前端弹窗确认后调用）。
#[tauri::command]
pub(crate) fn lan_kill_port_process(pid: u32) -> Result<(), String> {
    // 后端复核：只有确实占用 45130 端口的进程才允许被结束
    let conflict = get_port_conflict(LAN_TCP_BASE_PORT)?;
    verify_port_owner(conflict, pid)?;
    kill_port_process(pid)
}

/// 退出整个 App（前端在「占用进程是自身残留实例」等场景下调用）。
#[tauri::command]
pub(crate) fn lan_quit_app(app: AppHandle) {
    app.exit(0);
}

/// 把待发送的条目内容编码成 LAN 同步 payload 字节。
///
/// - 文本类条目：`text` 即原文，直接转 UTF-8 字节。
/// - 图片类条目（`clip_type == "image"`）：DB 里 `text` 存的是**本地图片文件路径**
///   （见 `store::clips::save_image_bytes` 与 `migrations::migrate_image_data_urls`），
///   不能直接发路径（对端机器上不存在该文件）。这里读回图片字节并编码成自包含的
///   `data:image/png;base64,...` 形式，与 `clipboard::clipboard_read_to_payload`
///   处理「当前剪贴板图片」的方式一致，接收侧 `captured_item_from_payload` 能解码。
fn build_send_payload(clip_type: &str, text: &str) -> Result<Vec<u8>, String> {
    if clip_type == "image" {
        let bytes = std::fs::read(text).map_err(|e| format!("读取图片文件失败：{e}"))?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        Ok(format!("data:image/png;base64,{b64}").into_bytes())
    } else {
        Ok(text.as_bytes().to_vec())
    }
}

/// 从历史条目构造待发送的 `SendItem`。
///
/// 历史条目若已加入某个分类，一并携带分类名/颜色——接收端 `apply_received` 会
/// 按名称匹配或新建同名分类并把条目放入该分类（用户期望「条目 + 分类一起同步，
/// B 端没有该分类则创建」）。未入分类的普通历史条目保持旧行为（分类均为 `None`）。
/// 条目若有重命名（display_name），一并携带（接收端落到 `clips`/`category_items`）。
fn build_item_send(store: &Store, id: &str) -> Result<SendItem, String> {
    let conn = store.connect()?;
    let clip = store.get_clip_with_conn(&conn, id)?;
    // 图片条目的 text 存的是本地文件路径，build_send_payload 会读回字节并编码成
    // 自包含的 data url；文本条目直接转 UTF-8 字节。
    let payload = build_send_payload(&clip.clip_type, &clip.text)?;
    let (category_name, category_color) =
        match store.get_category_for_clip_with_conn(&conn, &clip.content_hash)? {
            Some(category) => (Some(category.name), Some(category.color)),
            None => (None, None),
        };
    Ok((clip.clip_type, payload, category_name, category_color, clip.display_name))
}

/// 从分组条目构造待发送的 `SendItem`（携带分组名/颜色 + 条目重命名）。
fn build_category_item_send(
    store: &Store,
    id: &str,
    category_id: &str,
) -> Result<SendItem, String> {
    let conn = store.connect()?;
    let item = store.get_category_item_with_conn(&conn, id)?;
    // 校验条目确实属于所声明分组，避免前端传错 id。
    if item.category_id != category_id {
        return Err("条目不属于该分组".to_string());
    }
    let category = store.get_category_with_conn(&conn, category_id)?;
    // 与 Item 分支一致：图片条目发 data url 而非本地路径。
    let payload = build_send_payload(&item.clip_type, &item.text)?;
    Ok((
        item.clip_type,
        payload,
        Some(category.name),
        Some(category.color),
        item.display_name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_send_payload_text_returns_utf8_bytes() {
        let payload = build_send_payload("text", "hello-api-key").unwrap();
        assert_eq!(payload, b"hello-api-key");
    }

    #[test]
    fn build_send_payload_image_reads_file_and_encodes_data_url() {
        // 建一个临时 png 文件模拟 DB 里图片条目的 text（文件路径）。
        let dir = std::env::temp_dir().join(format!("ipaste-send-payload-{}", crate::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png_path = dir.join("img.png");
        // 1x1 透明 png 的最小字节
        let png_bytes = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // png signature
        ];
        std::fs::write(&png_path, png_bytes).unwrap();

        let payload = build_send_payload("image", png_path.to_str().unwrap()).unwrap();
        let text = String::from_utf8(payload).unwrap();
        assert!(
            text.starts_with("data:image/png;base64,"),
            "图片 payload 应是 data url，实际：{text}"
        );
        // 解码回来的字节与原文件一致
        let decoded = image_bytes_from_data_url(&text).unwrap();
        assert_eq!(decoded, png_bytes);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn build_send_payload_image_missing_file_errors() {
        // 不存在的路径应返回可读错误，而非 panic 或发空 payload。
        let result = build_send_payload("image", "/definitely/not/here/xyz.png");
        assert!(result.is_err());
    }

    /// 历史条目已加入分类：发送时携带分类名/颜色（接收端据此匹配/创建分类）。
    /// 覆盖用户报告的场景——A 端条目属于分类，B 端应收到分类信息并把条目放入分类。
    #[test]
    fn build_item_send_carries_category_for_joined_clip() {
        use crate::store::test_support::{create_category, temp_store};

        let store = temp_store();
        let conn = store.connect().unwrap();

        // 手工插入一条已知 id 的历史 clip，并把 category_items.clip_snapshot_id 指向它。
        let clip_id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, 'text', ?2, NULL, ?3, ?4, 'test', ?5, 0, 0)",
            rusqlite::params![
                clip_id,
                crate::util::hash_text("sk-api-key-123"),
                "sk-api-key-123",
                "sk-api-key-123",
                now,
            ],
        )
        .unwrap();
        let cat_id = create_category(&conn, "api_key", "#3B82F6", 0);
        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, ?3, 'text', ?4, NULL, ?5, ?5, 0, ?6, ?6, 'local', 0)",
            rusqlite::params![
                crate::new_id(),
                cat_id,
                clip_id,
                crate::util::hash_text("sk-api-key-123"),
                "sk-api-key-123",
                now,
            ],
        )
        .unwrap();

        let (clip_type, payload, category_name, category_color, display_name) =
            build_item_send(&store, &clip_id).unwrap();
        assert_eq!(clip_type, "text");
        assert_eq!(payload, b"sk-api-key-123");
        assert_eq!(category_name.as_deref(), Some("api_key"));
        assert_eq!(category_color.as_deref(), Some("#3B82F6"));
        assert_eq!(display_name, None, "未重命名的条目 display_name 应为 None");
    }

    /// 历史条目带重命名：发送时携带 display_name（用户报告的「A 端重命名
    /// B 端收不到」场景——发送侧必须把重命名放进取帧）。
    #[test]
    fn build_item_send_carries_display_name() {
        use crate::store::test_support::temp_store;

        let store = temp_store();
        let conn = store.connect().unwrap();

        let clip_id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, 'text', ?2, '登录口令', ?3, ?4, 'test', ?5, 0, 0)",
            rusqlite::params![
                clip_id,
                crate::util::hash_text("sk-api-key-456"),
                "sk-api-key-456",
                "sk-api-key-456",
                now,
            ],
        )
        .unwrap();

        let (_, _, _, _, display_name) = build_item_send(&store, &clip_id).unwrap();
        assert_eq!(display_name.as_deref(), Some("登录口令"));
    }

    /// 分组条目带重命名：发送时携带 display_name（单条发送路径）。
    #[test]
    fn build_category_item_send_carries_display_name() {
        use crate::store::test_support::{create_category, temp_store};

        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat_id = create_category(&conn, "api_key", "#3B82F6", 0);
        let item_id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, 'snap', 'text', ?3, '重命名的密钥', ?4, ?4, 0, ?5, ?5, 'local', 0)",
            rusqlite::params![
                item_id,
                cat_id,
                crate::util::hash_text("sk-abc"),
                "sk-abc",
                now,
            ],
        )
        .unwrap();

        let (clip_type, payload, category_name, category_color, display_name) =
            build_category_item_send(&store, &item_id, &cat_id).unwrap();
        assert_eq!(clip_type, "text");
        assert_eq!(payload, b"sk-abc");
        assert_eq!(category_name.as_deref(), Some("api_key"));
        assert_eq!(category_color.as_deref(), Some("#3B82F6"));
        assert_eq!(display_name.as_deref(), Some("重命名的密钥"));
    }

    /// 未入分类的历史条目：保持旧行为（分类信息为 None），接收端落入历史表。
    #[test]
    fn build_item_send_has_no_category_for_plain_clip() {
        use crate::store::test_support::{seed_clip, temp_store};

        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "plain", "plain text");
        let clip_id: String = conn
            .query_row("SELECT id FROM clips LIMIT 1", [], |row| row.get(0))
            .unwrap();

        let (clip_type, payload, category_name, category_color, display_name) =
            build_item_send(&store, &clip_id).unwrap();
        assert_eq!(clip_type, "text");
        assert_eq!(payload, b"plain text");
        assert!(category_name.is_none());
        assert!(category_color.is_none());
        assert!(display_name.is_none());
    }

    /// 接收侧落库与分类创建的联动：B 端没有该分类时创建，条目落到该分类下
    /// （即 `insert_received_category_item`，由 store 层测试覆盖，这里验证
    /// 发送侧产出的 category_name 能原样驱动该路径）。
    #[test]
    fn build_item_send_category_name_drives_receive_insert() {
        use crate::store::test_support::{create_category, temp_store};

        let store = temp_store();
        let conn = store.connect().unwrap();

        let clip_id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, 'text', ?2, NULL, ?3, ?4, 'test', ?5, 0, 0)",
            rusqlite::params![clip_id, crate::util::hash_text("token"), "token", "token", now],
        )
        .unwrap();
        let cat_id = create_category(&conn, "工作", "#0D9488", 0);
        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, ?3, 'text', ?4, NULL, ?5, ?5, 0, ?6, ?6, 'local', 0)",
            rusqlite::params![
                crate::new_id(),
                cat_id,
                clip_id,
                crate::util::hash_text("token"),
                "token",
                now,
            ],
        )
        .unwrap();

        let (clip_type, payload, category_name, category_color, display_name) =
            build_item_send(&store, &clip_id).unwrap();
        // 模拟 B 端接收：把发送侧产物原样交给接收侧落库函数。
        let received = store
            .insert_received_category_item(
                clip_type,
                crate::util::hash_text(&String::from_utf8(payload.clone()).unwrap()),
                String::from_utf8(payload.clone()).unwrap(),
                String::from_utf8(payload).unwrap(),
                category_name.unwrap(),
                category_color,
                display_name,
                None,
            )
            .unwrap();
        assert_eq!(received.category_id, cat_id, "同名分类应被复用");
    }
}
