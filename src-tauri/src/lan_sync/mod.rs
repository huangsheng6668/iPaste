//! lan_sync 模块根：v5 明文协议（iroh QUIC TLS 承担传输加密）。
//!
//! v4 TCP 会话状态机（LanSessionManager/LanRole/LanStatus 等）已在协议 v5
//! 迁移中移除；会话编排由 DeviceLinkRegistry（iroh 端点 + 连接登记）承担，
//! 命令层见 commands.rs（lib.rs setup 构造并 manage registry）。

pub(crate) mod protocol;
pub(crate) mod session;    // v5 泛型明文会话循环（registry 接线）
pub(crate) mod commands;   // Tauri 命令层：设备管理 + 票据配对 + 定向发送
pub(crate) mod pair_guard; // 配对防爆破（registry 配对门消费）
pub(crate) mod identity;   // 设备身份（iroh SecretKey）
pub(crate) mod frame;      // 泛型帧编解码（iroh 无耦合）
pub(crate) mod ticket;     // 配对票据 + 一次性邀请登记
pub(crate) mod registry;   // DeviceLinkRegistry：iroh 端点 + 每设备连接管理（命令层消费）
pub(crate) mod autopush;   // 自动推送基座：类型过滤矩阵 + 回环抑制滑窗（Spec 2 Task 2/3 消费）
#[cfg(test)]
mod integration_tests; // 双 endpoint 全链路集成测试（QUIC 回环，Task 9）

pub use registry::DeviceLinkRegistry;

use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, Emitter};
use ts_rs::TS;

/// 事件出口抽象：生产环境转发到 Tauri 前端；测试用 Noop（不构造任何
/// 窗口运行时，避免把 tao/wry 的 GUI 原生代码链接进测试二进制）。
pub(crate) trait LanEventSink: Send + Sync + 'static {
    fn emit(&self, event: &str, payload: &serde_json::Value);
}

/// 生产事件出口：经真实 AppHandle emit 到前端。
/// lib.rs setup 经 `tauri_event_sink` 构造并注入各组件。
pub(crate) struct TauriEventSink {
    app: AppHandle,
}

impl LanEventSink for TauriEventSink {
    fn emit(&self, event: &str, payload: &serde_json::Value) {
        let _ = self.app.emit(event, payload);
    }
}

/// 生产事件出口构造器（lib.rs setup 接线消费）。
pub(crate) fn tauri_event_sink(app: AppHandle) -> Arc<dyn LanEventSink> {
    Arc::new(TauriEventSink { app })
}

/// 测试事件出口：空操作。
#[allow(dead_code)] // 仅测试构造（session.rs 等）；生产路径用 TauriEventSink
pub(crate) struct NoopEventSink;

impl LanEventSink for NoopEventSink {
    fn emit(&self, _event: &str, _payload: &serde_json::Value) {}
}

#[derive(Debug, Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[ts(export)]
pub(crate) enum ClipSource {
    Current,
    Item { id: String },
    /// 分组（category）条目：id 为 `category_items.id`，category_id 为其所属分组。
    /// 发送时会附带分组名/颜色，接收端按名称匹配或新建分组。
    CategoryItem {
        id: String,
        #[serde(rename = "categoryId")]
        category_id: String,
    },
}

/// session loop 的控制指令。发送/断开类由 DeviceLinkRegistry 分发，
/// 会话循环（session.rs）消费。
#[derive(Debug)]
pub(crate) enum ControlMsg {
    /// 开始分组批量发送：session loop 先写 `CategoryBatchStart` 帧，随后
    /// 逐条 `SendClip`，最后 `BatchEnd` 写 `CategoryBatchEnd` 帧。
    BatchStart {
        category_name: String,
        category_color: Option<String>,
        item_count: u32,
    },
    /// 结束分组批量发送。
    BatchEnd,
    SendClip {
        clip_type: String,
        payload: Vec<u8>,
        /// 分组名（按名称在接收端匹配/创建分组）；None = 历史/无分组。
        category_name: Option<String>,
        /// 分组颜色（随分组名一起传，新建分组时采用）。
        category_color: Option<String>,
        /// 条目的重命名显示名；None = 未重命名。
        display_name: Option<String>,
        /// true = 捕获即自动同步（Spec 2 auto 路径；接收端静默入库）。
        /// 手动发送（send_raw/send_category）恒为 false。
        auto: bool,
        /// 发起方 EndpointId hex（Spec 2 回环抑制）；手动发送无 origin。
        origin_node_id: Option<String>,
    },
    RequestClip,
    /// 本地主动断开。v5 的 registry 经 drop 控制通道触发同一会话退出路径
    ///（session loop 的 `None` 分支与此共用发 Disconnect 帧的 arm），
    /// 目前无构造方，保留给显式断开语义。
    #[allow(dead_code)]
    Disconnect,
}

/// 本机设备名（host 名兜底 iPaste-device）。配对流程消费。
pub(crate) fn device_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "iPaste-device".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_source_deserializes_current() {
        let json = r#"{"kind":"current"}"#;
        let src: ClipSource = serde_json::from_str(json).unwrap();
        assert!(matches!(src, ClipSource::Current));
    }

    #[test]
    fn clip_source_deserializes_item() {
        let json = r#"{"kind":"item","id":"abc"}"#;
        let src: ClipSource = serde_json::from_str(json).unwrap();
        match src { ClipSource::Item { id } => assert_eq!(id, "abc"), _ => panic!("wrong variant") }
    }

    #[test]
    fn clip_source_deserializes_category_item() {
        // 前端 ipasteApi 走 camelCase（与 TS 类型一致）。
        let json = r#"{"kind":"categoryItem","id":"i1","categoryId":"c1"}"#;
        let src: ClipSource = serde_json::from_str(json).expect("camelCase variant must deserialize");
        match src {
            ClipSource::CategoryItem { id, category_id } => {
                assert_eq!(id, "i1");
                assert_eq!(category_id, "c1");
            }
            _ => panic!("wrong variant for {json}"),
        }
    }
}
