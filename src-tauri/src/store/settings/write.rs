//! 设置写入：各设置项的 update_* 与随之回填的 AppSettings。

// store/settings/{read,write}.rs — 设置读写（Task 32 拆分）
use rusqlite::{params, Connection};

use super::super::Store;
use super::registry;
use crate::models::{AppSettings, AutoPushSettings, CloudOcrSettings, CloudSettings};
use crate::util::{clean_append_copy_timeout_minutes, clean_retention_days};

impl Store {
    /// 设置写入的唯一落库动作：upsert 后回填完整 AppSettings。
    fn write_setting_value(&self, key: &str, value: &str) -> Result<AppSettings, String> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    /// 按 registry 规格写入字符串型设置：清洗不通过即拒绝（不落库）。
    fn write_string_setting(
        &self,
        spec: &registry::StringSettingSpec,
        value: String,
    ) -> Result<AppSettings, String> {
        let cleaned = (spec.clean)(value)?;
        self.write_setting_value(spec.key, &cleaned)
    }

    pub(crate) fn update_shortcut(&self, shortcut: String) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::SHORTCUT, shortcut)
    }

    pub(crate) fn update_ocr_shortcut(&self, shortcut: String) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::OCR_SHORTCUT, shortcut)
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
        self.write_setting_value("append_copy_timeout_minutes", &minutes.to_string())
    }

    pub(crate) fn update_panel_open_behavior(
        &self,
        behavior: String,
    ) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::PANEL_OPEN_BEHAVIOR, behavior)
    }

    pub(crate) fn update_panel_layout(&self, layout: String) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::PANEL_LAYOUT, layout)
    }

    pub(crate) fn update_ocr_mode(&self, mode: String) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::OCR_MODE, mode)
    }

    pub(crate) fn update_ocr_engine(&self, engine: String) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::OCR_ENGINE, engine)
    }

    pub(crate) fn update_language(&self, language: String) -> Result<AppSettings, String> {
        self.write_string_setting(&registry::LANGUAGE, language)
    }

    pub(crate) fn cloud_settings_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<CloudSettings, String> {
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
                match super::super::secrets::get_api_key() {
                    Ok(v) => v.unwrap_or_default(),
                    Err(reason) => {
                        eprintln!("[cloud] 读取系统凭据库失败：{reason}");
                        String::new()
                    }
                }
            } else {
                // 老版本明文遗留：一次性迁移进系统凭据库并清空该列。
                // 迁移失败（凭据库不可用）向上报错——用户重试即可，不做明文回退。
                super::super::secrets::put_api_key(&stored)
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

    /// 云 OCR 配置读取：Key 与云同步同法——settings 列只留空串占位，
    /// 实际值从系统凭据库回填；Base URL 与模型名是普通 KV。
    /// 凭据库读失败按未配置处理（不阻断整个设置读取），原因落 stderr 供排查。
    /// 另外顺手清理已移除的智谱专用引擎（≤0.9.9）遗留的凭据与占位行。
    pub(super) fn cloud_ocr_settings_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<CloudOcrSettings, String> {
        super::super::secrets::remove_legacy_bigmodel_api_key();
        let _ = conn.execute("DELETE FROM settings WHERE key = 'bigmodel_api_key'", []);
        let openai_api_key = match super::super::secrets::get_openai_ocr_api_key() {
            Ok(v) => v.unwrap_or_default(),
            Err(reason) => {
                eprintln!("[cloud-ocr] 读取系统凭据库失败（openai）：{reason}");
                String::new()
            }
        };
        let openai_prompts = match self.setting_value_with_conn(conn, "openai_ocr_prompts")? {
            Some(json_str) if !json_str.trim().is_empty() => {
                serde_json::from_str::<Vec<crate::models::CloudOcrPromptMessage>>(&json_str)
                    .unwrap_or_else(|_| crate::models::default_openai_ocr_prompts())
            }
            _ => crate::models::default_openai_ocr_prompts(),
        };
        Ok(CloudOcrSettings {
            openai_base_url: self
                .setting_value_with_conn(conn, "openai_ocr_base_url")?
                .unwrap_or_default(),
            openai_model: self
                .setting_value_with_conn(conn, "openai_ocr_model")?
                .unwrap_or_default(),
            openai_api_key,
            openai_prompts,
        })
    }

    /// 保存 OpenAI 兼容接口配置：Key 走凭据库，Base URL / 模型名 / Prompt 列表落普通 KV。
    pub(crate) fn update_openai_ocr_config(
        &self,
        base_url: String,
        model: String,
        api_key: String,
        prompts: Option<Vec<crate::models::CloudOcrPromptMessage>>,
    ) -> Result<AppSettings, String> {
        let base_url = crate::util::clean_openai_base_url(base_url)?;
        let model = crate::util::clean_openai_model(model)?;
        let api_key =
            crate::util::clean_api_key(api_key).map_err(|_| "请输入 API Key".to_string())?;
        super::super::secrets::put_openai_ocr_api_key(&api_key)?;
        let conn = self.connect()?;
        for (key, value) in [
            ("openai_ocr_base_url", base_url),
            ("openai_ocr_model", model),
        ] {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|error| error.to_string())?;
        }
        if let Some(prompts_list) = prompts {
            let filtered: Vec<crate::models::CloudOcrPromptMessage> = prompts_list
                .into_iter()
                .filter(|m| !m.content.trim().is_empty())
                .collect();
            let effective = if filtered.is_empty() {
                crate::models::default_openai_ocr_prompts()
            } else {
                filtered
            };
            let prompts_json = serde_json::to_string(&effective).map_err(|e| e.to_string())?;
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('openai_ocr_prompts', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![prompts_json],
            )
            .map_err(|error| error.to_string())?;
        }
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('openai_api_key', '')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    /// 清除 OpenAI 兼容接口配置（幂等删凭据库条目 + 清全部 KV）。
    pub(crate) fn clear_openai_ocr_config(&self) -> Result<AppSettings, String> {
        super::super::secrets::delete_openai_ocr_api_key()?;
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM settings WHERE key IN ('openai_ocr_base_url', 'openai_ocr_model', 'openai_api_key', 'openai_ocr_prompts')",
            [],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    /// 更新自定义中继地址：Some 须 `https://` 前缀（明文中继不走加密会被
    /// iroh 拒连/降级）；None/空白清除（恢复 n0 默认）。写入前 trim，
    /// 返回落库后的规范化值（空白归一为 None）。变更需重启应用才生效——
    /// endpoint 在启动时读取该设置绑定，命令层据此返回提示文案。
    pub(crate) fn update_sync_relay_url(
        &self,
        url: Option<&str>,
    ) -> Result<Option<String>, String> {
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

    /// 更新自动推送全局设置，返回落库后的值。
    pub(crate) fn update_auto_push_settings(
        &self,
        master: bool,
        notify: bool,
    ) -> Result<AutoPushSettings, String> {
        let conn = self.connect()?;
        for (key, value) in [
            ("sync_auto_push_master", master),
            ("sync_auto_push_notify", notify),
        ] {
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
