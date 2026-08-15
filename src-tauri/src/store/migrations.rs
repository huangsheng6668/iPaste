// store/migrations.rs — schema 迁移 + seed_default_clips
use chrono::{Duration as ChronoDuration, SecondsFormat, Utc};
use rusqlite::{params, Connection};

use crate::util::*;
use super::Store;
use crate::{
    clipboard::image_bytes_from_data_url, util::new_id, DEFAULT_CLIPBOARD_SEEDS,
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
        self.remove_empty_default_categories(conn)
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
        self.migrate_image_data_urls_for_table(conn, "clips")?;
        self.migrate_image_data_urls_for_table(conn, "category_items")
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
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_clips_last_captured_at ON clips(last_captured_at DESC)",
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
            "CREATE INDEX IF NOT EXISTS idx_clips_last_captured_at ON clips(last_captured_at DESC)",
            [],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }
}
