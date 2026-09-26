//! 设置读取：快照入口、AppSettings 组装、云 OCR 就绪判定与同步相关读取。

// store/settings/{read,write}.rs — 设置读写（Task 32 拆分）
use rusqlite::{params, Connection, OptionalExtension};

use super::super::Store;
use crate::models::{AppSettings, AutoPushSettings, Category, CategoryItem, ClipPage};
use crate::{
    DEFAULT_APPEND_COPY_TIMEOUT_MINUTES, DEFAULT_LANGUAGE, DEFAULT_OCR_ENGINE, DEFAULT_OCR_MODE,
    DEFAULT_OCR_SHORTCUT, DEFAULT_PANEL_LAYOUT, DEFAULT_PANEL_OPEN_BEHAVIOR,
    DEFAULT_RETENTION_DAYS, DEFAULT_SHORTCUT, CLIP_PAGE_SIZE,
    util::{
        clean_append_copy_timeout_minutes, clean_language, clean_ocr_engine, clean_ocr_mode,
        clean_panel_layout, clean_panel_open_behavior, clean_retention_days, clean_shortcut,
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

    pub(crate) fn settings_with_conn(&self, conn: &Connection) -> Result<AppSettings, String> {
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
        let ocr_engine = self
            .setting_value_with_conn(conn, "ocr_engine")?
            .and_then(|value| clean_ocr_engine(value).ok())
            .unwrap_or_else(|| DEFAULT_OCR_ENGINE.to_string());
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
            ocr_engine,
            language,
            cloud: self.cloud_settings_with_conn(conn)?,
            cloud_ocr: self.cloud_ocr_settings_with_conn(conn)?,
        })
    }

    /// 云 OCR 引擎当前是否可用（引擎已选且配置完整）。preflight 与调度
    /// 分支共用；凭据库读失败按未配置处理（可用性优先）。
    pub(crate) fn cloud_ocr_engine_ready(&self) -> bool {
        let Ok(settings) = self.settings() else {
            return false;
        };
        match settings.ocr_engine.as_str() {
            "openai" => {
                !settings.cloud_ocr.openai_api_key.is_empty()
                    && !settings.cloud_ocr.openai_base_url.is_empty()
                    && !settings.cloud_ocr.openai_model.is_empty()
            }
            _ => false,
        }
    }

    pub(super) fn setting_value_with_conn(
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
}
