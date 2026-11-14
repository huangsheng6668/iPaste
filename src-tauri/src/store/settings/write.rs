//! 设置写入：各设置项的 update_* 与随之回填的 AppSettings。

// store/settings/{read,write}.rs — 设置读写（Task 32 拆分）
use rusqlite::{params, Connection};

use super::super::Store;
use super::registry;
use crate::models::{
    AppSettings, AutoPushSettings, CloudOcrPromptMessage, CloudOcrSettings, CloudSettings,
    OpenaiOcrProfile,
};
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

    /// 云同步配置读取（纯读路径，不做任何写入）：
    /// - Key 正常存系统凭据库，settings 列只留空串占位（v0.3.29+）；
    /// - 列非空是 ≤0.3.28 的明文遗留，直接按值返回保可用性——迁移到凭据库
    ///   由启动时的 `cleanup_legacy_secrets` 完成，读取本身不再触发写库/写凭据库
    ///   （读路径写库会在 WAL 下与捕获写入方抢写锁，拖慢面板唤出等热路径）。
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

    /// 启动期一次性遗留清理（Store::new 调用；幂等、尽力而为，失败只记日志
    /// 不阻断启动——明文列保留，下次启动重试迁移）：
    /// - ≤0.3.28 的明文 cloud_api_key 迁入系统凭据库并清空该列；
    /// - ≤0.9.9 已移除的智谱专用引擎遗留的凭据条目与 settings 占位行。
    /// 这两件事原先挂在每次设置读取上，是读路径上的隐性写事务来源。
    pub(crate) fn cleanup_legacy_secrets(&self, conn: &Connection) {
        let stored = self
            .setting_value_with_conn(conn, "cloud_api_key")
            .unwrap_or_default()
            .unwrap_or_default();
        if !stored.is_empty() {
            match super::super::secrets::put_api_key(&stored) {
                Ok(()) => {
                    if let Err(error) = conn.execute(
                        "UPDATE settings SET value = '' WHERE key = 'cloud_api_key'",
                        [],
                    ) {
                        eprintln!("[cloud] 清空明文 API Key 列失败：{error}");
                    }
                }
                Err(reason) => {
                    eprintln!(
                        "[cloud] 迁移明文 API Key 到系统凭据库失败（下次启动重试）：{reason}"
                    );
                }
            }
        }

        super::super::secrets::remove_legacy_bigmodel_api_key();
        if let Err(error) = conn.execute("DELETE FROM settings WHERE key = 'bigmodel_api_key'", [])
        {
            eprintln!("[cloud-ocr] 清理遗留 bigmodel 占位行失败：{error}");
        }
    }

    /// 云 OCR 配置读取：列表不含真实 Key，逐配置从系统凭据库回填。
    /// 新列表缺失/损坏时把旧单配置读成 id=legacy 的配置，不做破坏性迁移；
    /// 读取路径保持纯读，凭据库失败按空 Key 处理并落 stderr。
    pub(super) fn cloud_ocr_settings_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<CloudOcrSettings, String> {
        let openai_prompts = self.openai_ocr_prompts_with_conn(conn)?;
        let mut profiles = self.stored_openai_ocr_profiles_with_conn(conn)?;
        let stored_active = self
            .setting_value_with_conn(conn, "openai_ocr_active_profile")?
            .unwrap_or_default();
        let active_profile_id = if profiles.iter().any(|profile| profile.id == stored_active) {
            stored_active
        } else {
            profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_default()
        };

        for profile in &mut profiles {
            profile.api_key =
                match super::super::secrets::get_openai_ocr_profile_api_key(&profile.id) {
                    Ok(value) => value.unwrap_or_default(),
                    Err(reason) => {
                        eprintln!("[cloud-ocr] 读取系统凭据库失败（{}）：{reason}", profile.id);
                        String::new()
                    }
                };
        }

        let active = profiles
            .iter()
            .find(|profile| profile.id == active_profile_id);
        let openai_base_url = active
            .map(|profile| profile.base_url.clone())
            .unwrap_or_default();
        let openai_model = active
            .map(|profile| profile.model.clone())
            .unwrap_or_default();
        let openai_api_key = active
            .map(|profile| profile.api_key.clone())
            .unwrap_or_default();
        Ok(CloudOcrSettings {
            openai_profiles: profiles,
            openai_active_profile_id: active_profile_id,
            openai_base_url,
            openai_model,
            openai_api_key,
            openai_prompts,
        })
    }

    /// OCR 测试连接的哨兵解析用：当前激活配置的存储 Key。
    /// 无激活配置时回退旧单配置凭据；都没有返回空串（本地无鉴权端点允许空 Key）。
    // clippy 门禁豁免：Task 4 接线前的中间态，接线后移除。
    #[allow(dead_code)]
    pub(crate) fn active_openai_ocr_profile_secret(&self) -> Result<String, String> {
        let conn = self.connect()?;
        let active_id = self
            .setting_value_with_conn(&conn, "openai_ocr_active_profile")?
            .unwrap_or_default();
        if active_id.is_empty() {
            return Ok(super::super::secrets::get_openai_ocr_api_key()
                .unwrap_or_default()
                .unwrap_or_default());
        }
        Ok(
            super::super::secrets::get_openai_ocr_profile_api_key(&active_id)
                .unwrap_or_default()
                .unwrap_or_default(),
        )
    }

    fn openai_ocr_prompts_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<Vec<CloudOcrPromptMessage>, String> {
        Ok(
            match self.setting_value_with_conn(conn, "openai_ocr_prompts")? {
                Some(json_str) if !json_str.trim().is_empty() => {
                    serde_json::from_str::<Vec<CloudOcrPromptMessage>>(&json_str)
                        .unwrap_or_else(|_| crate::models::default_openai_ocr_prompts())
                }
                _ => crate::models::default_openai_ocr_prompts(),
            },
        )
    }

    /// 读取不含密钥的配置列表。有效 JSON（含空数组）即权威来源；
    /// 只有列表缺失或损坏时才回退旧单配置，避免误把已删空的历史配置复活。
    fn stored_openai_ocr_profiles_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<Vec<OpenaiOcrProfile>, String> {
        if let Some(raw) = self.setting_value_with_conn(conn, "openai_ocr_profiles")? {
            if let Ok(mut profiles) = serde_json::from_str::<Vec<OpenaiOcrProfile>>(&raw) {
                for profile in &mut profiles {
                    profile.api_key.clear();
                }
                return Ok(profiles);
            }
        }

        let base_url = self
            .setting_value_with_conn(conn, "openai_ocr_base_url")?
            .unwrap_or_default();
        let model = self
            .setting_value_with_conn(conn, "openai_ocr_model")?
            .unwrap_or_default();
        // 只有 Key 残留不足以构成一个接口；旧 UI 保存时三项必然齐全。
        if base_url.trim().is_empty() && model.trim().is_empty() {
            return Ok(Vec::new());
        }
        let api_key = super::super::secrets::get_openai_ocr_api_key()
            .unwrap_or_default()
            .unwrap_or_default();
        Ok(vec![OpenaiOcrProfile {
            id: super::super::secrets::LEGACY_OPENAI_OCR_PROFILE_ID.to_string(),
            name: if model.trim().is_empty() {
                "OpenAI".to_string()
            } else {
                model.clone()
            },
            base_url,
            model,
            api_key,
        }])
    }

    fn write_openai_ocr_profiles_with_conn(
        &self,
        conn: &Connection,
        profiles: &[OpenaiOcrProfile],
        active_profile_id: &str,
    ) -> Result<(), String> {
        let mut stored = profiles.to_vec();
        for profile in &mut stored {
            profile.api_key.clear();
        }
        let profiles_json = serde_json::to_string(&stored).map_err(|error| error.to_string())?;
        let active = profiles
            .iter()
            .find(|profile| profile.id == active_profile_id);
        for (key, value) in [
            ("openai_ocr_profiles", profiles_json),
            ("openai_ocr_active_profile", active_profile_id.to_string()),
            (
                "openai_ocr_base_url",
                active
                    .map(|profile| profile.base_url.clone())
                    .unwrap_or_default(),
            ),
            (
                "openai_ocr_model",
                active
                    .map(|profile| profile.model.clone())
                    .unwrap_or_default(),
            ),
            ("openai_api_key", String::new()),
        ] {
            self.upsert_setting_value_with_conn(conn, key, &value)?;
        }
        Ok(())
    }

    fn upsert_setting_value_with_conn(
        &self,
        conn: &Connection,
        key: &str,
        value: &str,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn save_openai_ocr_prompts_with_conn(
        &self,
        conn: &Connection,
        prompts: Option<Vec<CloudOcrPromptMessage>>,
    ) -> Result<(), String> {
        if let Some(prompts_list) = prompts {
            let filtered: Vec<CloudOcrPromptMessage> = prompts_list
                .into_iter()
                .filter(|message| !message.content.trim().is_empty())
                .collect();
            let effective = if filtered.is_empty() {
                crate::models::default_openai_ocr_prompts()
            } else {
                filtered
            };
            let prompts_json = serde_json::to_string(&effective).map_err(|e| e.to_string())?;
            self.upsert_setting_value_with_conn(conn, "openai_ocr_prompts", &prompts_json)?;
        }
        Ok(())
    }

    /// 新增或更新一个 OCR 接口。空 id 生成稳定 UUID；首个配置自动成为主接口，
    /// 后续新增不会抢走当前主接口。Key 写独立凭据账户，列表只落非敏感字段。
    pub(crate) fn save_openai_ocr_profile(
        &self,
        profile: OpenaiOcrProfile,
        prompts: Option<Vec<CloudOcrPromptMessage>>,
    ) -> Result<AppSettings, String> {
        let base_url = crate::util::clean_openai_base_url(profile.base_url)?;
        let model = crate::util::clean_openai_model(profile.model)?;
        // 哨兵延迟解析：profile_id 在下方才确定（新建配置的哨兵视为未填写）。
        // 本地无鉴权端点允许空 Key；留空即清除该配置的凭据。
        let raw_api_key = profile.api_key.trim().to_string();
        let requested_id = profile.id.trim().to_string();
        let conn = self.connect()?;
        let mut profiles = self.stored_openai_ocr_profiles_with_conn(&conn)?;
        let stored_active = self
            .setting_value_with_conn(&conn, "openai_ocr_active_profile")?
            .unwrap_or_default();
        let mut active_profile_id = if profiles.iter().any(|item| item.id == stored_active) {
            stored_active
        } else {
            profiles
                .first()
                .map(|item| item.id.clone())
                .unwrap_or_default()
        };

        let profile_id = if requested_id.is_empty() {
            let id = crate::util::new_id();
            profiles.push(OpenaiOcrProfile {
                id: id.clone(),
                name: String::new(),
                base_url: String::new(),
                model: String::new(),
                api_key: String::new(),
            });
            id
        } else {
            let Some(existing) = profiles.iter_mut().find(|item| item.id == requested_id) else {
                return Err("接口配置不存在，请刷新后重试".to_string());
            };
            existing.id.clone()
        };

        let api_key = match crate::util::resolve_secret_input(&raw_api_key) {
            crate::util::ResolvedSecret::KeepExisting => {
                if requested_id.is_empty() {
                    // 新建配置收到哨兵视为未填写
                    String::new()
                } else {
                    super::super::secrets::get_openai_ocr_profile_api_key(&profile_id)
                        .unwrap_or_default()
                        .unwrap_or_default()
                }
            }
            crate::util::ResolvedSecret::Clear => String::new(),
            crate::util::ResolvedSecret::Set(value) => value,
        };

        let name = profile.name.trim();
        let cleaned = OpenaiOcrProfile {
            id: profile_id.clone(),
            name: if name.is_empty() {
                model.clone()
            } else {
                name.to_string()
            },
            base_url,
            model,
            api_key: api_key.clone(),
        };
        if let Some(existing) = profiles.iter_mut().find(|item| item.id == profile_id) {
            *existing = cleaned;
        } else {
            profiles.push(cleaned);
        }
        if active_profile_id.is_empty() {
            active_profile_id = profile_id.clone();
        }

        if api_key.is_empty() {
            super::super::secrets::delete_openai_ocr_profile_api_key(&profile_id)?;
        } else {
            super::super::secrets::put_openai_ocr_profile_api_key(&profile_id, &api_key)?;
        }
        self.save_openai_ocr_prompts_with_conn(&conn, prompts)?;
        self.write_openai_ocr_profiles_with_conn(&conn, &profiles, &active_profile_id)?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn set_openai_ocr_active_profile(
        &self,
        profile_id: String,
    ) -> Result<AppSettings, String> {
        let conn = self.connect()?;
        let profiles = self.stored_openai_ocr_profiles_with_conn(&conn)?;
        if !profiles.iter().any(|profile| profile.id == profile_id) {
            return Err("接口配置不存在，请刷新后重试".to_string());
        }
        self.write_openai_ocr_profiles_with_conn(&conn, &profiles, &profile_id)?;
        self.settings_with_conn(&conn)
    }

    /// 删除接口及独立 Key；删主接口后自动切换到剩余第一个，允许删空。
    pub(crate) fn delete_openai_ocr_profile(
        &self,
        profile_id: String,
    ) -> Result<AppSettings, String> {
        let conn = self.connect()?;
        let mut profiles = self.stored_openai_ocr_profiles_with_conn(&conn)?;
        let Some(index) = profiles.iter().position(|profile| profile.id == profile_id) else {
            return self.settings_with_conn(&conn);
        };
        profiles.remove(index);
        let stored_active = self
            .setting_value_with_conn(&conn, "openai_ocr_active_profile")?
            .unwrap_or_default();
        let active_profile_id = if profiles.iter().any(|profile| profile.id == stored_active) {
            stored_active
        } else {
            profiles
                .first()
                .map(|profile| profile.id.clone())
                .unwrap_or_default()
        };

        super::super::secrets::delete_openai_ocr_profile_api_key(&profile_id)?;
        self.write_openai_ocr_profiles_with_conn(&conn, &profiles, &active_profile_id)?;
        self.settings_with_conn(&conn)
    }

    /// 旧单配置写入口：保留给旧前端/命令契约。已有新列表时更新主接口；
    /// 仍处于旧存储形态时沿用原 KV，不生成新列表、不做破坏性迁移。
    pub(crate) fn update_openai_ocr_config(
        &self,
        base_url: String,
        model: String,
        api_key: String,
        prompts: Option<Vec<CloudOcrPromptMessage>>,
    ) -> Result<AppSettings, String> {
        let base_url = crate::util::clean_openai_base_url(base_url)?;
        let model = crate::util::clean_openai_model(model)?;
        // 本地无鉴权端点允许空 Key；留空即清除旧单配置的凭据。
        let api_key = api_key.trim().to_string();
        let conn = self.connect()?;
        if self
            .setting_value_with_conn(&conn, "openai_ocr_profiles")?
            .as_deref()
            .and_then(|raw| serde_json::from_str::<Vec<OpenaiOcrProfile>>(raw).ok())
            .is_none()
        {
            let api_key = match crate::util::resolve_secret_input(&api_key) {
                crate::util::ResolvedSecret::KeepExisting => {
                    super::super::secrets::get_openai_ocr_api_key()
                        .unwrap_or_default()
                        .unwrap_or_default()
                }
                crate::util::ResolvedSecret::Clear => String::new(),
                crate::util::ResolvedSecret::Set(value) => value,
            };
            if api_key.is_empty() {
                super::super::secrets::delete_openai_ocr_api_key()?;
            } else {
                super::super::secrets::put_openai_ocr_api_key(&api_key)?;
            }
            self.upsert_setting_value_with_conn(&conn, "openai_ocr_base_url", &base_url)?;
            self.upsert_setting_value_with_conn(&conn, "openai_ocr_model", &model)?;
            self.upsert_setting_value_with_conn(&conn, "openai_api_key", "")?;
            self.save_openai_ocr_prompts_with_conn(&conn, prompts)?;
            return self.settings_with_conn(&conn);
        }

        let mut profiles = self.stored_openai_ocr_profiles_with_conn(&conn)?;
        let active_profile_id = self
            .setting_value_with_conn(&conn, "openai_ocr_active_profile")?
            .filter(|id| profiles.iter().any(|profile| profile.id == *id))
            .or_else(|| profiles.first().map(|profile| profile.id.clone()))
            .unwrap_or_default();
        if profiles.is_empty() {
            return self.save_openai_ocr_profile(
                OpenaiOcrProfile {
                    id: String::new(),
                    name: String::new(),
                    base_url,
                    model,
                    api_key,
                },
                prompts,
            );
        }
        let Some(profile) = profiles
            .iter_mut()
            .find(|profile| profile.id == active_profile_id)
        else {
            return Err("接口配置不存在，请刷新后重试".to_string());
        };
        let api_key = match crate::util::resolve_secret_input(&api_key) {
            crate::util::ResolvedSecret::KeepExisting => {
                super::super::secrets::get_openai_ocr_profile_api_key(&active_profile_id)
                    .unwrap_or_default()
                    .unwrap_or_default()
            }
            crate::util::ResolvedSecret::Clear => String::new(),
            crate::util::ResolvedSecret::Set(value) => value,
        };
        profile.base_url = base_url;
        profile.model = model.clone();
        profile.api_key = api_key.clone();
        if profile.name.trim().is_empty() {
            profile.name = model;
        }

        if api_key.is_empty() {
            super::super::secrets::delete_openai_ocr_profile_api_key(&active_profile_id)?;
        } else {
            super::super::secrets::put_openai_ocr_profile_api_key(&active_profile_id, &api_key)?;
        }
        self.save_openai_ocr_prompts_with_conn(&conn, prompts)?;
        self.write_openai_ocr_profiles_with_conn(&conn, &profiles, &active_profile_id)?;
        self.settings_with_conn(&conn)
    }

    /// 清除全部 OpenAI 兼容接口配置（逐个删独立凭据 + 清全部 KV）。
    pub(crate) fn clear_openai_ocr_config(&self) -> Result<AppSettings, String> {
        let conn = self.connect()?;
        let profiles = self.stored_openai_ocr_profiles_with_conn(&conn)?;
        for profile in profiles {
            super::super::secrets::delete_openai_ocr_profile_api_key(&profile.id)?;
        }
        super::super::secrets::delete_openai_ocr_api_key()?;
        conn.execute(
            "DELETE FROM settings WHERE key IN ('openai_ocr_profiles', 'openai_ocr_active_profile', 'openai_ocr_base_url', 'openai_ocr_model', 'openai_api_key', 'openai_ocr_prompts')",
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

/// 面向前端的设置出口统一掩码：非空 API Key → `MASKED_SECRET`，空 → 空串。
/// 内部消费方（sync_cloud、OCR 引擎）走 `settings_unmasked_with_conn` 拿明文。
pub(crate) fn mask_app_settings_secrets(settings: &mut AppSettings) {
    if !settings.cloud.api_key.is_empty() {
        settings.cloud.api_key = crate::util::MASKED_SECRET.to_string();
    }
    if !settings.cloud_ocr.openai_api_key.is_empty() {
        settings.cloud_ocr.openai_api_key = crate::util::MASKED_SECRET.to_string();
    }
    for profile in &mut settings.cloud_ocr.openai_profiles {
        if !profile.api_key.is_empty() {
            profile.api_key = crate::util::MASKED_SECRET.to_string();
        }
    }
}

#[cfg(test)]
mod secret_mask_tests {
    use super::*;
    use crate::store::test_support::{temp_store, KEYRING_MOCK_LOCK};

    #[test]
    fn settings_with_conn_masks_secret_and_unmasked_keeps_plaintext() {
        let store = temp_store();
        store
            .save_openai_ocr_profile(
                OpenaiOcrProfile {
                    id: String::new(),
                    name: "main".to_string(),
                    base_url: "http://127.0.0.1:1234/v1".to_string(),
                    model: "test-model".to_string(),
                    api_key: "sk-plain-1".to_string(),
                },
                None,
            )
            .unwrap();

        let masked = store.settings().unwrap();
        assert_eq!(masked.cloud_ocr.openai_api_key, crate::util::MASKED_SECRET);
        assert_eq!(
            masked.cloud_ocr.openai_profiles[0].api_key,
            crate::util::MASKED_SECRET
        );

        let conn = store.connect().unwrap();
        let unmasked = store.settings_unmasked_with_conn(&conn).unwrap();
        assert_eq!(unmasked.cloud_ocr.openai_api_key, "sk-plain-1");
        assert_eq!(unmasked.cloud_ocr.openai_profiles[0].api_key, "sk-plain-1");
    }

    #[test]
    fn mask_app_settings_secrets_keeps_empty_as_empty() {
        let mut settings = AppSettings {
            shortcut: String::new(),
            ocr_shortcut: String::new(),
            retention_days: crate::DEFAULT_RETENTION_DAYS,
            append_copy_timeout_minutes: crate::DEFAULT_APPEND_COPY_TIMEOUT_MINUTES,
            panel_open_behavior: crate::models::PanelOpenBehavior::default_value(),
            panel_layout: crate::models::PanelLayout::default_value(),
            ocr_mode: crate::models::OcrMode::default_value(),
            ocr_engine: crate::models::OcrEngine::default_value(),
            language: crate::models::Language::default_value(),
            cloud: CloudSettings {
                api_address: String::new(),
                api_key: String::new(),
                enabled: false,
                last_connected_at: None,
            },
            cloud_ocr: CloudOcrSettings {
                openai_profiles: Vec::new(),
                openai_active_profile_id: String::new(),
                openai_base_url: String::new(),
                openai_model: String::new(),
                openai_api_key: String::new(),
                openai_prompts: Vec::new(),
            },
        };
        super::mask_app_settings_secrets(&mut settings);
        assert_eq!(settings.cloud.api_key, "");
        assert_eq!(settings.cloud_ocr.openai_api_key, "");
    }

    #[test]
    fn update_openai_ocr_config_sentinel_keeps_stored_key() {
        // 本测试写读 legacy 共享凭据账户：与 tests.rs 的 keyring 测试共用
        // test_support 的锁串行，mock 凭据库是进程级共享内存。
        let _guard = KEYRING_MOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = temp_store();
        store
            .update_openai_ocr_config(
                "http://127.0.0.1:1234/v1".into(),
                "test-model".into(),
                "sk-orig".into(),
                None,
            )
            .unwrap();

        // 哨兵回传：凭据保留
        store
            .update_openai_ocr_config(
                "http://127.0.0.1:1234/v1".into(),
                "test-model".into(),
                crate::util::MASKED_SECRET.into(),
                None,
            )
            .unwrap();
        let conn = store.connect().unwrap();
        let unmasked = store.settings_unmasked_with_conn(&conn).unwrap();
        assert_eq!(unmasked.cloud_ocr.openai_api_key, "sk-orig");

        // 空串回传：凭据清除（本地端点允许空 Key）
        store
            .update_openai_ocr_config(
                "http://127.0.0.1:1234/v1".into(),
                "test-model".into(),
                String::new(),
                None,
            )
            .unwrap();
        let unmasked = store.settings_unmasked_with_conn(&conn).unwrap();
        assert_eq!(unmasked.cloud_ocr.openai_api_key, "");
    }

    #[test]
    fn save_openai_ocr_profile_sentinel_keeps_stored_key_and_new_profile_treats_as_empty() {
        // 写读独立凭据账户，统一按 keyring 相关测试约定上锁串行。
        let _guard = KEYRING_MOCK_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let store = temp_store();
        let created = store
            .save_openai_ocr_profile(
                OpenaiOcrProfile {
                    id: String::new(),
                    name: String::new(),
                    base_url: "http://127.0.0.1:1234/v1".to_string(),
                    model: "test-model".to_string(),
                    api_key: "sk-profile-1".to_string(),
                },
                None,
            )
            .unwrap();
        let profile_id = created.cloud_ocr.openai_active_profile_id.clone();

        // 编辑现有配置回传哨兵：Key 保留
        store
            .save_openai_ocr_profile(
                OpenaiOcrProfile {
                    id: profile_id.clone(),
                    name: "renamed".to_string(),
                    base_url: "http://127.0.0.1:1234/v1".to_string(),
                    model: "test-model".to_string(),
                    api_key: crate::util::MASKED_SECRET.to_string(),
                },
                None,
            )
            .unwrap();
        let conn = store.connect().unwrap();
        let unmasked = store.settings_unmasked_with_conn(&conn).unwrap();
        assert_eq!(
            unmasked.cloud_ocr.openai_profiles[0].api_key,
            "sk-profile-1"
        );
        assert_eq!(unmasked.cloud_ocr.openai_profiles[0].name, "renamed");

        // 新建配置回传哨兵：视为未填写（空 Key）
        store
            .save_openai_ocr_profile(
                OpenaiOcrProfile {
                    id: String::new(),
                    name: "second".to_string(),
                    base_url: "http://127.0.0.1:2345/v1".to_string(),
                    model: "test-model-2".to_string(),
                    api_key: crate::util::MASKED_SECRET.to_string(),
                },
                None,
            )
            .unwrap();
        let unmasked = store.settings_unmasked_with_conn(&conn).unwrap();
        let second = unmasked
            .cloud_ocr
            .openai_profiles
            .iter()
            .find(|p| p.model == "test-model-2")
            .unwrap();
        assert_eq!(second.api_key, "");
    }
}
