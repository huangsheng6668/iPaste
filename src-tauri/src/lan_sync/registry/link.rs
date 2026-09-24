//! 链路域（原 registry.rs「重拨任务与会话」段整体迁入，纯移动）：每设备后台
//! 重拨状态机（link_task：拨号 → 会话 → 断开 → 指数退避重拨）、拨号（dial）、
//! 会话登记与准入复验（run_session）及 gen-aware 状态写（set_status_if_owner）。

use std::sync::Arc;
use std::time::Duration;

use iroh::endpoint::{Connection, RecvStream, SendStream, VarInt};
use iroh::{EndpointAddr, RelayUrl, TransportAddr};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{mpsc, oneshot};

use crate::lan_sync::protocol::IPASTE_ALPN;
use crate::lan_sync::session::{run_session_loop, SessionCtx};
use crate::models::DeviceOnline;

use super::{
    DeviceLinkRegistry, LinkHandle, CONNECT_TIMEOUT, endpoint_id_from_hex, hex_encode_32,
    send_stream_opener,
};

/// 重拨退避序列：5s→10s→20s→40s→80s→160s，之后恒为 300s（spec §5）。
const RECONNECT_BACKOFF: [Duration; 6] = [
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
    Duration::from_secs(40),
    Duration::from_secs(80),
    Duration::from_secs(160),
];
const RECONNECT_BACKOFF_CAP: Duration = Duration::from_secs(300);

/// 入站会话在线时 link_task 的轮询间隔：对端会话死亡后由下一轮接管重拨。
const INBOUND_SESSION_POLL: Duration = Duration::from_secs(5);

/// 第 N 次连续失败后的退避时长（0 基）。纯函数，独立测试。
pub(super) fn reconnect_backoff(attempt: usize) -> Duration {
    RECONNECT_BACKOFF.get(attempt).copied().unwrap_or(RECONNECT_BACKOFF_CAP)
}

impl DeviceLinkRegistry {

    /// link_task 专属的状态写：仅当登记仍是认领时的代次（gen 相同）且无活跃
    /// 会话（control_tx 为 None）才写，否则 no-op。
    ///
    /// 为什么必须 gen-aware：对端重拨落地时，入站 run_session 的收编分支会把
    /// 登记原子地换成新 gen + 新 control_tx（状态 Connected）。若无条件写状态
    ///（如会话结束后的 Offline），会把**存活中的入站会话**的状态字段砸成
    /// Offline——status 又被 has_live_session 当作在线判据时，link_task 误判
    /// 「无会话」而重拨，双向互踢以 5s 节奏永久震荡。gen 不匹配 = 登记已易主。
    pub(super) fn set_status_if_owner(&self, node_id: &str, gen: u64, status: DeviceOnline) {
        {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            let Some(handle) = links.get_mut(node_id) else { return };
            if handle.gen != gen || handle.control_tx.is_some() {
                return; // 登记已易主（新会话收编）或有活跃会话：不覆写
            }
            if handle.status == status {
                return; // 状态未变：不重复 emit
            }
            handle.status = status;
        }
        self.emit_status(node_id, status);
    }

    // —— 重拨任务与会话（spec §5）——

    /// 为已配对设备启动后台重拨任务（links 里已有该设备登记则不重复启动）。
    /// 占位与 spawn 在同一临界区内完成，防并发重复启动。
    pub(super) fn spawn_link_task(self: &Arc<Self>, node_id: String) {
        let mut links = self.inner.links.lock().expect("links 锁中毒");
        if links.contains_key(&node_id) {
            return;
        }
        let registry = self.clone();
        let task_node = node_id.clone();
        // tokio::spawn 是同步调用，持锁期间调用不违反「无 await 持锁」纪律
        let task = tokio::spawn(async move { registry.link_task(task_node).await });
        links.insert(
            node_id,
            LinkHandle {
                gen: self.next_gen(),
                control_tx: None,
                status: DeviceOnline::Connecting,
                batch_busy: false,
                task: Some(task),
            },
        );
    }

    /// links 里该设备是否已有「会话在线」的登记（Connected 且控制通道存活）。
    /// 典型场景：对端拨来的入站会话收编了本端登记。
    /// 判据是 **control_tx 存活**（活跃会话的 sender 在登记里），不看 status——
    /// status 只是展示层快照，任何遗留/并发的非 gen-aware 写都可能让它短暂失真，
    /// 拿它当在线判据会重新打开互踢震荡的口子。
    pub(super) fn has_live_session(&self, node_id: &str) -> bool {
        let links = self.inner.links.lock().expect("links 锁中毒");
        links.get(node_id).is_some_and(|handle| handle.control_tx.is_some())
    }

    /// 每设备后台任务：循环「拨号 → 会话 → 断开 → 退避重拨」（spec §5）。
    /// 每轮从 store 刷新信任态与地址线索（运行中可能被更新/撤销）。
    pub(super) async fn link_task(self: Arc<Self>, node_id: String) {
        let mut attempt: usize = 0;
        // store 查不到（已删除）或已撤销 → 循环结束（撤销即断链）
        while let Some(device) = self.inner.store.get_paired_device(&node_id).ok().flatten() {
            if device.revoked_at.is_some() {
                break;
            }
            // 对端拨来的会话已在本端在线（收编后的入站登记）：跳过本端拨号。
            // 否则双向 link_task 互踢——A 重拨收编 B 的会话 → B 的会话死 →
            // B 重拨收编 A 的会话 → ……以 5s 一次的节奏永久连接抖动。
            // 对端会话死亡（control_tx 清空）后由下一轮循环自然接管重拨。
            if self.has_live_session(&node_id) {
                tokio::time::sleep(INBOUND_SESSION_POLL).await;
                continue; // 不增加退避计数：这不是拨号失败
            }
            // 认领当前登记（无活跃会话时的 gen）作为本轮状态写的所有权凭据。
            // 拨号期间（最长 15s）若有入站会话收编登记，gen 变化 → 状态写自动 no-op。
            let claim_gen = {
                let links = self.inner.links.lock().expect("links 锁中毒");
                links
                    .get(&node_id)
                    .filter(|handle| handle.control_tx.is_none())
                    .map(|handle| handle.gen)
            };
            if let Some(gen) = claim_gen {
                self.set_status_if_owner(&node_id, gen, DeviceOnline::Connecting);
            }
            match self
                .dial(&node_id, device.relay_url.as_deref(), &device.direct_addrs)
                .await
            {
                Ok((conn, send, recv)) => {
                    attempt = 0;
                    self.inner.store.touch_last_seen(&node_id).ok();
                    if let Some(gen) = claim_gen {
                        self.set_status_if_owner(&node_id, gen, DeviceOnline::Connected);
                    }
                    let dead_rx = Self::watch_conn_death(conn.clone());
                    // run_session 按值拿走 Arc：clone 一份给本次会话；
                    // 返回本会话的 gen，供会话结束后的 Offline 写做所有权校验
                    let session_gen = self
                        .clone()
                        .run_session(Some(conn.clone()), node_id.clone(), recv, send, dead_rx)
                        .await;
                    conn.close(VarInt::from_u32(0), b"session-end");
                    // 会话结束：只有登记仍是本会话的 gen（未被入站会话收编）才置
                    // Offline；被收编则 no-op——存活的入站会话不受影响，link_task
                    // 在循环顶的 guard 处停靠，不再重拨（Fix：互踢震荡）。
                    self.set_status_if_owner(&node_id, session_gen, DeviceOnline::Offline);
                }
                Err(reason) => {
                    eprintln!("[lan-sync] 拨号 {node_id} 失败：{reason}");
                    // 拨号失败同样回到 Offline（凭据是认领 gen；期间被收编则 no-op）
                    if let Some(gen) = claim_gen {
                        self.set_status_if_owner(&node_id, gen, DeviceOnline::Offline);
                    }
                }
            }
            tokio::time::sleep(reconnect_backoff(attempt)).await;
            attempt += 1;
        }
        // 任务退出：清掉自己的登记（仅当无活跃会话占用时）
        let mut remove = false;
        {
            let links = self.inner.links.lock().expect("links 锁中毒");
            if links
                .get(&node_id)
                .is_some_and(|handle| handle.control_tx.is_none())
            {
                remove = true;
            }
        }
        if remove {
            self.inner.links.lock().expect("links 锁中毒").remove(&node_id);
            self.emit_device_list();
        }
    }

    /// 组装 EndpointAddr（直连 + 中继）并拨号；成功后由调用方开会话流。
    /// 对端元数据回写跳过（iroh 连接信息取不到对端新地址，保留 store 里
    /// 票据/历史线索即可——brief 允许）。
    async fn dial(
        &self,
        node_id: &str,
        relay: Option<&str>,
        addrs: &[String],
    ) -> Result<(Connection, SendStream, RecvStream), String> {
        let Some(peer_id) = endpoint_id_from_hex(node_id) else {
            return Err(format!("设备标识无效：{node_id}"));
        };
        let mut transports: Vec<TransportAddr> = addrs
            .iter()
            .filter_map(|addr| addr.parse::<std::net::SocketAddr>().ok().map(TransportAddr::Ip))
            .collect();
        if let Some(relay) = relay {
            if let Ok(url) = relay.parse::<RelayUrl>() {
                transports.push(TransportAddr::Relay(url));
            }
        }
        if transports.is_empty() {
            return Err("没有可用的连接地址".to_string());
        }
        let addr = EndpointAddr::from_parts(peer_id, transports);
        let conn = tokio::time::timeout(CONNECT_TIMEOUT, self.inner.endpoint.connect(addr, IPASTE_ALPN))
            .await
            .map_err(|_| "连接超时".to_string())?
            .map_err(|e| e.to_string())?;
        let (mut send, recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
        // 拨号方首发帧：对端（已配对入站分支）的 accept_bi 才会解除挂起
        send_stream_opener(&mut send).await?;
        Ok((conn, send, recv))
    }

    /// 注册并运行一个会话（入站/配对/重拨共用入口）。收编语义：同设备已有旧
    /// 登记时，旧 control_tx 随替换被 drop → 旧会话干净关闭；重拨任务句柄继承。
    /// 返回本会话的 gen（调用方 link_task 以此做会话后状态写的所有权校验）。
    ///
    /// `conn`（v0.9.2 A1）供登记被拒时以应用层原因关闭整条连接；测试的 duplex
    /// 流没有真实连接，传 None——流随本函数返回被 drop，对端观察到 EOF。
    pub(super) async fn run_session<R, W>(
        self: Arc<Self>,
        conn: Option<Connection>,
        node_hex: String,
        read: R,
        write: W,
        dead: oneshot::Receiver<()>,
    ) -> u64
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (control_tx, control_rx) = mpsc::channel(16);
        let my_gen = self.next_gen();
        {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            // A1 登记前复验（TOCTOU）：入站分流（首帧路由）与这里的登记之间，
            // revoke/disconnect/delete 可能恰好落地——kill_link 找不到条目可杀，随后
            // Vacant 插入会让已撤销/已断开/已删除的设备顶着 Connected 幽灵会话继续
            // 收推送。在**持有 links 锁的同一临界区**内复验并登记，配合三条路径
            // 统一「状态写先行」的顺序（与 revoke 同形），任意交错都收敛干净：
            // - disconnect 先置标记 / delete 先删 store 行 / revoke 先写撤销：
            //   状态写先行 ⇒ 随后 run_session 的 admission 检查必然读到（标记
            //   或已删除/已撤销的行）→ 直接拒绝登记；
            // - 若本处赶在状态写之前抢到 links 锁完成登记 ⇒ 随后被阻塞在锁上的
            //   kill_link 兜底清理已存在的登记。
            // 临界区内嵌套 disconnected 锁与 store 读是安全的：全仓只有本处以
            // links → disconnected 顺序嵌套，无反向路径；store 不回调 registry
            // 锁。会话建立低频，临界区内一次 store 读可接受。
            if let Some(reason) = self.session_admission_denied(&node_hex) {
                if let Some(conn) = &conn {
                    conn.close(VarInt::from_u32(0), reason);
                }
                // control_tx / 流随返回 drop：不发 Connected、不留登记
                return my_gen;
            }
            use std::collections::hash_map::Entry;
            match links.entry(node_hex.clone()) {
                // 已有登记：收编（旧 control_tx 在赋值时 drop → 旧会话收到 None 干净关闭；
                // 旧重拨任务保留，不 abort）。批量忙标记随收编复位——旧通道的
                // 批量（若有）已随旧会话死亡，新通道上必然无批量在途。
                Entry::Occupied(mut occupied) => {
                    let handle = occupied.get_mut();
                    handle.gen = my_gen;
                    handle.control_tx = Some(control_tx);
                    handle.status = DeviceOnline::Connected;
                    handle.batch_busy = false;
                }
                Entry::Vacant(vacant) => {
                    vacant.insert(LinkHandle {
                        gen: my_gen,
                        control_tx: Some(control_tx),
                        status: DeviceOnline::Connected,
                        batch_busy: false,
                        task: None,
                    });
                }
            }
        }
        self.emit_status(&node_hex, DeviceOnline::Connected);
        self.emit_device_list();
        let peer_device_name = self
            .inner
            .store
            .get_paired_device(&node_hex)
            .ok()
            .flatten()
            .map(|device| device.device_name)
            .unwrap_or_else(|| node_hex.chars().take(8).collect());
        let ctx = SessionCtx {
            sink: self.inner.sink.clone(),
            store: self.inner.store.clone(),
            peer_node_id: node_hex.clone(),
            peer_device_name,
            // 本机身份（origin 自环防御基准）+ registry 级 recent 单例 +
            // auto 轻提示开关（每次会话建立时重读设置，改动即时生效于新会话）。
            local_node_id: hex_encode_32(self.inner.endpoint.id().as_bytes()),
            recent: self.inner.recent.clone(),
            auto_notify: self
                .inner
                .store
                .auto_push_settings()
                .map(|settings| settings.notify)
                .unwrap_or(false),
            // 追加复制会话状态（与 watcher 共享同一实例）：auto 接收据此跳过
            // 剪贴板写，防对端内容被 merge 进本地追加缓冲。
            append_copy_state: self.inner.append_state.clone(),
        };
        run_session_loop(read, write, ctx, control_rx, dead).await;
        // 会话结束：只清理仍属于自己的登记（gen 相同）；被新会话收编则不动。
        // task 存活（is_finished=false）才有重拨接管者；配对/入站建立的会话没有
        // 任务，或继承的任务已退出（如撤销导致 link_task break）——都走移除+重起。
        let mut remove_entry = false;
        let mut owned = false;
        {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            if let Some(handle) = links.get_mut(&node_hex) {
                if handle.gen == my_gen {
                    owned = true;
                    let task_alive = handle
                        .task
                        .as_ref()
                        .is_some_and(|task| !task.is_finished());
                    if task_alive {
                        // 有重拨任务接管：登记保留，状态置 Offline，任务继续循环
                        handle.control_tx = None;
                        handle.status = DeviceOnline::Offline;
                    } else {
                        remove_entry = true; // 无接管任务：移除登记，稍后按需重起
                    }
                }
            }
        }
        if remove_entry {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            // 复查 gen：两段锁之间可能已被新会话收编
            if links.get(&node_hex).is_some_and(|handle| handle.gen == my_gen) {
                links.remove(&node_hex);
            }
        }
        // Offline 状态事件只在登记仍属于本会话时发：被收编意味着同一设备的
        // 新会话已在线（收编登记时已发过 Connected），再发 Offline 是假事件。
        if owned {
            self.emit_status(&node_hex, DeviceOnline::Offline);
        }
        self.emit_device_list();
        // 无重拨任务的会话（配对建立）结束：若设备仍可信则起任务接管后续重拨
        if remove_entry && self.inner.store.is_trusted(&node_hex).unwrap_or(false) {
            self.spawn_link_task(node_hex);
        }
        my_gen
    }
}
