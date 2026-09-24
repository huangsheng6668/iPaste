//! 设备管理域（原 registry.rs「设备管理」段整体迁入，纯移动）：设备列表合成
//! （device_infos）、撤销/删除（revoke/delete_device）、同步偏好（set_auto_sync）、
//! 显式断开（disconnect）与整体关停（shutdown）。

use crate::models::{AutoSyncMode, DeviceInfo, DeviceOnline};

use super::{DeviceLinkRegistry, LinkHandle};

impl DeviceLinkRegistry {
    // —— 设备管理 ——

    /// store 行 + links 状态合成；撤销的恒 Offline（即使有残留登记）。
    pub(crate) fn device_infos(&self) -> Vec<DeviceInfo> {
        let devices = self.inner.store.list_paired_devices().unwrap_or_else(|reason| {
            eprintln!("[lan-sync] 读取设备列表失败：{reason}");
            Vec::new()
        });
        if devices.is_empty() {
            return Vec::new();
        }
        let links = self.inner.links.lock().expect("links 锁中毒");
        devices
            .into_iter()
            .map(|device| {
                let online = if device.revoked_at.is_some() {
                    DeviceOnline::Offline
                } else {
                    links
                        .get(&device.node_id)
                        .map(|handle| handle.status)
                        .unwrap_or(DeviceOnline::Offline)
                };
                DeviceInfo { device, online }
            })
            .collect()
    }

    /// 断开某设备的链路：登记移除（control_tx drop → 会话发 Disconnect 帧干净退出）
    /// + abort 重拨任务。
    fn kill_link(&self, node_id: &str) {
        let removed = {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            links.remove(node_id)
        };
        if let Some(handle) = removed {
            if let Some(task) = &handle.task {
                task.abort();
            }
        }
    }

    /// 撤销信任（软删）：store 撤销 + 断链 + emit 列表。
    pub(crate) fn revoke(&self, node_id: &str) {
        if let Err(reason) = self.inner.store.revoke_device(node_id) {
            eprintln!("[lan-sync] 撤销设备失败：{reason}");
        }
        self.kill_link(node_id);
        self.clear_disconnected(node_id); // 撤销行本身即拒绝入站，标记无意义
        self.emit_status(node_id, DeviceOnline::Offline);
        self.emit_device_list();
    }

    /// 彻底删除记录：此后该设备拨号等同陌生设备。先删 store 行再断链
    /// （状态写先行，与 revoke 同形——admission 的 store 读必然看到已删除）。
    pub(crate) fn delete_device(&self, node_id: &str) {
        if let Err(reason) = self.inner.store.delete_device(node_id) {
            eprintln!("[lan-sync] 删除设备失败：{reason}");
        }
        self.kill_link(node_id);
        self.clear_disconnected(node_id);
        self.emit_status(node_id, DeviceOnline::Offline);
        self.emit_device_list();
    }

    /// 仅改同步偏好（不断链）+ emit 列表。
    pub(crate) fn set_auto_sync(&self, node_id: &str, mode: AutoSyncMode) {
        if let Err(reason) = self.inner.store.set_auto_sync_mode(node_id, mode) {
            eprintln!("[lan-sync] 设置同步偏好失败：{reason}");
        }
        self.emit_device_list();
    }

    /// 用户主动断开某设备：先记入内存态断开标记，再杀会话/停重拨（状态写先行，
    /// 与 revoke 同形——关闭 run_session 准入与登记间的 TOCTOU 窗口）——此后对端
    /// 重拨一律静默拒绝，直到重新配对成功或重启应用（重启清空标记）。
    pub(crate) fn disconnect(&self, node_id: &str) {
        self.inner
            .disconnected
            .lock()
            .expect("disconnected 锁中毒")
            .insert(node_id.to_string());
        self.kill_link(node_id);
        self.emit_status(node_id, DeviceOnline::Offline);
        self.emit_device_list();
    }

    /// 停止入站接受循环并断开全部链路。Endpoint 本体随最后的 Arc 引用释放关闭
    ///（其 close() 是异步的，留给 Task 8 的 lib 接线决定是否显式等待）。
    pub(crate) fn shutdown(&self) {
        if let Some(task) = self.inner.accept_task.lock().expect("accept_task 锁中毒").take() {
            task.abort();
        }
        let handles: Vec<LinkHandle> = {
            let mut links = self.inner.links.lock().expect("links 锁中毒");
            links.drain().map(|(_, handle)| handle).collect()
        };
        // control_tx 随 handle drop → 各会话收到 None 干净关闭；任务 abort 停止重拨
        for handle in handles {
            if let Some(task) = handle.task {
                task.abort();
            }
        }
        self.emit_device_list();
    }
}
