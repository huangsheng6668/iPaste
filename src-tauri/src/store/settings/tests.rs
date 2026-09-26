//! 设置读写的单元测试（Task 32 从 store/settings.rs 原样外移）。

/// registry 自洽性：每个字符串型设置项的默认值都必须能通过它自己的清洗器，
/// 且明显非法的值不得被原样接受。新增设置项写错规格时这条会立刻失败。
#[test]
fn string_setting_specs_are_self_consistent() {
    for spec in crate::store::settings::registry::STRING_SETTINGS {
        assert!(
            (spec.clean)(spec.default.to_string()).is_ok(),
            "默认值未通过清洗器：{}",
            spec.key
        );

        let bogus = "\u{0}\u{1}bogus".to_string();
        if let Ok(cleaned) = (spec.clean)(bogus.clone()) {
            assert_ne!(cleaned, bogus, "非法值被原样接受：{}", spec.key);
        }
    }
}

/// 全字段快照：`settings()` 的序列化输出必须逐字段稳定。
/// 这是设置 registry 化（Task 35/36）与任何读取路径调整的等价性判据——
/// 指纹覆盖全部字段（含云端 OCR 的默认 Prompt 文案）。
#[test]
fn settings_snapshot_is_stable() {
    let store = crate::store::test_support::temp_store();
    let settings = store.settings().unwrap();

    // 结构化断言：默认值逐一列出，失败时能直接看出是哪个字段变了
    assert_eq!(settings.shortcut, "CommandOrControl+Shift+V");
    assert_eq!(settings.ocr_shortcut, "CommandOrControl+Shift+O");
    assert_eq!(settings.retention_days, 30);
    assert_eq!(settings.append_copy_timeout_minutes, 1);
    assert_eq!(
        settings.panel_open_behavior,
        crate::models::PanelOpenBehavior::History
    );
    assert_eq!(settings.panel_layout.as_str(), "top");
    assert_eq!(settings.ocr_mode.as_str(), "fast");
    assert_eq!(settings.ocr_engine.as_str(), "local");
    assert_eq!(settings.language.as_str(), "en");
    assert_eq!(settings.cloud.api_address, "");
    assert!(!settings.cloud.enabled);
    assert!(settings.cloud.last_connected_at.is_none());
    assert_eq!(settings.cloud_ocr.openai_base_url, "");
    assert_eq!(settings.cloud_ocr.openai_model, "");
    assert_eq!(settings.cloud_ocr.openai_prompts.len(), 2);

    // 指纹：字段顺序、命名与默认 Prompt 文案的任何变化都会改变它。
    // 两个 api key 来自进程级共享的 mock keyring（同文件的 keyring 测试会并发写入），
    // 因此在克隆上清空它们再序列化：结果不依赖测试执行顺序，也不改变字段顺序。
    let mut masked = settings.clone();
    masked.cloud.api_key.clear();
    masked.cloud_ocr.openai_api_key.clear();
    let json = serde_json::to_string(&masked).unwrap();
    assert_eq!(json.len(), 827);
    assert_eq!(
        crate::util::hash_text(&json),
        "a1f61a3f6f7b059d427a3ca00161b083783071fbb99e91616c6eb4daa672b1f1"
    );
}

use crate::store::test_support::temp_store;

#[test]
fn settings_round_trip_for_enum_like_values() {
    let store = temp_store();

    let s = store.update_panel_layout("side".to_string()).unwrap();
    assert_eq!(s.panel_layout.as_str(), "side");

    let s = store.update_ocr_mode("best".to_string()).unwrap();
    assert_eq!(s.ocr_mode.as_str(), "best");

    let s = store
        .update_panel_open_behavior("last_selected".to_string())
        .unwrap();
    assert_eq!(
        s.panel_open_behavior,
        crate::models::PanelOpenBehavior::LastSelected
    );

    let s = store.update_language("zh-CN".to_string()).unwrap();
    assert_eq!(s.language.as_str(), "zh-CN");

    let s = store.settings().unwrap();
    assert_eq!(s.panel_layout.as_str(), "side");
    assert_eq!(s.ocr_mode.as_str(), "best");
    assert_eq!(
        s.panel_open_behavior,
        crate::models::PanelOpenBehavior::LastSelected
    );
    assert_eq!(s.language.as_str(), "zh-CN");
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
fn ocr_engine_round_trip_and_default() {
    let store = temp_store();

    // 缺省 local
    assert_eq!(store.settings().unwrap().ocr_engine.as_str(), "local");

    let s = store.update_ocr_engine("openai".to_string()).unwrap();
    assert_eq!(s.ocr_engine.as_str(), "openai");
    assert_eq!(store.settings().unwrap().ocr_engine.as_str(), "openai");

    // 非法值被拒绝且不落库
    assert!(store.update_ocr_engine("bigmodel".to_string()).is_err());
    assert_eq!(store.settings().unwrap().ocr_engine.as_str(), "openai");
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
    assert_eq!(
        store.sync_relay_url().unwrap(),
        None,
        "空白清除后读取为 None"
    );

    // 再次写入后显式 None 也清除
    store
        .update_sync_relay_url(Some("https://relay2.example.com"))
        .unwrap();
    let saved = store.update_sync_relay_url(None).unwrap();
    assert_eq!(saved, None);
    assert_eq!(store.sync_relay_url().unwrap(), None);
}

#[test]
fn sync_relay_url_rejects_non_https() {
    let store = temp_store();
    for bad in [
        "http://relay.example.com",
        "relay.example.com",
        "https-relay.example.com",
    ] {
        let error = store.update_sync_relay_url(Some(bad)).unwrap_err();
        assert!(error.contains("https"), "got: {error}");
    }
    assert_eq!(store.sync_relay_url().unwrap(), None, "拒绝的值不落库");
}

/// Task 40 枚举化的遗留数据保护：库里存着非法枚举字符串（历史版本写入）时，
/// 读取一律回落默认值而不是报错——迁移期不逼用户清库。
#[test]
fn legacy_invalid_enum_values_fall_back_to_defaults() {
    let store = temp_store();
    {
        let conn = store.connect().unwrap();
        for (key, value) in [
            ("panel_layout", "diagonal"),
            ("ocr_mode", "turbo"),
            ("ocr_engine", "bigmodel"),
            ("panel_open_behavior", "somewhere"),
            ("language", "klingon"),
        ] {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                rusqlite::params![key, value],
            )
            .unwrap();
        }
    }

    let s = store.settings().unwrap();
    assert_eq!(s.panel_layout.as_str(), "top");
    assert_eq!(s.ocr_mode.as_str(), "fast");
    assert_eq!(s.ocr_engine.as_str(), "local");
    assert_eq!(
        s.panel_open_behavior,
        crate::models::PanelOpenBehavior::History
    );
    assert_eq!(s.language.as_str(), "en");
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
        crate::models::AutoPushSettings {
            master: true,
            notify: false
        }
    );

    // 写后往返
    let saved = store.update_auto_push_settings(false, true).unwrap();
    assert_eq!(
        saved,
        crate::models::AutoPushSettings {
            master: false,
            notify: true
        }
    );
    assert_eq!(
        store.auto_push_settings().unwrap(),
        crate::models::AutoPushSettings {
            master: false,
            notify: true
        }
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
        crate::models::AutoPushSettings {
            master: true,
            notify: false
        },
        "坏值回退缺省"
    );
}

#[cfg(test)]
mod cloud_ocr_keyring_tests {
    use crate::store::secrets;
    use crate::store::test_support::temp_store;

    /// mock keyring 是进程级共享的内存后端，相关测试必须串行。
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn openai_ocr_config_round_trip_and_clear() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = secrets::delete_openai_ocr_api_key();
        let store = temp_store();

        let custom_prompts = vec![crate::models::CloudOcrPromptMessage {
            role: "user".to_string(),
            content: "Custom OCR for {recognition_language}.".to_string(),
        }];

        let s = store
            .update_openai_ocr_config(
                "  https://open.bigmodel.cn/api/paas/v4/  ".to_string(),
                "glm-4v-flash".to_string(),
                "  sk-test  ".to_string(),
                Some(custom_prompts.clone()),
            )
            .unwrap();
        assert_eq!(
            s.cloud_ocr.openai_base_url, "https://open.bigmodel.cn/api/paas/v4",
            "保存会 trim 斜杠"
        );
        assert_eq!(s.cloud_ocr.openai_model, "glm-4v-flash");
        assert_eq!(s.cloud_ocr.openai_api_key, "sk-test");
        assert_eq!(s.cloud_ocr.openai_prompts, custom_prompts);

        let reloaded = store.settings().unwrap();
        assert_eq!(
            reloaded.cloud_ocr.openai_base_url,
            "https://open.bigmodel.cn/api/paas/v4"
        );
        assert_eq!(
            reloaded.cloud_ocr.openai_api_key, "sk-test",
            "Key 从凭据库回填"
        );
        assert_eq!(reloaded.cloud_ocr.openai_prompts, custom_prompts);

        // 非法 Base URL / 空模型被拒绝且不落库
        assert!(store
            .update_openai_ocr_config(
                "ftp://x".to_string(),
                "m".to_string(),
                "k".to_string(),
                None
            )
            .is_err());
        assert!(store
            .update_openai_ocr_config(
                "https://x".to_string(),
                "  ".to_string(),
                "k".to_string(),
                None
            )
            .is_err());
        assert_eq!(
            store.settings().unwrap().cloud_ocr.openai_model,
            "glm-4v-flash"
        );

        let s = store.clear_openai_ocr_config().unwrap();
        assert_eq!(s.cloud_ocr.openai_base_url, "");
        assert_eq!(s.cloud_ocr.openai_model, "");
        assert_eq!(s.cloud_ocr.openai_api_key, "");
        assert_eq!(
            s.cloud_ocr.openai_prompts,
            crate::models::default_openai_ocr_prompts(),
            "清空后回退默认 prompt"
        );
        assert_eq!(secrets::get_openai_ocr_api_key().unwrap(), None);
    }

    #[test]
    fn cloud_ocr_engine_ready_requires_openai_config() {
        let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _ = secrets::delete_openai_ocr_api_key();
        let store = temp_store();

        assert!(!store.cloud_ocr_engine_ready(), "缺省 local 引擎不可用");

        store.update_ocr_engine("openai".to_string()).unwrap();
        assert!(!store.cloud_ocr_engine_ready(), "引擎已选但配置不全");
        store
            .update_openai_ocr_config(
                "https://api.example.com/v1".into(),
                "m".into(),
                "k".into(),
                None,
            )
            .unwrap();
        assert!(store.cloud_ocr_engine_ready());

        let _ = store.clear_openai_ocr_config();
        store.update_ocr_engine("local".to_string()).unwrap();
    }

    /// 历史遗留：库里有已移除引擎的 bigmodel_api_key 占位行时，读取设置会顺带清掉。
    #[test]
    fn legacy_bigmodel_row_is_cleaned_on_read() {
        let store = temp_store();
        {
            let conn = store.connect().unwrap();
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('bigmodel_api_key', '')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .unwrap();
        }
        let _ = store.settings().unwrap();
        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM settings WHERE key = 'bigmodel_api_key'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "遗留占位行应被清除");
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
        assert_eq!(
            secrets::get_api_key().unwrap().as_deref(),
            Some("legacy-plain-key")
        );
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
        assert!(
            cloud.enabled || cloud.api_address.is_empty(),
            "enabled 判定沿用地址+键非空"
        );
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
