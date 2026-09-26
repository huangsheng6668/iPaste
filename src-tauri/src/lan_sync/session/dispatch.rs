//! 会话内入站帧分发（Task 33 从 lan_sync/session.rs 拆出）：
//! 单个帧的分类处理与批次状态维护；会话生命周期与事件出口留在 mod.rs。

use std::sync::Arc;

use tokio::io::AsyncWrite;

use crate::lan_sync::frame::FrameWriter;
use crate::lan_sync::protocol::{LanMessage, LAN_BATCH_MAX_ITEMS};

use super::apply::apply_received;
use super::{
    emit_category_received, emit_clip_receive_failed, read_current_payload, SessionCtx,
    MAX_CATEGORY_COLOR_LEN, MAX_CATEGORY_NAME_LEN,
};

pub(super) struct BatchState {
    category_name: String,
    category_color: Option<String>,
    /// 第一条新条目的 sort_order（= 现有最小 sort_order - 预计条目数），
    /// 之后每条 +1，保证新条目整体排在现有条目之上且保持发送顺序。
    base_order: i64,
    next_index: i64,
    received: u32,
    failed: u32,
}
/// 校验对端发来的分组元数据；Err(原因) 表示该帧应拒收（会话不断开）。
pub(super) fn validate_category_meta(
    name: Option<&str>,
    color: Option<&str>,
) -> Result<(), String> {
    if let Some(n) = name {
        if n.chars().count() > MAX_CATEGORY_NAME_LEN {
            return Err(format!("分组名不能超过 {MAX_CATEGORY_NAME_LEN} 个字符"));
        }
    }
    if let Some(c) = color {
        if c.chars().count() > MAX_CATEGORY_COLOR_LEN {
            return Err(format!("分组颜色不能超过 {MAX_CATEGORY_COLOR_LEN} 个字符"));
        }
    }
    Ok(())
}

/// 处理一条入站帧。返回 `false` 表示该帧要求结束会话（对端 Disconnect / 回写失败）。
///
/// 由会话的**读任务**逐条调用（读到即原地处理）；`write_half` 是与主循环共享的
/// 写半（仅回 `ClipResponse`/`Pong` 时短暂加锁）。
pub(super) async fn handle_frame<W>(
    frame: (LanMessage, Option<Vec<u8>>),
    ctx: &SessionCtx,
    write_half: &Arc<tokio::sync::Mutex<FrameWriter<W>>>,
    batch: &mut Option<BatchState>,
) -> bool
where
    W: AsyncWrite + Unpin,
{
    match frame {
        (
            LanMessage::CategoryBatchStart {
                category_name,
                category_color,
                item_count,
            },
            _,
        ) => {
            if let Err(reason) =
                validate_category_meta(Some(&category_name), category_color.as_deref())
            {
                emit_clip_receive_failed(ctx, reason);
                // 拒收该 BatchStart：不进入新的批量态（已存在的旧批量态保持不变——其元数据
                // 已在各自的 BatchStart 处通过校验）；后续逐条帧若无批量态则走单条路径并被再次校验。
                return true;
            }
            // 预排 sort_order：新条目整体插到现有条目之上，且按发送顺序排列。
            // item_count 由对端提供，封顶 LAN_BATCH_MAX_ITEMS 防偏移被放大。
            let min_order = ctx
                .store
                .category_min_sort_order(&category_name)
                .unwrap_or(0);
            let cap = i64::from(item_count.min(LAN_BATCH_MAX_ITEMS));
            *batch = Some(BatchState {
                category_name,
                category_color,
                base_order: min_order - cap,
                next_index: 0,
                received: 0,
                failed: 0,
            });
        }
        (LanMessage::CategoryBatchEnd, _) => {
            if let Some(b) = batch.take() {
                emit_category_received(ctx, b.category_name, b.received, b.failed);
            }
        }
        (
            LanMessage::ClipPush {
                clip_type,
                empty,
                category_name,
                category_color,
                display_name,
                auto,
                origin_node_id,
            },
            payload,
        ) => {
            if !empty {
                if let Err(reason) =
                    validate_category_meta(category_name.as_deref(), category_color.as_deref())
                {
                    emit_clip_receive_failed(ctx, reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    match batch.as_mut() {
                        // 批量中：静默逐条落库（顺序预排），结束时统一 emit。
                        // 批量（分组发送）恒手动路径：auto=false、无 origin。
                        // 守卫 `!auto`：发送侧虽有批量忙标记防 auto 帧插入批量
                        //（registry），此处再兜底——即使 auto 帧因任何原因混进
                        // 批量中段，也绝不被折叠进打开的分组（那会污染对端用户
                        // 分拣的分组），一律走下面的单条路径按 auto 语义处理。
                        Some(b) if !auto => {
                            let order = Some(b.base_order + b.next_index);
                            b.next_index += 1;
                            let ok = apply_received(
                                ctx,
                                &ctx.store,
                                &clip_type,
                                &data,
                                Some(b.category_name.clone()),
                                b.category_color.clone(),
                                display_name,
                                order,
                                true,
                                false,
                                None,
                            );
                            if ok {
                                b.received += 1
                            } else {
                                b.failed += 1
                            }
                        }
                        _ => {
                            apply_received(
                                ctx,
                                &ctx.store,
                                &clip_type,
                                &data,
                                category_name,
                                category_color,
                                display_name,
                                None,
                                false,
                                auto,
                                origin_node_id.as_deref(),
                            );
                        }
                    }
                }
            }
        }
        (LanMessage::ClipRequest, _) => match read_current_payload() {
            Ok(Some((ct, data))) => {
                let empty = false;
                let msg = LanMessage::ClipResponse {
                    clip_type: ct,
                    empty,
                    category_name: None,
                    category_color: None,
                    display_name: None,
                };
                let mut wh = write_half.lock().await;
                if wh.write_message(&msg, Some(&data)).await.is_err() {
                    return false;
                }
            }
            Ok(None) | Err(_) => {
                let msg = LanMessage::ClipResponse {
                    clip_type: "text".into(),
                    empty: true,
                    category_name: None,
                    category_color: None,
                    display_name: None,
                };
                let mut wh = write_half.lock().await;
                let _ = wh.write_message(&msg, None).await;
            }
        },
        (
            LanMessage::ClipResponse {
                clip_type,
                empty,
                category_name,
                category_color,
                display_name,
            },
            payload,
        ) => {
            if !empty {
                if let Err(reason) =
                    validate_category_meta(category_name.as_deref(), category_color.as_deref())
                {
                    emit_clip_receive_failed(ctx, reason);
                    return true; // 拒收该帧，会话继续
                }
                if let Some(data) = payload {
                    // ClipResponse（对端应答「请求剪贴板」）恒手动路径：auto=false、无 origin。
                    apply_received(
                        ctx,
                        &ctx.store,
                        &clip_type,
                        &data,
                        category_name,
                        category_color,
                        display_name,
                        None,
                        false,
                        false,
                        None,
                    );
                }
            }
        }
        // 心跳：收到 Ping 立即回 Pong（持锁写半，与控制写互斥但不长期争用）。
        (LanMessage::Ping, _) => {
            let mut wh = write_half.lock().await;
            if wh.write_message(&LanMessage::Pong, None).await.is_err() {
                return false;
            }
        }
        // Pong：对端对我方 Ping 的应答，无需处理（无超时判定——连接死亡由
        // iroh 连接层与 dead watcher 兜底）。
        (LanMessage::Pong, _) => {}
        (LanMessage::Disconnect, _) => {
            // 对端主动发来 Disconnect 帧：读任务退出并经 peer_gone 通知主循环；
            // 会话级状态清理（DeviceStatusChanged 等）由 Task 7 的 registry 承担。
            return false;
        }
        // PairRequest/PairAccept/PairReject 不应出现在会话期（配对在首条流完成，
        // 会话流是配对成功后新开的流），忽略。
        _ => {}
    }
    true
}
