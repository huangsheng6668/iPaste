//! paired_devices 信任表的 CRUD 与撤销/删除语义（spec §3）。

use chrono::Utc;
use rusqlite::params;

use super::Store;
use crate::models::{AutoSyncMode, PairedDevice};

fn row_to_paired_device(row: &rusqlite::Row<'_>) -> rusqlite::Result<PairedDevice> {
    let relay_url: Option<String> = row.get("relay_url")?;
    let addrs_json: String = row.get("direct_addrs")?;
    let mode: String = row.get("auto_sync_mode")?;
    Ok(PairedDevice {
        node_id: row.get("node_id")?,
        device_name: row.get("device_name")?,
        relay_url,
        direct_addrs: serde_json::from_str(&addrs_json).unwrap_or_default(),
        auto_sync_mode: match mode.as_str() {
            "all" => AutoSyncMode::All,
            "off" => AutoSyncMode::Off,
            _ => AutoSyncMode::TextOnly,
        },
        added_at: row.get("added_at")?,
        last_seen_at: row.get("last_seen_at")?,
        revoked_at: row.get("revoked_at")?,
    })
}

impl Store {
    /// 配对成功时调用：不存在则插入（偏好默认 text_only），存在且未撤销则刷新元数据；
    /// 已撤销的行**不复活**（撤销后再次配对需先删除记录，防「撤销被配对绕过」）。
    pub(crate) fn upsert_paired_device(
        &self,
        node_id: &str,
        device_name: &str,
        relay_url: Option<&str>,
        direct_addrs: &[String],
    ) -> Result<(), String> {
        let conn = self.connect()?;
        self.upsert_paired_device_with_conn(&conn, node_id, device_name, relay_url, direct_addrs)
    }

    pub(super) fn upsert_paired_device_with_conn(
        &self,
        conn: &rusqlite::Connection,
        node_id: &str,
        device_name: &str,
        relay_url: Option<&str>,
        direct_addrs: &[String],
    ) -> Result<(), String> {
        let addrs_json = serde_json::to_string(direct_addrs).map_err(|e| e.to_string())?;
        let existing_revoked: Option<Option<String>> = conn
            .query_row(
                "SELECT revoked_at FROM paired_devices WHERE node_id = ?1",
                params![node_id],
                |row| row.get::<_, Option<String>>(0),
            )
            .map(Some)
            .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            .map_err(|e| e.to_string())?;
        match existing_revoked {
            Some(Some(_revoked_at)) => Ok(()), // 已撤销：保持撤销态，调用方由配对流程拒绝
            Some(None) => {
                conn.execute(
                    "UPDATE paired_devices
                     SET device_name = ?2, relay_url = ?3, direct_addrs = ?4, last_seen_at = ?5
                     WHERE node_id = ?1",
                    params![node_id, device_name, relay_url, addrs_json, Utc::now().to_rfc3339()],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
            None => {
                conn.execute(
                    "INSERT INTO paired_devices
                        (node_id, device_name, relay_url, direct_addrs, auto_sync_mode, added_at, last_seen_at)
                     VALUES (?1, ?2, ?3, ?4, 'text_only', ?5, ?5)",
                    params![node_id, device_name, relay_url, addrs_json, Utc::now().to_rfc3339()],
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    }

    pub(crate) fn list_paired_devices(&self) -> Result<Vec<PairedDevice>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT * FROM paired_devices ORDER BY added_at ASC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], row_to_paired_device)
            .map_err(|e| e.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
    }

    pub(crate) fn get_paired_device(&self, node_id: &str) -> Result<Option<PairedDevice>, String> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT * FROM paired_devices WHERE node_id = ?1",
            params![node_id],
            row_to_paired_device,
        )
        .map(Some)
        .or_else(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e.to_string()) }
        })
        .map_err(|e| e.to_string())
    }

    pub(crate) fn is_trusted(&self, node_id: &str) -> Result<bool, String> {
        Ok(self
            .get_paired_device(node_id)?
            .is_some_and(|d| d.revoked_at.is_none()))
    }

    pub(crate) fn touch_last_seen(&self, node_id: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE paired_devices SET last_seen_at = ?2 WHERE node_id = ?1",
            params![node_id, Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 更新已配对设备的元数据（名称/中继/直连线索）；不插入新行、不改动撤销态
    /// 与 last_seen_at（在线心跳由 `touch_last_seen` 单独维护）。
    pub(crate) fn update_peer_meta(
        &self,
        node_id: &str,
        device_name: &str,
        relay_url: Option<&str>,
        direct_addrs: &[String],
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let addrs_json = serde_json::to_string(direct_addrs).map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE paired_devices
             SET device_name = ?2, relay_url = ?3, direct_addrs = ?4
             WHERE node_id = ?1",
            params![node_id, device_name, relay_url, addrs_json],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn set_auto_sync_mode(&self, node_id: &str, mode: AutoSyncMode) -> Result<(), String> {
        let conn = self.connect()?;
        let mode_str = match mode {
            AutoSyncMode::TextOnly => "text_only",
            AutoSyncMode::All => "all",
            AutoSyncMode::Off => "off",
        };
        conn.execute(
            "UPDATE paired_devices SET auto_sync_mode = ?2 WHERE node_id = ?1",
            params![node_id, mode_str],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 撤销：软删（保留行以静默拒绝重拨）。
    pub(crate) fn revoke_device(&self, node_id: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE paired_devices SET revoked_at = ?2 WHERE node_id = ?1",
            params![node_id, Utc::now().to_rfc3339()],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// 彻底删除记录：此后该设备拨号等同陌生设备（无邀请 → 静默拒绝）。
    pub(crate) fn delete_device(&self, node_id: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM paired_devices WHERE node_id = ?1", params![node_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::temp_store;

    fn node(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    #[test]
    fn upsert_then_list_roundtrip() {
        let store = temp_store();
        store
            .upsert_paired_device(&node(1), "MBP", Some("https://r.example"), &["192.168.1.5:1".into()])
            .unwrap();
        let list = store.list_paired_devices().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].device_name, "MBP");
        assert_eq!(list[0].auto_sync_mode, AutoSyncMode::TextOnly);
        assert_eq!(list[0].direct_addrs, vec!["192.168.1.5:1".to_string()]);
        assert!(list[0].revoked_at.is_none());
    }

    #[test]
    fn revoke_blocks_trust_and_upsert_does_not_revive() {
        let store = temp_store();
        store.upsert_paired_device(&node(2), "PC", None, &[]).unwrap();
        assert!(store.is_trusted(&node(2)).unwrap());
        store.revoke_device(&node(2)).unwrap();
        assert!(!store.is_trusted(&node(2)).unwrap(), "撤销后不可信");
        // 已撤销行上再 upsert：不复活（调用方需先 delete）
        store.upsert_paired_device(&node(2), "PC2", None, &[]).unwrap();
        assert!(!store.is_trusted(&node(2)).unwrap(), "撤销不可被配对绕过");
        // 删除后从列表消失
        store.delete_device(&node(2)).unwrap();
        assert!(store.list_paired_devices().unwrap().is_empty());
        // 删除后重新配对 → 全新行（未撤销）
        store.upsert_paired_device(&node(2), "PC3", None, &[]).unwrap();
        assert!(store.is_trusted(&node(2)).unwrap());
    }

    #[test]
    fn set_auto_sync_mode_persists() {
        let store = temp_store();
        store.upsert_paired_device(&node(3), "X", None, &[]).unwrap();
        store.set_auto_sync_mode(&node(3), AutoSyncMode::All).unwrap();
        assert_eq!(store.get_paired_device(&node(3)).unwrap().unwrap().auto_sync_mode, AutoSyncMode::All);
        store.set_auto_sync_mode(&node(3), AutoSyncMode::Off).unwrap();
        assert_eq!(store.get_paired_device(&node(3)).unwrap().unwrap().auto_sync_mode, AutoSyncMode::Off);
    }

    #[test]
    fn touch_last_seen_updates() {
        let store = temp_store();
        store.upsert_paired_device(&node(4), "Y", None, &[]).unwrap();
        assert!(store.get_paired_device(&node(4)).unwrap().unwrap().last_seen_at.is_some());
        store.touch_last_seen(&node(4)).unwrap();
        assert!(store.get_paired_device(&node(4)).unwrap().unwrap().last_seen_at.is_some());
    }

    #[test]
    fn update_peer_meta_updates_existing_row_only() {
        let store = temp_store();
        // 行不存在：静默无效果，不插入
        store.update_peer_meta(&node(5), "Ghost", None, &[]).unwrap();
        assert!(store.list_paired_devices().unwrap().is_empty());
        store.upsert_paired_device(&node(5), "Old", Some("https://old"), &[]).unwrap();
        store
            .update_peer_meta(&node(5), "New", Some("https://new"), &["10.0.0.2:9".into()])
            .unwrap();
        let d = store.get_paired_device(&node(5)).unwrap().unwrap();
        assert_eq!(d.device_name, "New");
        assert_eq!(d.relay_url.as_deref(), Some("https://new"));
        assert_eq!(d.direct_addrs, vec!["10.0.0.2:9".to_string()]);
        assert_eq!(d.auto_sync_mode, AutoSyncMode::TextOnly, "元数据更新不动同步偏好");
    }
}
