//! 发送分发域（原 registry.rs「发送分发/捕获即扇出」段整体迁入，纯移动）：
//! 手动单条（send_raw）、整组批量（send_category，BatchStart…BatchEnd 流式）与
//! 捕获即扇出（fan_out_auto，锁内忙检查 + try_send 有损投递）。

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::mpsc;

use crate::events::{DeviceCategorySent, EVENT_DEVICE_CATEGORY_SENT};
use crate::lan_sync::autopush::fan_out_targets;
use crate::lan_sync::protocol::LAN_MAX_PAYLOAD;
use crate::lan_sync::ControlMsg;
use crate::models::{AutoSyncMode, ClipItem, DeviceOnline};

use super::{DeviceLinkRegistry, build_send_payload, hex_encode_32};

/// send_category 的单目标发送状态（流式逐条发送时的聚合账本）：
/// `started=false`（BatchStart 未达）的目标不参与后续、不发汇总事件；
/// `dead=true` 后该目标剩余条目逐条计 failed（与会话中断前的语义一致）。
struct CategorySendTarget {
    node_id: String,
    tx: mpsc::Sender<ControlMsg>,
    started: bool,
    dead: bool,
    sent: u32,
    failed: u32,
}

impl DeviceLinkRegistry {
    // —— 发送分发 ——

    /// 解析在线目标：None = 全部 Connected 链路；Some = 指定设备。
    /// 结果为空报「没有在线的目标设备」。锁内只 clone sender，不 await。
    fn online_targets(
        &self,
        target: Option<&str>,
    ) -> Result<Vec<(String, mpsc::Sender<ControlMsg>)>, String> {
        let links = self.inner.links.lock().expect("links 锁中毒");
        let mut out: Vec<(String, mpsc::Sender<ControlMsg>)> = Vec::new();
        match target {
            None => {
                for (node, handle) in links.iter() {
                    if handle.status == DeviceOnline::Connected {
                        if let Some(tx) = &handle.control_tx {
                            out.push((node.clone(), tx.clone()));
                        }
                    }
                }
            }
            Some(node_id) => {
                if let Some(handle) = links.get(node_id) {
                    if handle.status == DeviceOnline::Connected {
                        if let Some(tx) = &handle.control_tx {
                            out.push((node_id.to_string(), tx.clone()));
                        }
                    }
                }
            }
        }
        drop(links);
        if out.is_empty() {
            Err("没有在线的目标设备".to_string())
        } else {
            Ok(out)
        }
    }

    /// 置/清某链路的「批量发送进行中」标记。仅当登记的控制通道仍是本批量的
    /// 通道时生效（same_channel）：会话被收编/替换后，旧批量的清理不得误改
    /// 新会话的标记（收编时新登记已复位为 false）。
    fn mark_batch_busy(&self, node_id: &str, tx: &mpsc::Sender<ControlMsg>, busy: bool) {
        let mut links = self.inner.links.lock().expect("links 锁中毒");
        if let Some(handle) = links.get_mut(node_id) {
            let same = handle
                .control_tx
                .as_ref()
                .is_some_and(|current| current.same_channel(tx));
            if same {
                handle.batch_busy = busy;
            }
        }
    }

    /// auto 推送的单链路投递：**同一次 links 锁内**完成「批量忙检查 + try_send」。
    /// 与 send_category 的「先置忙标记、再入队 BatchStart」配合，从结构上排除
    /// auto 帧插进 BatchStart…BatchEnd 中段：锁内看到不忙 ⇒ BatchStart 尚未
    /// 入队 ⇒ 本条 auto 帧只可能排在批量之前；锁内看到忙 ⇒ 跳过并计数
    /// （auto 推送本就有损设计）。返回 true = 已入队。
    fn try_send_auto_guarded(
        &self,
        node_id: &str,
        tx: &mpsc::Sender<ControlMsg>,
        msg: ControlMsg,
    ) -> bool {
        let links = self.inner.links.lock().expect("links 锁中毒");
        if links.get(node_id).is_some_and(|handle| handle.batch_busy) {
            count_auto_drop(&self.inner.auto_dropped, &format!("目标 {node_id} 整组发送进行中"));
            return false;
        }
        // 锁内 try_send 是同步非阻塞调用，不违反「无 await 持锁」纪律。
        try_send_auto(tx, msg, &self.inner.auto_dropped)
    }

    /// 发送单条剪贴板内容（无分组语义时 category_* 传 None）。
    /// 指定设备不在线 / 无任何在线设备时报错；个别目标会话已死时跳过并记日志。
    pub(crate) async fn send_raw(
        &self,
        target: Option<&str>,
        clip_type: &str,
        payload: &[u8],
        category_name: Option<&str>,
        category_color: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<(), String> {
        let targets = self.online_targets(target)?;
        for (node_id, tx) in targets {
            let msg = ControlMsg::SendClip {
                clip_type: clip_type.to_string(),
                payload: payload.to_vec(),
                category_name: category_name.map(str::to_string),
                category_color: category_color.map(str::to_string),
                display_name: display_name.map(str::to_string),
                // 手动发送恒为非自动、无 origin（Spec 2 auto 路径由后续任务接线）
                auto: false,
                origin_node_id: None,
            };
            if tx.send(msg).await.is_err() {
                eprintln!("[lan-sync] 发送到 {node_id} 失败：会话已关闭");
            }
        }
        Ok(())
    }

    /// 整组发送某分组：`BatchStart` → 逐条 `SendClip`（携带分组名/颜色 + 重命名）→
    /// `BatchEnd`。条目逐条流式装配发送（v4 的 build_send_payload 共用，不再
    /// 预装配整个分组）；多目标时逐目标 emit `DeviceCategorySent`，返回
    /// (组名, 至少送达 1 个目标的条数, 其余计数)。
    pub(crate) async fn send_category(
        &self,
        target: Option<&str>,
        category_id: &str,
    ) -> Result<(String, u32, u32), String> {
        let online = self.online_targets(target)?;
        let conn = self.inner.store.connect()?;
        let category = self.inner.store.get_category_with_conn(&conn, category_id)?;
        let items = self
            .inner
            .store
            .list_category_items_for_category_with_conn(&conn, category_id)?;
        drop(conn);
        if items.is_empty() {
            return Err("该分组没有可发送的条目".to_string());
        }
        let item_count = items.len().min(u32::MAX as usize) as u32;
        let category_name = category.name.clone();
        let category_color = category.color.clone();
        // 逐条流式发送（不预装配全部 payload）：单条构建失败（如图片文件缺失）
        // 跳过并计数、不中断整组（v4 行为）。条目外层 / 目标内层——同一时刻
        // 内存只持一条 payload（外加各目标通道在途副本），避免 10k 条 × ~8MB
        // 的整组放大；每目标通道内的消息顺序仍是 BatchStart → 条目 → BatchEnd。
        let mut targets: Vec<CategorySendTarget> = Vec::with_capacity(online.len());
        for (node_id, tx) in online {
            // 先置「批量进行中」再入队 BatchStart（顺序关键）：fan_out_auto 的
            // 忙检查与 try_send 在同一次 links 锁内完成，标记先于 BatchStart
            // 可见 ⇒ 并发的 auto 帧只可能排在批量之前或被跳过，绝不会插进
            // 批量中段（接收端另有 belt：auto 帧永不折叠进打开的分组）。
            self.mark_batch_busy(&node_id, &tx, true);
            let started = tx
                .send(ControlMsg::BatchStart {
                    category_name: category_name.clone(),
                    category_color: Some(category_color.clone()),
                    item_count,
                })
                .await
                .is_ok();
            if !started {
                // BatchStart 未达（会话已死）：回滚标记，防忙碌标记永久滞留
                self.mark_batch_busy(&node_id, &tx, false);
            }
            // BatchStart 都送不达（会话已死）的目标：跳过且不发汇总事件（v4 行为）
            targets.push(CategorySendTarget {
                node_id,
                tx,
                started,
                dead: !started,
                sent: 0,
                failed: 0,
            });
        }
        let mut build_failed: u32 = 0;
        let mut delivered_any_count: u32 = 0; // 至少送达 1 个目标的条数（跨目标聚合）
        for item in &items {
            let payload = match build_send_payload(&item.clip_type, &item.text) {
                Ok(payload) => payload,
                Err(reason) => {
                    eprintln!("[lan-sync] 整组发送跳过条目 {}：{reason}", item.id);
                    build_failed += 1;
                    for target in &mut targets {
                        target.failed += 1; // 构建失败对所有目标计失败
                    }
                    continue;
                }
            };
            let mut delivered_this = false;
            for target in &mut targets {
                if target.dead {
                    target.failed += 1; // 会话已断：该目标剩余条目逐条计失败
                    continue;
                }
                let msg = ControlMsg::SendClip {
                    clip_type: item.clip_type.clone(),
                    payload: payload.clone(),
                    category_name: Some(category_name.clone()),
                    category_color: Some(category_color.clone()),
                    display_name: item.display_name.clone(),
                    // 整组发送为手动操作：非自动、无 origin
                    auto: false,
                    origin_node_id: None,
                };
                if target.tx.send(msg).await.is_ok() {
                    target.sent += 1;
                    delivered_this = true;
                } else {
                    // 通道关闭（会话已断）：该目标剩余条目必然失败，停发
                    target.dead = true;
                    target.failed += 1;
                }
            }
            if delivered_this {
                delivered_any_count += 1;
            }
        }
        for target in &mut targets {
            if !target.started {
                continue;
            }
            let _ = target.tx.send(ControlMsg::BatchEnd).await;
            // BatchEnd 已入队后清「批量进行中」：此后入队的 auto 帧只可能排在
            // BatchEnd 之后（安全）。窗口内 fan_out 仍会跳过——auto 有损，无害。
            self.mark_batch_busy(&target.node_id, &target.tx, false);
            self.emit(
                EVENT_DEVICE_CATEGORY_SENT,
                &DeviceCategorySent {
                    node_id: target.node_id.clone(),
                    category_name: category_name.clone(),
                    sent: target.sent,
                    failed: target.failed,
                },
            );
        }
        let sent = delivered_any_count;
        let failed = item_count.saturating_sub(sent);
        Ok((category_name, sent, failed))
    }

    /// 请求指定设备回推它当前的剪贴板内容。
    pub(crate) async fn request_clip(&self, node_id: &str) -> Result<(), String> {
        let targets = self.online_targets(Some(node_id))?;
        for (_, tx) in targets {
            let _ = tx.send(ControlMsg::RequestClip).await;
        }
        Ok(())
    }

    // —— 捕获即扇出（Spec 2 发送侧）——

    /// 捕获即扇出（spec §1）：master 开关 → recent 命中跳过（回环第一道）→
    /// 无在线候选跳过 → payload 构建（超限跳过）→ 在线目标两段式过滤 →
    /// 锁内忙检查 + try_send（批量进行中/队列满均丢弃计数，绝不阻塞捕获）。
    /// Err 仅在 payload 构建失败时返回；其余抑制路径一律静默 Ok
    /// （同步不得拖垮捕获路径）。
    pub(crate) async fn fan_out_auto(&self, clip: &ClipItem) -> Result<(), String> {
        // 闸门 1：master 总开关关闭——短路在 payload 构建之前（不读图片文件）。
        let settings = self.inner.store.auto_push_settings()?;
        if !settings.master {
            return Ok(());
        }
        // 闸门 2：recent 命中（回环窗口内刚从对端收到的内容）——同样短路在
        // payload 构建之前，防回推的同时省掉图片读盘。
        if self.inner.recent.contains(&clip.content_hash) {
            return Ok(());
        }
        // 闸门 3：无在线候选——同样短路在 payload 构建之前（无人在线时不读
        // 图片文件、不做 base64）。auto 推送静默语义，此处静默 Ok（与手动
        // 发送的「没有在线的目标设备」报错口径不同）。
        // 两段式锁纪律：第一段在 links 锁内只收集 Connected 链路的 (node, 控制通道)，
        // 仅 clone sender——锁内无 SQLite/IO（无 await 纪律 + 最小化锁持有）。
        let candidates: Vec<(String, mpsc::Sender<ControlMsg>)> = {
            let links = self.inner.links.lock().expect("links 锁中毒");
            links
                .iter()
                .filter(|(_, handle)| handle.status == DeviceOnline::Connected)
                .filter_map(|(node, handle)| {
                    handle.control_tx.as_ref().map(|tx| (node.clone(), tx.clone()))
                })
                .collect()
        };
        if candidates.is_empty() {
            return Ok(());
        }
        // payload 规则与手动 send_raw 一致：image = 文件读出转 data url
        // （clip.text 为落盘路径），其余 = text 原文。图片超限在 build_send_payload
        // 内被拒（Err）；文本类超出 LAN_MAX_PAYLOAD 则跳过本次扇出（非错误）。
        let payload = build_send_payload(&clip.clip_type, &clip.text)?;
        if payload.len() > LAN_MAX_PAYLOAD {
            eprintln!(
                "[auto-push] payload 超出单帧上限（{} 字节），跳过本次自动推送",
                payload.len()
            );
            return Ok(());
        }
        // 第二段在锁外查每设备偏好（同步 SQLite），过滤交给纯函数 fan_out_targets。
        // store 无行（如设备刚被删除）按 TextOnly 兜底。
        let modes: Vec<(String, AutoSyncMode)> = candidates
            .iter()
            .map(|(node, _)| {
                let mode = self
                    .inner
                    .store
                    .get_paired_device(node)
                    .ok()
                    .flatten()
                    .map(|device| device.auto_sync_mode)
                    .unwrap_or(AutoSyncMode::TextOnly);
                (node.clone(), mode)
            })
            .collect();
        let allowed: HashSet<String> =
            fan_out_targets(&modes, &clip.clip_type).into_iter().collect();
        let my_id = hex_encode_32(self.inner.endpoint.id().as_bytes());
        for (node, tx) in candidates {
            if !allowed.contains(&node) {
                continue; // 该设备偏好不接收此类型：不发
            }
            let msg = ControlMsg::SendClip {
                clip_type: clip.clip_type.clone(),
                payload: payload.clone(),
                // 捕获路径无分组/重命名语义（brief 约定）
                category_name: None,
                category_color: None,
                display_name: None,
                auto: true,
                origin_node_id: Some(my_id.clone()),
            };
            // 批量忙检查 + try_send 在 links 锁内原子完成（见 try_send_auto_guarded）：
            // 队列满/会话已死/批量进行中均计数丢弃，不重试、不等待。
            let _ = self.try_send_auto_guarded(&node, &tx, msg);
        }
        Ok(())
    }
}

/// auto 推送丢弃的统一计数 + 日志（队列满/会话已死/批量进行中共用口径）。
fn count_auto_drop(dropped: &AtomicU64, reason: &str) {
    let count = dropped.fetch_add(1, Ordering::Relaxed) + 1;
    eprintln!("[auto-push] {reason}，丢弃第 {count} 条自动推送");
}

/// auto 推送的投递原语：try_send 入队，队列满或会话已死时经 `dropped` 计数
/// 后丢弃并记日志——同步等待容量，是「扇出绝不阻塞捕获」的核心保证
///（fan_out_auto 消费）。返回 true = 已入队。
pub(super) fn try_send_auto(
    tx: &mpsc::Sender<ControlMsg>,
    msg: ControlMsg,
    dropped: &AtomicU64,
) -> bool {
    match tx.try_send(msg) {
        Ok(()) => true,
        Err(_) => {
            count_auto_drop(dropped, "目标队列满或会话已关闭");
            false
        }
    }
}

