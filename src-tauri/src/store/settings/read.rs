//! 设置读取：快照入口、AppSettings 组装、云 OCR 就绪判定与同步相关读取。

// store/settings/{read,write}.rs — 设置读写（Task 32 拆分）
use rusqlite::{params, Connection, OptionalExtension};

use super::super::Store;
use super::registry;
use crate::models::{
    AppSettings, AutoPushSettings, Category, CategoryItem, ClipPage, Language, OcrEngine, OcrMode,
    PanelLayout, PanelOpenBehavior,
};
use crate::{
    util::{clean_append_copy_timeout_minutes, clean_retention_days},
    CLIP_PAGE_SIZE, DEFAULT_APPEND_COPY_TIMEOUT_MINUTES, DEFAULT_RETENTION_DAYS,
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
        // 字符串型设置统一按 registry 循环读取（新增设置项只需登记规格）。
        let mut strings = std::collections::HashMap::new();
        for spec in registry::STRING_SETTINGS {
            strings.insert(spec.key, self.string_setting_with_conn(conn, spec)?);
        }
        let value = |key: &str| strings.get(key).cloned().unwrap_or_default();

        let shortcut = value(registry::SHORTCUT.key);
        let stored_ocr_shortcut = value(registry::OCR_SHORTCUT.key);
        // 防御历史脏数据：与面板快捷键同值时回落默认
        let ocr_shortcut = if stored_ocr_shortcut == shortcut {
            crate::DEFAULT_OCR_SHORTCUT.to_string()
        } else {
            stored_ocr_shortcut
        };

        Ok(AppSettings {
            shortcut,
            ocr_shortcut,
            retention_days: self.number_setting_with_conn(
                conn,
                "retention_days",
                clean_retention_days,
                DEFAULT_RETENTION_DAYS,
            )?,
            append_copy_timeout_minutes: self.number_setting_with_conn(
                conn,
                "append_copy_timeout_minutes",
                clean_append_copy_timeout_minutes,
                DEFAULT_APPEND_COPY_TIMEOUT_MINUTES,
            )?,
            panel_open_behavior: PanelOpenBehavior::from_db_str(&value(
                registry::PANEL_OPEN_BEHAVIOR.key,
            ))
            .unwrap_or_else(PanelOpenBehavior::default_value),
            panel_layout: PanelLayout::from_db_str(&value(registry::PANEL_LAYOUT.key))
                .unwrap_or_else(PanelLayout::default_value),
            ocr_mode: OcrMode::from_db_str(&value(registry::OCR_MODE.key))
                .unwrap_or_else(OcrMode::default_value),
            ocr_engine: OcrEngine::from_db_str(&value(registry::OCR_ENGINE.key))
                .unwrap_or_else(OcrEngine::default_value),
            language: Language::from_db_str(&value(registry::LANGUAGE.key))
                .unwrap_or_else(Language::default_value),
            cloud: self.cloud_settings_with_conn(conn)?,
            cloud_ocr: self.cloud_ocr_settings_with_conn(conn)?,
        })
    }

    /// 按 registry 规格读取字符串型设置：清洗失败回落默认（与旧逐字段手写版等价）。
    fn string_setting_with_conn(
        &self,
        conn: &Connection,
        spec: &registry::StringSettingSpec,
    ) -> Result<String, String> {
        Ok(self
            .setting_value_with_conn(conn, spec.key)?
            .and_then(|value| (spec.clean)(value).ok())
            .unwrap_or_else(|| spec.default.to_string()))
    }

    /// 按 registry 规格读取数值型设置：解析或清洗失败回落默认。
    fn number_setting_with_conn(
        &self,
        conn: &Connection,
        key: &str,
        clean: fn(i64) -> Result<i64, String>,
        default: i64,
    ) -> Result<i64, String> {
        let raw = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        Ok(raw
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(|value| clean(value).ok())
            .unwrap_or(default))
    }

    /// 云 OCR 引擎当前是否可用（引擎已选且配置完整）。preflight 与调度
    /// 分支共用；凭据库读失败按未配置处理（可用性优先）。
    /// macOS 的 preflight 走屏幕录制权限分支，此处仅测试引用；
    /// allow(dead_code) 消除平台性警告。
    #[allow(dead_code)]
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
                        eprintln!(
                            "[autopush] settings 键 {key} 坏值（{value}），回退缺省 {default}"
                        );
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
