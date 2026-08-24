// store/settings.rs — 设置读写
use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::models::{AppSettings, AutoPushSettings, Category, CategoryItem, ClipPage, CloudSettings};
use crate::{
    DEFAULT_APPEND_COPY_TIMEOUT_MINUTES, DEFAULT_LANGUAGE, DEFAULT_OCR_MODE, DEFAULT_OCR_SHORTCUT,
    DEFAULT_PANEL_LAYOUT, DEFAULT_PANEL_OPEN_BEHAVIOR, DEFAULT_RETENTION_DAYS, DEFAULT_SHORTCUT,
    CLIP_PAGE_SIZE,
    util::{
        clean_append_copy_timeout_minutes, clean_language, clean_ocr_mode, clean_panel_layout,
        clean_panel_open_behavior, clean_retention_days, clean_shortcut,
    },
};

impl Store {
    pub(crate) fn snapshot(&self) -> Result<(ClipPage, Vec<Category>, Vec<CategoryItem>), String> {
        let conn = self.connect()?;
        Ok((
            self.list_clips_page_with_conn(&conn, 0, CLIP_PAGE_SIZE, "")?,
            self.list_categories_with_conn(&conn)?,
            self.list_category_items_with_conn(&conn)?,
        ))
    }

    pub(crate) fn settings(&self) -> Result<AppSettings, String> {
        let conn = self.connect()?;
        self.settings_with_conn(&conn)
    }

    pub(super) fn settings_with_conn(&self, conn: &Connection) -> Result<AppSettings, String> {
        let shortcut = self
            .setting_value_with_conn(conn, "shortcut")?
            .and_then(|value| clean_shortcut(value).ok())
            .unwrap_or_else(|| DEFAULT_SHORTCUT.to_string());
        let ocr_shortcut = self
            .setting_value_with_conn(conn, "ocr_shortcut")?
            .and_then(|value| clean_shortcut(value).ok())
            .unwrap_or_else(|| DEFAULT_OCR_SHORTCUT.to_string());
        // 防御历史脏数据：与面板快捷键同值时回落默认
        let ocr_shortcut = if ocr_shortcut == shortcut {
            DEFAULT_OCR_SHORTCUT.to_string()
        } else {
            ocr_shortcut
        };
        let retention_days = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'retention_days'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(|value| clean_retention_days(value).ok())
            .unwrap_or(DEFAULT_RETENTION_DAYS);
        let append_copy_timeout_minutes = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'append_copy_timeout_minutes'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(|value| clean_append_copy_timeout_minutes(value).ok())
            .unwrap_or(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
        let panel_open_behavior = self
            .setting_value_with_conn(conn, "panel_open_behavior")?
            .and_then(|value| clean_panel_open_behavior(value).ok())
            .unwrap_or_else(|| DEFAULT_PANEL_OPEN_BEHAVIOR.to_string());
        let panel_layout = self
            .setting_value_with_conn(conn, "panel_layout")?
            .and_then(|value| clean_panel_layout(value).ok())
            .unwrap_or_else(|| DEFAULT_PANEL_LAYOUT.to_string());
        let ocr_mode = self
            .setting_value_with_conn(conn, "ocr_mode")?
            .and_then(|value| clean_ocr_mode(value).ok())
            .unwrap_or_else(|| DEFAULT_OCR_MODE.to_string());
        let language = self
            .setting_value_with_conn(conn, "language")?
            .and_then(|value| clean_language(value).ok())
            .unwrap_or_else(|| DEFAULT_LANGUAGE.to_string());

        Ok(AppSettings {
            shortcut,
            ocr_shortcut,
            retention_days,
            append_copy_timeout_minutes,
            panel_open_behavior,
            panel_layout,
            ocr_mode,
            language,
            cloud: self.cloud_settings_with_conn(conn)?,
        })
    }

    pub(crate) fn update_shortcut(&self, shortcut: String) -> Result<AppSettings, String> {
        let shortcut = clean_shortcut(shortcut)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('shortcut', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![shortcut],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_ocr_shortcut(&self, shortcut: String) -> Result<AppSettings, String> {
        let shortcut = clean_shortcut(shortcut)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('ocr_shortcut', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![shortcut],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_settings(&self, retention_days: i64) -> Result<AppSettings, String> {
        let retention_days = clean_retention_days(retention_days)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('retention_days', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![retention_days.to_string()],
        )
        .map_err(|error| error.to_string())?;
        self.prune_expired_with_conn(&conn, retention_days)?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_append_copy_timeout_minutes(
        &self,
        minutes: i64,
    ) -> Result<AppSettings, String> {
        let minutes = clean_append_copy_timeout_minutes(minutes)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('append_copy_timeout_minutes', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![minutes.to_string()],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_panel_open_behavior(&self, behavior: String) -> Result<AppSettings, String> {
        let behavior = clean_panel_open_behavior(behavior)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('panel_open_behavior', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![behavior],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_panel_layout(&self, layout: String) -> Result<AppSettings, String> {
        let layout = clean_panel_layout(layout)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('panel_layout', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![layout],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_ocr_mode(&self, mode: String) -> Result<AppSettings, String> {
        let mode = clean_ocr_mode(mode)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('ocr_mode', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![mode],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn update_language(&self, language: String) -> Result<AppSettings, String> {
        let language = clean_language(language)?;
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('language', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![language],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(super) fn cloud_settings_with_conn(&self, conn: &Connection) -> Result<CloudSettings, String> {
        let api_address = self
            .setting_value_with_conn(conn, "cloud_api_address")?
            .unwrap_or_default();
        let api_key = {
            let stored = self
                .setting_value_with_conn(conn, "cloud_api_key")?
                .unwrap_or_default();
            if stored.is_empty() {
                // v0.3.29+：Key 存系统凭据库，settings 列只留空串占位。
                // 凭据库读失败按「未配置」处理（可用性优先，不阻断整个设置读取），
                // 原因落 stderr 供排查。
                match super::secrets::get_api_key() {
                    Ok(v) => v.unwrap_or_default(),
                    Err(reason) => {
                        eprintln!("[cloud] 读取系统凭据库失败：{reason}");
                        String::new()
                    }
                }
            } else {
                // 老版本明文遗留：一次性迁移进系统凭据库并清空该列。
                // 迁移失败（凭据库不可用）向上报错——用户重试即可，不做明文回退。
                super::secrets::put_api_key(&stored)
                    .map_err(|reason| format!("迁移 API Key 到系统凭据库失败：{reason}"))?;
                conn.execute(
                    "UPDATE settings SET value = '' WHERE key = 'cloud_api_key'",
                    [],
                )
                .map_err(|error| error.to_string())?;
                stored
            }
        };
        let last_connected_at = self.setting_value_with_conn(conn, "cloud_last_connected_at")?;
        let enabled = !api_address.is_empty() && !api_key.is_empty();

        Ok(CloudSettings {
            api_address,
            api_key,
            enabled,
            last_connected_at,
        })
    }

    fn setting_value_with_conn(
        &self,
        conn: &Connection,
        key: &str,
    ) -> Result<Option<String>, String> {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    /// 跨设备同步的自定义中继地址（settings 表 KV `sync_relay_url`）。
    /// None = 使用 n0 默认中继。防御历史脏数据：空白值按未设置处理。
    pub(crate) fn sync_relay_url(&self) -> Result<Option<String>, String> {
        let conn = self.connect()?;
        Ok(self
            .setting_value_with_conn(&conn, "sync_relay_url")?
            .filter(|value| !value.trim().is_empty()))
    }

    /// 更新自定义中继地址：Some 须 `https://` 前缀（明文中继不走加密会被
    /// iroh 拒连/降级）；None/空白清除（恢复 n0 默认）。写入前 trim，
    /// 返回落库后的规范化值（空白归一为 None）。变更需重启应用才生效——
    /// endpoint 在启动时读取该设置绑定，命令层据此返回提示文案。
    pub(crate) fn update_sync_relay_url(&self, url: Option<&str>) -> Result<Option<String>, String> {
        let cleaned = url
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        if let Some(value) = &cleaned {
            if !value.starts_with("https://") {
                return Err("中继地址必须以 https:// 开头".to_string());
            }
        }
        let conn = self.connect()?;
        match &cleaned {
            Some(value) => conn
                .execute(
                    "INSERT INTO settings (key, value) VALUES ('sync_relay_url', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![value],
                )
                .map_err(|error| error.to_string())?,
            None => conn
                .execute("DELETE FROM settings WHERE key = 'sync_relay_url'", [])
                .map_err(|error| error.to_string())?,
        };
        Ok(cleaned)
    }

    /// 自动推送全局设置（settings 表 KV `sync_auto_push_master/notify`；缺省
    /// true/false）。坏值（非 "true"/"false"）回退缺省并记 stderr，不阻断读取。
    pub(crate) fn auto_push_settings(&self) -> Result<AutoPushSettings, String> {
        let conn = self.connect()?;
        let read_bool = |key: &str, default: bool| -> Result<bool, String> {
            Ok(self
                .setting_value_with_conn(&conn, key)?
                .map(|value| match value.parse::<bool>() {
                    Ok(parsed) => parsed,
                    Err(_) => {
                        eprintln!("[autopush] settings 键 {key} 坏值（{value}），回退缺省 {default}");
                        default
                    }
                })
                .unwrap_or(default))
        };
        Ok(AutoPushSettings {
            master: read_bool("sync_auto_push_master", true)?,
            notify: read_bool("sync_auto_push_notify", false)?,
        })
    }

    /// 更新自动推送全局设置，返回落库后的值。
    pub(crate) fn update_auto_push_settings(
        &self,
        master: bool,
        notify: bool,
    ) -> Result<AutoPushSettings, String> {
        let conn = self.connect()?;
        for (key, value) in [("sync_auto_push_master", master), ("sync_auto_push_notify", notify)] {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value.to_string()],
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(AutoPushSettings { master, notify })
    }
}

#[cfg(test)]
mod tests {
    use crate::store::test_support::temp_store;

    #[test]
    fn settings_round_trip_for_enum_like_values() {
        let store = temp_store();

        let s = store.update_panel_layout("side".to_string()).unwrap();
        assert_eq!(s.panel_layout, "side");

        let s = store.update_ocr_mode("best".to_string()).unwrap();
        assert_eq!(s.ocr_mode, "best");

        let s = store.update_panel_open_behavior("last_selected".to_string()).unwrap();
        assert_eq!(s.panel_open_behavior, "last_selected");

        let s = store.update_language("zh-CN".to_string()).unwrap();
        assert_eq!(s.language, "zh-CN");

        let s = store.settings().unwrap();
        assert_eq!(s.panel_layout, "side");
        assert_eq!(s.ocr_mode, "best");
        assert_eq!(s.panel_open_behavior, "last_selected");
        assert_eq!(s.language, "zh-CN");
    }

    #[test]
    fn ocr_shortcut_round_trip_and_conflict_fallback() {
        let store = temp_store();

        let s = store.update_ocr_shortcut("Alt+S".to_string()).unwrap();
        assert_eq!(s.ocr_shortcut, "Alt+S");

        // 存储值与面板快捷键同值时，读取侧回落默认，避免一个组合触发两个动作
        let s = store.update_shortcut("Alt+S".to_string()).unwrap();
        assert_eq!(s.shortcut, "Alt+S");
        assert_eq!(s.ocr_shortcut, crate::DEFAULT_OCR_SHORTCUT);
    }

    #[test]
    fn sync_relay_url_round_trip_and_clear() {
        let store = temp_store();
        assert_eq!(store.sync_relay_url().unwrap(), None, "未设置时为 None");

        // 写入会 trim；往返读回一致
        let saved = store
            .update_sync_relay_url(Some("  https://relay.example.com  "))
            .unwrap();
        assert_eq!(saved.as_deref(), Some("https://relay.example.com"));
        assert_eq!(
            store.sync_relay_url().unwrap().as_deref(),
            Some("https://relay.example.com")
        );

        // 空白 = 清除（恢复 n0 默认）
        let saved = store.update_sync_relay_url(Some("   ")).unwrap();
        assert_eq!(saved, None);
        assert_eq!(store.sync_relay_url().unwrap(), None, "空白清除后读取为 None");

        // 再次写入后显式 None 也清除
        store.update_sync_relay_url(Some("https://relay2.example.com")).unwrap();
        let saved = store.update_sync_relay_url(None).unwrap();
        assert_eq!(saved, None);
        assert_eq!(store.sync_relay_url().unwrap(), None);
    }

    #[test]
    fn sync_relay_url_rejects_non_https() {
        let store = temp_store();
        for bad in ["http://relay.example.com", "relay.example.com", "https-relay.example.com"] {
            let error = store.update_sync_relay_url(Some(bad)).unwrap_err();
            assert!(error.contains("https"), "got: {error}");
        }
        assert_eq!(store.sync_relay_url().unwrap(), None, "拒绝的值不落库");
    }

    #[test]
    fn sync_relay_url_blank_stored_value_reads_as_none() {
        // 防御历史脏数据：库里存了空白串时读取按未设置处理
        let store = temp_store();
        {
            let conn = store.connect().unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('sync_relay_url', '  ')",
                [],
            )
            .unwrap();
        }
        assert_eq!(store.sync_relay_url().unwrap(), None);
    }

    #[test]
    fn auto_push_settings_defaults_and_round_trip() {
        let store = temp_store();

        // 缺省 master=true / notify=false
        assert_eq!(
            store.auto_push_settings().unwrap(),
            crate::models::AutoPushSettings { master: true, notify: false }
        );

        // 写后往返
        let saved = store.update_auto_push_settings(false, true).unwrap();
        assert_eq!(
            saved,
            crate::models::AutoPushSettings { master: false, notify: true }
        );
        assert_eq!(
            store.auto_push_settings().unwrap(),
            crate::models::AutoPushSettings { master: false, notify: true }
        );
    }

    #[test]
    fn auto_push_settings_bad_values_fall_back_to_defaults() {
        let store = temp_store();
        {
            let conn = store.connect().unwrap();
            for key in ["sync_auto_push_master", "sync_auto_push_notify"] {
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, 'junk')
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [key],
                )
                .unwrap();
            }
        }
        assert_eq!(
            store.auto_push_settings().unwrap(),
            crate::models::AutoPushSettings { master: true, notify: false },
            "坏值回退缺省"
        );
    }
}

#[cfg(test)]
mod cloud_keyring_tests {
    use crate::store::secrets;
    use crate::store::test_support::temp_store;

    /// mock keyring 是进程级共享的内存后端，相关测试必须串行。
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn legacy_plaintext_key_migrates_into_secret_store() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = secrets::delete_api_key();
        let store = temp_store();
        {
            let conn = store.connect().unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('cloud_api_key', 'legacy-plain-key')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
        }
        let conn = store.connect().unwrap();
        let cloud = store.cloud_settings_with_conn(&conn).unwrap();
        assert_eq!(cloud.api_key, "legacy-plain-key");
        assert_eq!(secrets::get_api_key().unwrap().as_deref(), Some("legacy-plain-key"));
        let leftover: String = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'cloud_api_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(leftover, "", "明文列迁移后必须被清空");
        let _ = secrets::delete_api_key();
    }

    #[test]
    fn keyring_value_is_returned_when_column_is_placeholder() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = secrets::delete_api_key();
        secrets::put_api_key("from-keyring").unwrap();
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cloud = store.cloud_settings_with_conn(&conn).unwrap();
        assert_eq!(cloud.api_key, "from-keyring");
        assert!(cloud.enabled || cloud.api_address.is_empty(), "enabled 判定沿用地址+键非空");
        let _ = secrets::delete_api_key();
    }

    #[test]
    fn disable_cloud_sync_clears_secret_store() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = secrets::delete_api_key();
        secrets::put_api_key("to-be-removed").unwrap();
        let store = temp_store();
        {
            let conn = store.connect().unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('cloud_api_address', 'https://x.example')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('cloud_api_key', '')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
        }
        store.disable_cloud_sync().unwrap();
        assert_eq!(secrets::get_api_key().unwrap(), None);
    }
}
