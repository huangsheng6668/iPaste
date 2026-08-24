//! 跨设备同步命令层（v5）：设备管理 + 票据配对 + 定向发送。
//!
//! v4 的 TCP 会话命令（lan_create_session/lan_join_by_address/lan_send_clip 等）
//! 已随协议 v5 移除；本模块把 DeviceLinkRegistry（iroh 端点 + 连接登记）暴露为
//! 前端可调用的 `#[tauri::command]`。发送类命令的 payload 装配逻辑从 v4
//! `lan_send_clip` 原样迁移（`clipboard_read_to_payload` / store 查询 API 未变），
//! 分发统一走 `registry.send_raw(target, …)`（None = 全部在线设备）。
//!
//! registry 在 lib.rs setup 构造；构造失败时应用继续运行（不 manage），
//! 此处经 `DeviceRegistryExt` 优雅报错而非 panic。

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::clipboard::{clipboard_read_to_payload, read_current_clipboard};
use crate::error::AppError;
use crate::lan_sync::registry::{build_send_payload, DeviceLinkRegistry};
use crate::lan_sync::ClipSource;
use crate::models::{AppState, AutoPushSettings, AutoSyncMode, DeviceInfo};
use crate::store::Store;

/// 从 Tauri State 取 `Arc<DeviceLinkRegistry>`。缺失（启动失败/尚未就绪）时
/// 返回「同步服务启动失败」AppError——绝不 unwrap managed state。
pub(crate) trait DeviceRegistryExt {
    fn device_registry(&self) -> Result<Arc<DeviceLinkRegistry>, AppError>;
}

impl DeviceRegistryExt for tauri::AppHandle {
    fn device_registry(&self) -> Result<Arc<DeviceLinkRegistry>, AppError> {
        self.try_state::<Arc<DeviceLinkRegistry>>()
            .map(|state| state.inner().clone())
            .ok_or_else(|| AppError::internal("同步服务启动失败，请重启应用后再试"))
    }
}

// —— 设备管理 ——

#[tauri::command]
pub(crate) fn device_list(app: AppHandle) -> Result<Vec<DeviceInfo>, AppError> {
    Ok(app.device_registry()?.device_infos())
}

#[tauri::command]
pub(crate) fn device_revoke(app: AppHandle, node_id: String) -> Result<(), AppError> {
    app.device_registry()?.revoke(&node_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn device_delete(app: AppHandle, node_id: String) -> Result<(), AppError> {
    app.device_registry()?.delete_device(&node_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn device_disconnect(app: AppHandle, node_id: String) -> Result<(), AppError> {
    app.device_registry()?.disconnect(&node_id);
    Ok(())
}

#[tauri::command]
pub(crate) fn device_set_auto_sync(
    app: AppHandle,
    node_id: String,
    mode: AutoSyncMode,
) -> Result<(), AppError> {
    app.device_registry()?.set_auto_sync(&node_id, mode);
    Ok(())
}

// —— 配对（票据邀请 / 加入 / 确认）——

/// 生成配对票据（覆盖旧邀请）。registry 内会尽力等待中继上线（上限 5s），
/// 故为 async；票据同时经 EVENT_PAIR_INVITE_STATE 事件推送前端。
#[tauri::command]
pub(crate) async fn pairing_create_invite(app: AppHandle) -> Result<String, AppError> {
    app.device_registry()?
        .create_invite()
        .await
        .map_err(AppError::internal)
}

#[tauri::command]
pub(crate) fn pairing_cancel_invite(app: AppHandle) -> Result<(), AppError> {
    app.device_registry()?.cancel_invite().map_err(AppError::internal)
}

/// 凭票据加入对方。`registry.join` 会内联 await 整个会话（拨号 + 对方用户
/// 确认可能耗时数十秒），必须放后台任务：命令立即返回，连接类失败经
/// EVENT_PAIR_JOIN_FAILED 事件反馈。票据本身的格式错误 registry 选择直达
/// 返回（不 emit），而后台任务的返回值无人消费——故在 spawn 前预检一次，
/// 贴错内容时同步报错给 invoke 调用方。
#[tauri::command]
pub(crate) async fn pairing_join(app: AppHandle, ticket: String) -> Result<(), AppError> {
    let registry = app.device_registry()?;
    crate::lan_sync::ticket::PairTicket::decode(&ticket).map_err(AppError::internal)?;
    tokio::spawn(async move {
        if let Err(reason) = registry.join(&ticket).await {
            eprintln!("[lan-sync] 加入配对失败：{reason}");
        }
    });
    Ok(())
}

/// 对待确认的配对请求作出决定（host 侧）。
#[tauri::command]
pub(crate) fn pairing_respond(app: AppHandle, accept: bool) -> Result<(), AppError> {
    app.device_registry()?
        .respond_pair(accept)
        .map_err(AppError::internal)
}

// —— 定向发送 ——

/// 待发送条目（clip_type, payload, category_name, category_color, display_name）。
/// category_name 为 None 表示历史/无分组条目；Some 表示分组条目（接收端按
/// 名称匹配或新建同名分组）。
type SendItem = (String, Vec<u8>, Option<String>, Option<String>, Option<String>);

/// 发送单条剪贴板内容。`target = None` 广播到全部在线设备；payload 装配逻辑
/// 从 v4 `lan_send_clip` 原样迁移。
#[tauri::command]
pub(crate) async fn device_send_clip(
    app: AppHandle,
    state: State<'_, AppState>,
    target: Option<String>,
    source: ClipSource,
) -> Result<(), AppError> {
    let registry = app.device_registry()?;
    let (clip_type, payload, category_name, category_color, display_name) = match source {
        ClipSource::Current => {
            let read = read_current_clipboard().map_err(AppError::internal)?;
            let opt = clipboard_read_to_payload(read).map_err(AppError::internal)?;
            let (clip_type, payload) =
                opt.ok_or_else(|| AppError::internal("当前剪贴板为空"))?;
            (clip_type, payload, None, None, None)
        }
        ClipSource::Item { id } => {
            build_item_send(&state.store, &id).map_err(AppError::internal)?
        }
        ClipSource::CategoryItem { id, category_id } => {
            build_category_item_send(&state.store, &id, &category_id)
                .map_err(AppError::internal)?
        }
    };
    registry
        .send_raw(
            target.as_deref(),
            &clip_type,
            &payload,
            category_name.as_deref(),
            category_color.as_deref(),
            display_name.as_deref(),
        )
        .await
        .map_err(AppError::internal)
}

/// `device_send_category` 的返回值（驼峰序列化，前端消费）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceSendCategoryResult {
    pub(crate) category_name: String,
    pub(crate) sent: u32,
    pub(crate) failed: u32,
}

/// 整组发送某分组：registry 组装 `BatchStart → SendClip × N → BatchEnd` 并
/// 逐目标 emit `DeviceCategorySent` 汇总事件；这里只回传聚合计数。
#[tauri::command]
pub(crate) async fn device_send_category(
    app: AppHandle,
    target: Option<String>,
    category_id: String,
) -> Result<DeviceSendCategoryResult, AppError> {
    let registry = app.device_registry()?;
    let (category_name, sent, failed) = registry
        .send_category(target.as_deref(), &category_id)
        .await
        .map_err(AppError::internal)?;
    Ok(DeviceSendCategoryResult { category_name, sent, failed })
}

/// 请求指定设备回推它当前的剪贴板内容。
#[tauri::command]
pub(crate) async fn device_request_clip(app: AppHandle, node_id: String) -> Result<(), AppError> {
    app.device_registry()?
        .request_clip(&node_id)
        .await
        .map_err(AppError::internal)
}

// —— 传输设置（自定义中继）——

/// `sync_transport_settings_get` 的返回值。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncTransportSettings {
    pub(crate) relay_url: Option<String>,
}

/// `sync_transport_settings_set` 的返回值：落库后的规范化地址 + 生效提示
/// （endpoint 在启动时按此设置绑定，变更需重启应用）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SyncTransportSettingsUpdate {
    pub(crate) relay_url: Option<String>,
    pub(crate) hint: String,
}

#[tauri::command]
pub(crate) fn sync_transport_settings_get(
    state: State<'_, AppState>,
) -> Result<SyncTransportSettings, AppError> {
    state
        .store
        .sync_relay_url()
        .map(|relay_url| SyncTransportSettings { relay_url })
        .map_err(AppError::internal)
}

/// 更新自定义中继（空/None = 恢复 n0 默认；非空须 `https://` 前缀）。
#[tauri::command]
pub(crate) fn sync_transport_settings_set(
    state: State<'_, AppState>,
    relay_url: Option<String>,
) -> Result<SyncTransportSettingsUpdate, AppError> {
    let relay_url = state
        .store
        .update_sync_relay_url(relay_url.as_deref())
        .map_err(AppError::internal)?;
    let hint = if relay_url.is_some() {
        "自定义中继已保存，重启 iPaste 后生效"
    } else {
        "已恢复 n0 默认中继，重启 iPaste 后生效"
    };
    Ok(SyncTransportSettingsUpdate {
        relay_url,
        hint: hint.to_string(),
    })
}

// —— 自动推送全局设置 ——

/// 读取自动推送全局开关（Spec 2）。走 `State<AppState>` 直连 store，
/// registry 启动失败时依旧可用（与传输设置命令同一模式）。
#[tauri::command]
pub(crate) fn sync_auto_push_settings_get(
    state: State<'_, AppState>,
) -> Result<AutoPushSettings, AppError> {
    state.store.auto_push_settings().map_err(AppError::internal)
}

/// 更新自动推送全局开关，返回落库后的值。
#[tauri::command]
pub(crate) fn sync_auto_push_settings_set(
    state: State<'_, AppState>,
    master: bool,
    notify: bool,
) -> Result<AutoPushSettings, AppError> {
    state
        .store
        .update_auto_push_settings(master, notify)
        .map_err(AppError::internal)
}

// —— 窗口接入 ——

#[tauri::command]
pub(crate) async fn open_lan_sync(app: AppHandle) -> Result<(), AppError> {
    crate::window::open_lan_sync_window(&app).map_err(AppError::from)
}

// —— payload 装配（v4 lan_send_clip 原样迁移）——

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

    /// 历史条目已加入分类：发送时携带分类名/颜色（接收端据此匹配/创建分类）。
    /// 覆盖用户报告的场景——A 端条目属于分类，B 端应收到分类信息并把条目放入分类。
    #[test]
    fn build_item_send_carries_category_for_joined_clip() {
        use crate::store::test_support::{create_category, temp_store};

        let store = temp_store();
        let conn = store.connect().unwrap();

        // 手工插入一条已知 id 的历史 clip，并把 category_items.clip_snapshot_id 指向它。
        let clip_id = crate::util::new_id();
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
                crate::util::new_id(),
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

        let clip_id = crate::util::new_id();
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
        let item_id = crate::util::new_id();
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

    /// 分组条目与所声明分组不符：报错（前端传错 id 的防御）。
    #[test]
    fn build_category_item_send_rejects_foreign_category() {
        use crate::store::test_support::{create_category, temp_store};

        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat_a = create_category(&conn, "A", "#111111", 0);
        let cat_b = create_category(&conn, "B", "#222222", 1);
        let item_id = crate::util::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, 'snap', 'text', ?3, NULL, ?4, ?4, 0, ?5, ?5, 'local', 0)",
            rusqlite::params![item_id, cat_a, crate::util::hash_text("x"), "x", now],
        )
        .unwrap();

        let error = build_category_item_send(&store, &item_id, &cat_b).unwrap_err();
        assert_eq!(error, "条目不属于该分组");
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

        let clip_id = crate::util::new_id();
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
                crate::util::new_id(),
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
                String::from_utf8(payload.clone()).unwrap(),
                category_name.unwrap(),
                category_color,
                display_name,
                None,
            )
            .unwrap();
        assert_eq!(received.category_id, cat_id, "同名分类应被复用");
    }
}
