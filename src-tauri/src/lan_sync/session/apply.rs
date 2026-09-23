//! 接收应用层决策（session 子模块）：对端推来的剪贴板内容如何落到本地——
//! 文本/链接/颜色/HTML 直接入库，图片额外落盘成文件路径；历史条目按 auto
//! 分流决定是否回写系统剪贴板（追加合并期间跳过），分组条目只进
//! category_items、不触碰剪贴板。会话编排在父模块 session.rs。

use super::{emit, emit_clip_receive_failed, SessionCtx};
use crate::clipboard::{
    captured_item_from_payload, write_clipboard_image, write_clipboard_text,
};
use crate::events::{DeviceClipReceived, EVENT_DEVICE_CLIP_RECEIVED};
use crate::models::AppendCopyState;
use crate::store::Store;
use crate::util::clean_display_name;

/// 追加复制会话是否活跃（is_enabled 且已有 session）：活跃期间本机 watcher
/// 会把新剪贴板内容 merge 进追加缓冲——auto 接收此刻不得写剪贴板，否则
/// 对端内容会被并进本地用户的追加文本（内容仍落历史 + recent，同步不缺）。
pub(super) fn append_session_active(state: &AppendCopyState) -> bool {
    state.is_enabled && state.session_id.is_some()
}

/// 处理收到的剪贴板内容：落库 + emit（auto 分流见历史条目）
///
/// - `category_name` 为 `None`：历史/无分组条目，入 `clips`（历史）表。
///   按 `auto` 分流（spec §2/§3）：
///   - `auto=true`（对端捕获即自动同步）：**先落库**再登记 `recent`（仅 auto 路径，
///     供发送侧防回推），随后**尽力**写系统剪贴板（失败只发诊断事件、不推翻同步
///     结果——无头 CI/剪贴板被占用时同步本身不受影响）；追加复制会话活跃期间
///     跳过剪贴板写（防对端内容被 merge 进本地追加缓冲）。剪贴板写成败均按
///     `auto_notify` 决定是否 emit 轻提示（内容已在历史可用，提示不该被写失败吞掉）。
///   - `auto=false`（手动发送）：落库 + 提示，**不接管本机剪贴板**（v4 行为变更，
///     spec §2：手动收到的内容进历史即可，覆盖用户当前剪贴板反而打扰）。
///   - origin 自环防御（spec §3 第三道）：`auto=true` 且 `origin_node_id` 是本机
///     （自己发出的内容绕回）时静默丢弃——无行、无 recent、无事件。
/// - `category_name` 为 `Some`：分组条目。**不写系统剪贴板**（避免打扰用户当前
///   剪贴板），落到匹配/新建的同名分组下的 `category_items` 表；`auto` 参数
///   对该路径无效（自动同步只推历史条目，spec §1）。
///
/// `display_name`：对端条目的重命名显示名（历史条目落到 `clips.display_name`，
/// 分组条目落到 `category_items.display_name`）。
/// `sort_order`：整组接收时预排的分组内顺序（保持发送顺序）；`None` 走旧行为
/// （插到分组顶部）。
/// `silent`：整组接收时为 true——不逐条 emit 事件，由调用方在批量结束时汇总。
/// 返回是否成功落库（供批量统计）。
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_received(
    ctx: &SessionCtx,
    store: &Store,
    clip_type: &str,
    payload: &[u8],
    category_name: Option<String>,
    category_color: Option<String>,
    display_name: Option<String>,
    sort_order: Option<i64>,
    silent: bool,
    auto: bool,
    origin_node_id: Option<&str>,
) -> bool {
    // 图片 = data url（utf-8）；文本 = 原文 utf-8
    let text = String::from_utf8_lossy(payload).to_string();
    let mut item = match captured_item_from_payload(clip_type, &text) {
        Ok(Some(item)) => item,
        // 空文本（trim 后为空）：不诊断（对端发空 payload 是合法的「清空」语义之外的罕见情况）
        Ok(None) => {
            if !silent {
                emit_clip_receive_failed(ctx, "收到空内容，已忽略".to_string());
            }
            return false;
        }
        // 解析失败（如图片 data url 损坏 / 对端发了本地路径读不到）：暴露原因
        Err(reason) => {
            if !silent {
                emit_clip_receive_failed(ctx, format!("解析收到的内容失败：{reason}"));
            }
            return false;
        }
    };

    // 对端重命名：清洗（trim、空 → None、超长报错）。清洗失败按该条目失败处理。
    let display_name = match clean_display_name(display_name) {
        Ok(name) => name,
        Err(reason) => {
            if !silent {
                emit_clip_receive_failed(ctx, format!("条目名称无效：{reason}"));
            }
            return false;
        }
    };
    item.display_name = display_name.clone();

    match category_name {
        // 分组条目：不触碰系统剪贴板，直接落到 category_items
        Some(name) => {
            // 图片条目：captured_item_from_payload 解码出的是内存字节（text 为空串），
            // 先落盘成文件路径（与历史条目 insert_captured_item 的处理一致），
            // 否则 B 端 category_items/clips 里存的 text 是空串，图片内容等于丢失。
            let stored_text = if clip_type == "image" {
                match item.image_bytes.as_deref() {
                    Some(bytes) => match store.save_image_bytes(&item.content_hash, bytes) {
                        Ok(path) => path,
                        Err(reason) => {
                            if !silent {
                                emit_clip_receive_failed(ctx, format!("保存图片文件失败：{reason}"));
                            }
                            return false;
                        }
                    },
                    None => item.text.clone(),
                }
            } else {
                item.text.clone()
            };
            match store.insert_received_category_item(
                item.clip_type,
                item.content_hash,
                item.preview_text,
                stored_text,
                name.clone(),
                category_color,
                display_name,
                sort_order,
            ) {
                Ok(_) => {
                    if !silent {
                        emit_clip_received(ctx, clip_type.to_string(), Some(name));
                    }
                    true
                }
                // 落库失败（DB 约束/磁盘等）：暴露原因，不再静默吞掉。
                // name 是对端可控输入（协议层仅限制帧大小），失败提示里必须截断，
                // 避免恶意对端把超长字符串灌进 UI。
                Err(reason) => {
                    if !silent {
                        let short_name: String = name.chars().take(40).collect();
                        emit_clip_receive_failed(
                            ctx,
                            format!("保存到分组「{short_name}」失败：{reason}"),
                        );
                    }
                    false
                }
            }
        }
        // 历史/无分组：按 auto 分流（spec §2/§3）
        None => {
            // origin 自环防御（spec §3 第三道）：自己发出的内容绕回，静默丢弃。
            if auto && origin_node_id == Some(ctx.local_node_id.as_str()) {
                return false;
            }
            if auto {
                // 自动同步：①落库 ②登记 recent（仅 auto 路径，Task 3 发送侧防回推
                // 消费同一实例）③尽力写剪贴板 ④不强制提示（auto_notify 只控制轻提示）。
                // 顺序刻意把落库放前：无头 CI/剪贴板被占用时同步本身不受影响。
                let hash = item.content_hash.clone();
                let item_text = item.text.clone();
                match store.insert_captured_item(item) {
                    Err(reason) => {
                        emit_clip_receive_failed(ctx, format!("保存到历史失败：{reason}"));
                        return false;
                    }
                    Ok(_) => {}
                }
                ctx.recent.insert(&hash);
                // 追加合并期间不接管剪贴板：本地用户正在追加复制会话中，此刻
                // 写剪贴板会被 watcher 捕获并 merge 进追加缓冲（对端内容混进
                // 本地合并文本）。内容已在历史 + recent，同步不受影响。
                let append_active = ctx
                    .append_copy_state
                    .lock()
                    .map(|state| append_session_active(&state))
                    .unwrap_or(false);
                let write_result = if append_active {
                    Ok(())
                } else if clip_type == "image" {
                    write_clipboard_image(&text)
                } else {
                    write_clipboard_text(item_text.trim())
                };
                match write_result {
                    Err(reason) => {
                        // 诊断但不计失败：条目已入库，剪贴板写失败不推翻同步结果。
                        // 不受 silent 门控：auto 恒单条（批量帧走 false/None），无汇总场景。
                        emit_clip_receive_failed(ctx, format!("写入系统剪贴板失败：{reason}"));
                    }
                    Ok(()) => {}
                }
                if ctx.auto_notify {
                    // 开了轻提示的用户无论剪贴板写成败都收到 toast：内容已在
                    // 历史可用，写失败也有诊断事件兜底，不该静默吞掉提示。
                    emit_clip_received(ctx, clip_type.to_string(), None);
                }
                true
            } else {
                // 手动发送：落库 + 提示，不接管本机剪贴板（v4 行为变更，spec §2）。
                match store.insert_captured_item(item) {
                    Ok(_) => {
                        if !silent {
                            emit_clip_received(ctx, clip_type.to_string(), None);
                        }
                        true
                    }
                    Err(reason) => {
                        if !silent {
                            emit_clip_receive_failed(ctx, format!("保存到历史失败：{reason}"));
                        }
                        false
                    }
                }
            }
        }
    }
}

/// 单条接收成功：emit 汇总事件（历史条目 category_name = None）。
fn emit_clip_received(ctx: &SessionCtx, clip_type: String, category_name: Option<String>) {
    emit(
        ctx,
        EVENT_DEVICE_CLIP_RECEIVED,
        &DeviceClipReceived { node_id: ctx.peer_node_id.clone(), clip_type, category_name },
    );
}
