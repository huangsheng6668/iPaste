// store/settings.rs — 设置读写
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::*;
use crate::util::*;
use super::Store;
use crate::{
    DEFAULT_APPEND_COPY_TIMEOUT_MINUTES, DEFAULT_LANGUAGE, DEFAULT_OCR_MODE,
    DEFAULT_PANEL_LAYOUT, DEFAULT_PANEL_OPEN_BEHAVIOR, DEFAULT_RETENTION_DAYS, DEFAULT_SHORTCUT,
    APPEND_COPY_TIMEOUT_OPTIONS, CLIP_PAGE_SIZE, RETENTION_OPTIONS,
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
        let retention_days = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'retention_days'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|value| RETENTION_OPTIONS.contains(value))
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
            .filter(|value| APPEND_COPY_TIMEOUT_OPTIONS.contains(value))
            .unwrap_or(DEFAULT_APPEND_COPY_TIMEOUT_MINUTES);
        let panel_open_behavior = self
            .setting_value_with_conn(conn, "panel_open_behavior")?
            .filter(|value| value == "history" || value == "last_selected")
            .unwrap_or_else(|| DEFAULT_PANEL_OPEN_BEHAVIOR.to_string());
        let panel_layout = self
            .setting_value_with_conn(conn, "panel_layout")?
            .filter(|value| value == "top" || value == "side")
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
}

#[cfg(test)]
mod cloud_keyring_tests {
    use super::*;
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
