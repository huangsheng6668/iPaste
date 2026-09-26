//! 剪贴板捕获域状态：监听开关、追加复制会话、去重光标。
//!
//! 这四个字段原先平铺在 AppState 上（Task 27 前），聚合后仍由调用方各自持锁，
//! 锁的粒度与获取顺序完全不变——本步是纯结构移动。

use std::sync::{Arc, Mutex};

use crate::models::AppendCopyState;

/// 捕获域上下文。
pub struct CaptureState {
    /// 是否正在监听剪贴板（托盘"暂停捕获"控制）。
    pub is_listening: Arc<Mutex<bool>>,
    /// 追加复制会话状态（与 lan_sync 接收侧共享同一实例）。
    pub append_copy_state: Arc<Mutex<AppendCopyState>>,
    /// 上次捕获的系统 change id（平台不支持时为 None）。
    pub last_clipboard_change_id: Arc<Mutex<Option<u64>>>,
    /// 上次捕获的内容哈希（无 change id 平台上的唯一去重依据）。
    pub last_clipboard_hash: Arc<Mutex<Option<String>>>,
}
