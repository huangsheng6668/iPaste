// store/migrations.rs — schema 迁移 + seed_default_clips
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::{
    clipboard::image_bytes_from_data_url,
    util::{hash_text, new_id, preview},
    DEFAULT_CLIPBOARD_SEEDS,
};

fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<(), String> {
    let exists = table_column_names(conn, table)?
        .iter()
        .any(|name| name == column);

    if !exists {
        conn.execute(
            &format!("ALTER TABLE {table} ADD COLUMN {column} {definition}"),
            [],
        )
        .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn table_column_names(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(|error| error.to_string())?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|error| error.to_string())?;
    columns
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| error.to_string())
}

impl Store {
    pub(super) fn migrate(&self, conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS clips (
                id TEXT PRIMARY KEY,
                clip_type TEXT NOT NULL,
                content_hash TEXT NOT NULL UNIQUE,
                display_name TEXT,
                preview_text TEXT NOT NULL,
                text TEXT NOT NULL,
                source_app TEXT,
                last_captured_at TEXT NOT NULL,
                favorite_count INTEGER NOT NULL DEFAULT 0,
                is_pinned INTEGER NOT NULL DEFAULT 0
            );

            -- 历史列表 ORDER BY datetime(last_captured_at) DESC 的表达式索引。
            -- 列里混有两种格式：now() 写 RFC3339（Z 后缀），旧版迁移回填写
            -- datetime('now') 的 'YYYY-MM-DD HH:MM:SS'——裸列排序会乱序，
            -- datetime() 归一化后入索引才能正确比较。同时替掉没有任何查询
            -- 使用者的裸列索引 idx_clips_last_captured_at（每次捕获都白维护它）。
            DROP INDEX IF EXISTS idx_clips_last_captured_at;
            CREATE INDEX IF NOT EXISTS idx_clips_last_captured_at_dt
                ON clips(datetime(last_captured_at) DESC);

            CREATE TABLE IF NOT EXISTS categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                color TEXT NOT NULL,
                sort_order INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS category_items (
                id TEXT PRIMARY KEY,
                category_id TEXT NOT NULL,
                clip_snapshot_id TEXT NOT NULL,
                clip_type TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                display_name TEXT,
                preview_text TEXT NOT NULL,
                text TEXT NOT NULL,
                sort_order INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                sync_state TEXT NOT NULL DEFAULT 'local',
                is_pinned INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY(category_id) REFERENCES categories(id) ON DELETE CASCADE
            );

            CREATE UNIQUE INDEX IF NOT EXISTS idx_category_items_unique_clip
                ON category_items(category_id, content_hash);
            CREATE INDEX IF NOT EXISTS idx_category_items_category ON category_items(category_id, sort_order);

            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS paired_devices (
                node_id TEXT PRIMARY KEY,
                device_name TEXT NOT NULL,
                relay_url TEXT,
                direct_addrs TEXT NOT NULL DEFAULT '[]',
                auto_sync_mode TEXT NOT NULL DEFAULT 'text_only',
                added_at TEXT NOT NULL,
                last_seen_at TEXT,
                revoked_at TEXT
            );

            CREATE TABLE IF NOT EXISTS sync_tombstones (
                entity TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY(entity, entity_id)
            );

            CREATE TABLE IF NOT EXISTS automations (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                command TEXT NOT NULL,
                cwd TEXT,
                run_mode TEXT NOT NULL DEFAULT 'background',
                confirm_before_run INTEGER NOT NULL DEFAULT 0,
                close_panel_on_success INTEGER NOT NULL DEFAULT 0,
                sort_order INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS automation_runs (
                id TEXT PRIMARY KEY,
                automation_id TEXT NOT NULL,
                status TEXT NOT NULL,
                exit_code INTEGER,
                stdout TEXT NOT NULL DEFAULT '',
                stderr TEXT NOT NULL DEFAULT '',
                started_at TEXT NOT NULL,
                finished_at TEXT,
                duration_ms INTEGER,
                FOREIGN KEY(automation_id) REFERENCES automations(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_automations_sort_order ON automations(sort_order, created_at);

            CREATE INDEX IF NOT EXISTS idx_automation_runs_automation_started ON automation_runs(automation_id, started_at DESC);
            ",
        )
        .map_err(|error| error.to_string())?;

        add_column_if_missing(conn, "clips", "display_name", "TEXT")?;
        self.migrate_clip_last_captured_at(conn)?;
        add_column_if_missing(conn, "clips", "is_pinned", "INTEGER NOT NULL DEFAULT 0")?;
        self.remove_legacy_clip_columns(conn)?;
        add_column_if_missing(conn, "category_items", "display_name", "TEXT")?;
        add_column_if_missing(
            conn,
            "category_items",
            "is_pinned",
            "INTEGER NOT NULL DEFAULT 0",
        )?;

        self.migrate_image_data_urls(conn)?;
        self.normalize_timestamp_columns(conn)?;
        self.setup_clips_fts(conn)?;
        self.remove_empty_default_categories(conn)
    }

    /// 历史搜索的 FTS5 全文索引（trigram 分词）：
    /// - 外部内容表（content='clips'）：索引只存 token，不复制正文；
    /// - text 列对图片行为空（与 LIKE 语义一致——图片的 text 是文件路径，
    ///   不应被搜到；图片靠 clip_type='image' / 「图片 image」标签命中）；
    /// - 触发器按列限定（UPDATE OF ...）：高频的 last_captured_at 触碰
    ///   不触发重建索引；
    /// - 存量一次性回填，标志门控；此后增删改由触发器同步。
    ///
    /// 注意：trigram 最短命中 3 字符，更短的查询（如中文两字词）由查询层
    /// 回退 LIKE 扫描。display_name/preview_text/text 与 LIKE 检索的三列
    /// 完全一致，FTS 命中集与原子串语义等价（大小写不敏感子串匹配）。
    fn setup_clips_fts(&self, conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS clips_fts USING fts5(
                display_name, preview_text, text,
                content='clips', content_rowid='rowid',
                tokenize='trigram'
            );
            CREATE TRIGGER IF NOT EXISTS clips_fts_ai AFTER INSERT ON clips BEGIN
                INSERT INTO clips_fts(rowid, display_name, preview_text, text)
                VALUES (new.rowid, COALESCE(new.display_name, ''), new.preview_text,
                        CASE WHEN new.clip_type != 'image' THEN new.text ELSE '' END);
            END;
            CREATE TRIGGER IF NOT EXISTS clips_fts_ad AFTER DELETE ON clips BEGIN
                INSERT INTO clips_fts(clips_fts, rowid, display_name, preview_text, text)
                VALUES ('delete', old.rowid, COALESCE(old.display_name, ''), old.preview_text,
                        CASE WHEN old.clip_type != 'image' THEN old.text ELSE '' END);
            END;
            CREATE TRIGGER IF NOT EXISTS clips_fts_au
            AFTER UPDATE OF display_name, preview_text, text, clip_type ON clips BEGIN
                INSERT INTO clips_fts(clips_fts, rowid, display_name, preview_text, text)
                VALUES ('delete', old.rowid, COALESCE(old.display_name, ''), old.preview_text,
                        CASE WHEN old.clip_type != 'image' THEN old.text ELSE '' END);
                INSERT INTO clips_fts(rowid, display_name, preview_text, text)
                VALUES (new.rowid, COALESCE(new.display_name, ''), new.preview_text,
                        CASE WHEN new.clip_type != 'image' THEN new.text ELSE '' END);
            END;",
        )
        .map_err(|error| error.to_string())?;

        let populated: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migrated_clips_fts'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if populated.as_deref() != Some("1") {
            conn.execute(
                "INSERT INTO clips_fts(rowid, display_name, preview_text, text)
                 SELECT rowid, COALESCE(display_name, ''), preview_text,
                        CASE WHEN clip_type != 'image' THEN text ELSE '' END
                 FROM clips",
                [],
            )
            .map_err(|error| error.to_string())?;
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('migrated_clips_fts', '1')
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [],
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// 时间列格式规范化：把历史版本可能写入的 "YYYY-MM-DD HH:MM:SS"
    /// （SQLite datetime() 风格）或带 +00:00 偏移的值统一成 now() 的
    /// 定长 "YYYY-MM-DDTHH:MM:SSZ"。定长 UTC 格式下字典序即时间序，
    /// 查询层才能去掉 datetime() 包裹、直接命中 last_captured_at 等索引。
    ///
    /// 幂等且无损：仅重写形状不符的值；strftime 解析不了的值原样保留。
    fn normalize_timestamp_columns(&self, conn: &Connection) -> Result<(), String> {
        for (table, column) in [
            ("clips", "last_captured_at"),
            ("categories", "created_at"),
            ("categories", "updated_at"),
            ("category_items", "created_at"),
            ("category_items", "updated_at"),
            ("automation_runs", "started_at"),
            ("automation_runs", "finished_at"),
        ] {
            conn.execute(
                &format!(
                    "UPDATE {table} SET {column} = strftime('%Y-%m-%dT%H:%M:%SZ', {column})
                     WHERE {column} IS NOT NULL
                       AND {column} != ''
                       AND {column} NOT GLOB '____-__-__T__:__:__Z'
                       AND strftime('%Y-%m-%dT%H:%M:%SZ', {column}) IS NOT NULL"
                ),
                [],
            )
            .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) fn seed_default_clips(&self, conn: &Connection) -> Result<(), String> {
        if self.clip_total_count_with_conn(conn)? > 0 {
            return Ok(());
        }

        let seeded_at = Utc::now();
        for (index, (clip_type, display_name, text)) in DEFAULT_CLIPBOARD_SEEDS.iter().enumerate() {
            let last_captured_at = (seeded_at - ChronoDuration::seconds(index as i64))
                .to_rfc3339_opts(SecondsFormat::Secs, true);
            let content_hash = hash_text(&format!("ipaste-default-seed:{clip_type}:{text}"));
            conn.execute(
                "INSERT OR IGNORE INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 0, 0)",
                params![
                    new_id(),
                    clip_type,
                    content_hash,
                    display_name,
                    preview(text),
                    text,
                    "iPaste",
                    last_captured_at
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn migrate_image_data_urls(&self, conn: &Connection) -> Result<(), String> {
        // 这一步要全表 LIKE 扫描两遍，只为找「还没落盘的 data-url 图片行」
        //（老版本行为，新捕获一律直接落文件）。完成后用标志位门控，启动不再
        // 白扫；迁移本体幂等，中途被杀（标志未置位）下次会安全重跑。
        let migrated: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = 'migrated_image_data_urls'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        if migrated.as_deref() == Some("1") {
            return Ok(());
        }

        self.migrate_image_data_urls_for_table(conn, "clips")?;
        self.migrate_image_data_urls_for_table(conn, "category_items")?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('migrated_image_data_urls', '1')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }


    fn migrate_image_data_urls_for_table(
        &self,
        conn: &Connection,
        table: &str,
    ) -> Result<(), String> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT id, content_hash, text FROM {table}
                 WHERE clip_type = 'image' AND text LIKE 'data:image/%;base64,%'"
            ))
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;
        let items = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())?;

        for (id, content_hash, data_url) in items {
            let bytes = image_bytes_from_data_url(&data_url)?;
            let path = self.save_image_bytes(&content_hash, &bytes)?;
            conn.execute(
                &format!("UPDATE {table} SET text = ?1 WHERE id = ?2"),
                params![path, id],
            )
            .map_err(|error| error.to_string())?;
        }

        Ok(())
    }

    fn remove_empty_default_categories(&self, conn: &Connection) -> Result<(), String> {
        conn.execute(
            "DELETE FROM categories
             WHERE name IN ('Favorites', '收藏')
               AND NOT EXISTS (
                   SELECT 1 FROM category_items WHERE category_items.category_id = categories.id
               )",
            [],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn remove_legacy_clip_columns(&self, conn: &Connection) -> Result<(), String> {
        let columns = table_column_names(conn, "clips")?;
        let has_created_at = columns.iter().any(|name| name == "created_at");
        let has_last_used_at = columns.iter().any(|name| name == "last_used_at");
        if !has_created_at && !has_last_used_at {
            return Ok(());
        }

        conn.execute("DROP TABLE IF EXISTS clips_next", [])
            .map_err(|error| error.to_string())?;
        conn.execute(
            "
            CREATE TABLE clips_next (
                id TEXT PRIMARY KEY,
                clip_type TEXT NOT NULL,
                content_hash TEXT NOT NULL UNIQUE,
                display_name TEXT,
                preview_text TEXT NOT NULL,
                text TEXT NOT NULL,
                source_app TEXT,
                last_captured_at TEXT NOT NULL,
                favorite_count INTEGER NOT NULL DEFAULT 0,
                is_pinned INTEGER NOT NULL DEFAULT 0
            )
            ",
            [],
        )
        .map_err(|error| error.to_string())?;

        conn.execute(
            "
            INSERT INTO clips_next (
                id,
                clip_type,
                content_hash,
                display_name,
                preview_text,
                text,
                source_app,
                last_captured_at,
                favorite_count,
                is_pinned
            )
            SELECT
                id,
                clip_type,
                content_hash,
                display_name,
                preview_text,
                text,
                source_app,
                last_captured_at,
                favorite_count,
                is_pinned
            FROM clips
            ",
            [],
        )
        .map_err(|error| error.to_string())?;
        conn.execute("DROP TABLE clips", [])
            .map_err(|error| error.to_string())?;
        conn.execute("ALTER TABLE clips_next RENAME TO clips", [])
            .map_err(|error| error.to_string())?;
        // 表重建会带走旧索引；datetime() 表达式索引必须随之重建（见 migrate 批次）。
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_clips_last_captured_at_dt ON clips(datetime(last_captured_at) DESC)",
            [],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn migrate_clip_last_captured_at(&self, conn: &Connection) -> Result<(), String> {
        let columns = table_column_names(conn, "clips")?;
        let has_created_at = columns.iter().any(|name| name == "created_at");
        let has_last_used_at = columns.iter().any(|name| name == "last_used_at");
        let captured_at_source = match (has_last_used_at, has_created_at) {
            (true, true) => "COALESCE(last_used_at, created_at, datetime('now'))",
            (true, false) => "COALESCE(last_used_at, datetime('now'))",
            (false, true) => "COALESCE(created_at, datetime('now'))",
            (false, false) => "datetime('now')",
        };

        if !columns.iter().any(|name| name == "last_captured_at") {
            conn.execute("ALTER TABLE clips ADD COLUMN last_captured_at TEXT", [])
                .map_err(|error| error.to_string())?;
        }
        conn.execute(
            &format!(
                "UPDATE clips
                 SET last_captured_at = {captured_at_source}
                 WHERE last_captured_at IS NULL OR last_captured_at = ''"
            ),
            [],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_clips_last_captured_at_dt ON clips(datetime(last_captured_at) DESC)",
            [],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::params;

    use crate::store::test_support::temp_store;

    /// 时间戳规范化：SQLite datetime() 风格与带偏移的值统一为定长 "…Z"；
    /// 已符合格式或无法解析的值保持原样。
    #[test]
    fn normalize_timestamp_columns_rewrites_legacy_formats() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        for (id, raw) in [
            ("legacy", "2024-01-02 03:04:05"),
            ("offset", "2024-01-02T03:04:05+00:00"),
            ("garbage", "not-a-date"),
            ("conforming", "2024-01-02T03:04:05Z"),
        ] {
            conn.execute(
                "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
                 VALUES (?1, 'text', ?2, NULL, 'p', 't', NULL, ?3, 0, 0)",
                params![id, format!("hash-normalize-{id}"), raw],
            )
            .unwrap();
        }

        store.normalize_timestamp_columns(&conn).unwrap();

        let expected = [
            ("legacy", "2024-01-02T03:04:05Z"),
            ("offset", "2024-01-02T03:04:05Z"),
            ("garbage", "not-a-date"),
            ("conforming", "2024-01-02T03:04:05Z"),
        ];
        for (id, want) in expected {
            let got: String = conn
                .query_row(
                    "SELECT last_captured_at FROM clips WHERE id = ?1",
                    params![id],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(got, want, "id = {id}");
        }
    }
}
