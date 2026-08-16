// store/clips.rs — 剪贴板捕获/查询/CRUD
use std::fs;
use std::path::PathBuf;

use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::models::{CapturedClipboardItem, ClipItem, ClipPage, ClipUpdate, SearchResult};
use crate::{
    clipboard::image_bytes_from_data_url, store::rows::{collect_rows, map_clip},
    util::{clean_display_name, hash_text, new_id, now, preview, safe_filename}, IMAGE_DIR,
};

impl Store {
    pub(crate) fn insert_captured_item(
        &self,
        item: CapturedClipboardItem,
    ) -> Result<Option<(ClipItem, usize, bool)>, String> {
        let conn = self.connect()?;
        let existing: Option<ClipItem> = conn
            .query_row(
                "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
                 FROM clips WHERE content_hash = ?1",
                params![item.content_hash],
                map_clip,
            )
            .optional()
            .map_err(|error| error.to_string())?;

        if let Some(clip) = existing {
            let captured_at = now();
            conn.execute(
                "UPDATE clips SET last_captured_at = ?1 WHERE id = ?2",
                params![captured_at, clip.id],
            )
            .map_err(|error| error.to_string())?;
            // LAN 接收的重命名仅在本地还没有重命名时补齐，不覆盖用户自己的命名。
            if item.display_name.is_some() && clip.display_name.is_none() {
                conn.execute(
                    "UPDATE clips SET display_name = ?1 WHERE id = ?2",
                    params![&item.display_name, clip.id],
                )
                .map_err(|error| error.to_string())?;
            }
            let clip = self.get_clip_with_conn(&conn, &clip.id)?;
            return Ok(Some((clip, self.clip_total_count_with_conn(&conn)?, false)));
        }

        let text = if item.clip_type == "image" {
            match item.image_bytes.as_deref() {
                Some(bytes) => self.save_image_bytes(&item.content_hash, bytes)?,
                None if item.text.starts_with("data:image/") => {
                    let bytes = image_bytes_from_data_url(&item.text)?;
                    self.save_image_bytes(&item.content_hash, &bytes)?
                }
                None => item.text,
            }
        } else {
            item.text
        };

        let last_captured_at = now();
        let clip = ClipItem {
            id: new_id(),
            clip_type: item.clip_type,
            content_hash: item.content_hash,
            display_name: item.display_name,
            preview_text: item.preview_text,
            text,
            source_app: None,
            last_captured_at,
            favorite_count: 0,
            is_pinned: false,
        };

        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                clip.id,
                clip.clip_type,
                clip.content_hash,
                clip.display_name,
                clip.preview_text,
                clip.text,
                clip.source_app,
                clip.last_captured_at,
                clip.favorite_count,
                clip.is_pinned
            ],
        )
        .map_err(|error| error.to_string())?;

        Ok(Some((clip, self.clip_total_count_with_conn(&conn)?, true)))
    }

    pub(crate) fn upsert_append_copy_item(
        &self,
        clip_id: Option<String>,
        session_id: &str,
        text: String,
    ) -> Result<(ClipItem, usize, bool), String> {
        let conn = self.connect()?;
        let content_hash = hash_text(&format!("ipaste-append-copy:{session_id}:{text}"));
        let preview_text = preview(&text);
        let captured_at = now();

        if let Some(id) = clip_id.as_deref() {
            let active_exists = conn
                .query_row("SELECT id FROM clips WHERE id = ?1", params![id], |row| {
                    row.get::<_, String>(0)
                })
                .optional()
                .map_err(|error| error.to_string())?
                .is_some();

            if active_exists {
                conn.execute(
                    "UPDATE clips
                     SET clip_type = 'text',
                         content_hash = ?1,
                         preview_text = ?2,
                         text = ?3,
                         last_captured_at = ?4
                     WHERE id = ?5",
                    params![content_hash, preview_text, text, captured_at, id],
                )
                .map_err(|error| error.to_string())?;
                let clip = self.get_clip_with_conn(&conn, id)?;
                return Ok((clip, self.clip_total_count_with_conn(&conn)?, false));
            }
        }

        let clip = ClipItem {
            id: new_id(),
            clip_type: "text".to_string(),
            content_hash,
            display_name: Some("追加复制".to_string()),
            preview_text,
            text,
            source_app: None,
            last_captured_at: captured_at,
            favorite_count: 0,
            is_pinned: false,
        };

        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                clip.id,
                clip.clip_type,
                clip.content_hash,
                clip.display_name,
                clip.preview_text,
                clip.text,
                clip.source_app,
                clip.last_captured_at,
                clip.favorite_count,
                clip.is_pinned
            ],
        )
        .map_err(|error| error.to_string())?;

        Ok((clip, self.clip_total_count_with_conn(&conn)?, true))
    }

    pub(crate) fn prune_expired(&self) -> Result<(), String> {
        let conn = self.connect()?;
        let settings = self.settings_with_conn(&conn)?;
        self.prune_expired_with_conn(&conn, settings.retention_days)
    }

    pub(super) fn prune_expired_with_conn(
        &self,
        conn: &Connection,
        retention_days: i64,
    ) -> Result<(), String> {
        let cutoff = (Utc::now() - ChronoDuration::days(retention_days)).to_rfc3339();
        conn.execute(
            "DELETE FROM clips WHERE is_pinned = 0 AND datetime(last_captured_at) < datetime(?1)",
            params![cutoff],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn list_clips(
        &self,
        offset: usize,
        limit: usize,
        search: String,
    ) -> Result<ClipPage, String> {
        let conn = self.connect()?;
        self.list_clips_page_with_conn(&conn, offset, limit, &search)
    }

    pub(super) fn list_clips_page_with_conn(
        &self,
        conn: &Connection,
        offset: usize,
        limit: usize,
        search: &str,
    ) -> Result<ClipPage, String> {
        let limit = limit.clamp(1, 100);
        let query = search.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let total_count = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM clips
                 WHERE ?1 = ''
                    OR lower(COALESCE(display_name, '')) LIKE ?2
                    OR lower(preview_text) LIKE ?2
                    OR lower(clip_type) LIKE ?2
                    OR (clip_type != 'image' AND lower(text) LIKE ?2)
                    OR (clip_type = 'image' AND '图片 image' LIKE ?2)",
                params![query.as_str(), pattern.as_str()],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| error.to_string())? as usize;
        let all_count = self.clip_total_count_with_conn(conn)?;
        let mut stmt = conn
            .prepare(
                "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
                 FROM clips
                 WHERE ?3 = ''
                    OR lower(COALESCE(display_name, '')) LIKE ?4
                    OR lower(preview_text) LIKE ?4
                    OR lower(clip_type) LIKE ?4
                    OR (clip_type != 'image' AND lower(text) LIKE ?4)
                    OR (clip_type = 'image' AND '图片 image' LIKE ?4)
                 ORDER BY datetime(last_captured_at) DESC LIMIT ?1 OFFSET ?2",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map(
                params![
                    (limit + 1) as i64,
                    offset as i64,
                    query.as_str(),
                    pattern.as_str()
                ],
                map_clip,
            )
            .map_err(|error| error.to_string())?;

        let mut clips = collect_rows(rows)?;
        let has_more = clips.len() > limit;
        if has_more {
            clips.truncate(limit);
        }

        Ok(ClipPage {
            clips,
            has_more,
            total_count,
            all_count,
        })
    }

    pub(super) fn clip_total_count_with_conn(&self, conn: &Connection) -> Result<usize, String> {
        conn.query_row("SELECT COUNT(*) FROM clips", [], |row| row.get::<_, i64>(0))
            .map(|count| count as usize)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn count_clips_matching_with_conn(
        &self,
        conn: &Connection,
        search: &str,
    ) -> Result<usize, String> {
        let query = search.trim().to_lowercase();
        let pattern = format!("%{query}%");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*)
                 FROM clips
                 WHERE ?1 = ''
                    OR lower(COALESCE(display_name, '')) LIKE ?2
                    OR lower(preview_text) LIKE ?2
                    OR lower(clip_type) LIKE ?2
                    OR (clip_type != 'image' AND lower(text) LIKE ?2)
                    OR (clip_type = 'image' AND '图片 image' LIKE ?2)",
                params![query.as_str(), pattern.as_str()],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(count as usize)
    }

    /// 跨"历史/分类"搜索的统一入口：先查历史，历史无命中时回退到分类。
    pub(crate) fn search_with_fallback(
        &self,
        offset: usize,
        limit: usize,
        search: &str,
    ) -> Result<SearchResult, String> {
        let conn = self.connect()?;
        let total = self.count_clips_matching_with_conn(&conn, search)?;
        if total > 0 {
            let page = self.list_clips_page_with_conn(&conn, offset, limit, search)?;
            Ok(SearchResult::History { page })
        } else {
            let groups = self.search_all_category_items_with_conn(&conn, search)?;
            Ok(SearchResult::CategoryHits { groups })
        }
    }

    pub(crate) fn save_image_bytes(&self, content_hash: &str, bytes: &[u8]) -> Result<String, String> {
        let dir = self.image_dir()?;
        let filename = format!("{}.png", safe_filename(content_hash));
        let path = dir.join(filename);
        if !path.exists() {
            fs::write(&path, bytes).map_err(|error| error.to_string())?;
        }
        Ok(path.to_string_lossy().to_string())
    }

    fn image_dir(&self) -> Result<PathBuf, String> {
        let dir = self
            .db_path
            .parent()
            .ok_or_else(|| "无法定位应用数据目录".to_string())?
            .join(IMAGE_DIR);
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        Ok(dir)
    }

    pub(crate) fn delete_clip(&self, id: String) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM clips WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn clear_clips(&self) -> Result<usize, String> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM clips", [])
            .map_err(|error| error.to_string())
    }

    pub(crate) fn rename_clip(
        &self,
        id: String,
        collection: String,
        display_name: Option<String>,
    ) -> Result<ClipUpdate, String> {
        let display_name = clean_display_name(display_name)?;
        let conn = self.connect()?;
        match collection.as_str() {
            "history" => {
                conn.execute(
                    "UPDATE clips SET display_name = ?1 WHERE id = ?2",
                    params![display_name, id],
                )
                .map_err(|error| error.to_string())?;
                self.get_clip_with_conn(&conn, &id).map(ClipUpdate::Clip)
            }
            "category" => {
                conn.execute(
                    "UPDATE category_items SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
                    params![display_name, now(), id],
                )
                .map_err(|error| error.to_string())?;
                self.get_category_item_with_conn(&conn, &id)
                    .map(ClipUpdate::CategoryItem)
            }
            _ => Err("未知条目来源".to_string()),
        }
    }

    pub(crate) fn update_clip_content(
        &self,
        id: String,
        collection: String,
        text: String,
    ) -> Result<ClipUpdate, String> {
        let preview_text = preview(&text);
        let content_hash = hash_text(&text);
        match collection.as_str() {
            "history" => {
                let mut conn = self.connect()?;
                let tx = conn.transaction().map_err(|error| error.to_string())?;
                let current = self.get_clip_with_conn(&tx, &id)?;

                if let Some(existing) =
                    self.get_clip_by_content_hash_with_conn(&tx, &content_hash, Some(&id))?
                {
                    let existing_id = existing.id.clone();
                    let last_captured_at = now();
                    let display_name = existing.display_name.clone().or(current.display_name);
                    let is_pinned = existing.is_pinned || current.is_pinned;
                    let favorite_count = existing.favorite_count + current.favorite_count;
                    tx.execute(
                        "UPDATE clips
                         SET display_name = ?1, favorite_count = ?2, is_pinned = ?3, last_captured_at = ?4
                         WHERE id = ?5",
                        params![display_name, favorite_count, is_pinned, last_captured_at, existing_id],
                    )
                    .map_err(|error| error.to_string())?;
                    tx.execute(
                        "UPDATE category_items SET clip_snapshot_id = ?1 WHERE clip_snapshot_id = ?2",
                        params![existing_id, id],
                    )
                    .map_err(|error| error.to_string())?;
                    tx.execute("DELETE FROM clips WHERE id = ?1", params![id])
                        .map_err(|error| error.to_string())?;
                    let clip = self.get_clip_with_conn(&tx, &existing_id)?;
                    tx.commit().map_err(|error| error.to_string())?;
                    return Ok(ClipUpdate::Clip(clip));
                }

                tx.execute(
                    "UPDATE clips SET text = ?1, preview_text = ?2, content_hash = ?3 WHERE id = ?4",
                    params![text, preview_text, content_hash, id],
                )
                .map_err(|error| error.to_string())?;
                let clip = self.get_clip_with_conn(&tx, &id)?;
                tx.commit().map_err(|error| error.to_string())?;
                Ok(ClipUpdate::Clip(clip))
            }
            "category" => {
                let mut conn = self.connect()?;
                let tx = conn.transaction().map_err(|error| error.to_string())?;
                let current = self.get_category_item_with_conn(&tx, &id)?;

                if let Some(existing) = self.get_category_item_by_content_hash_with_conn(
                    &tx,
                    &current.category_id,
                    &content_hash,
                    Some(&id),
                )? {
                    self.record_tombstone_with_conn(&tx, "category_item", &id)?;
                    tx.execute("DELETE FROM category_items WHERE id = ?1", params![id])
                        .map_err(|error| error.to_string())?;
                    tx.commit().map_err(|error| error.to_string())?;
                    return Ok(ClipUpdate::CategoryItem(existing));
                }

                tx.execute(
                    "UPDATE category_items SET text = ?1, preview_text = ?2, content_hash = ?3, sync_state = 'local', updated_at = ?4 WHERE id = ?5",
                    params![text, preview_text, content_hash, now(), id],
                )
                .map_err(|error| error.to_string())?;
                let item = self.get_category_item_with_conn(&tx, &id)?;
                tx.commit().map_err(|error| error.to_string())?;
                Ok(ClipUpdate::CategoryItem(item))
            }
            _ => Err("未知条目来源".to_string()),
        }
    }

    pub(crate) fn set_clip_pinned(
        &self,
        id: String,
        collection: String,
        is_pinned: bool,
    ) -> Result<ClipUpdate, String> {
        let conn = self.connect()?;
        match collection.as_str() {
            "history" => {
                conn.execute(
                    "UPDATE clips SET is_pinned = ?1 WHERE id = ?2",
                    params![is_pinned, id],
                )
                .map_err(|error| error.to_string())?;
                self.get_clip_with_conn(&conn, &id).map(ClipUpdate::Clip)
            }
            "category" => {
                conn.execute(
                    "UPDATE category_items SET is_pinned = ?1, updated_at = ?2 WHERE id = ?3",
                    params![is_pinned, now(), id],
                )
                .map_err(|error| error.to_string())?;
                self.get_category_item_with_conn(&conn, &id)
                    .map(ClipUpdate::CategoryItem)
            }
            _ => Err("未知条目来源".to_string()),
        }
    }

    pub(crate) fn get_clip_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<ClipItem, String> {
        conn.query_row(
            "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
             FROM clips WHERE id = ?1",
            params![id],
            map_clip,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到剪贴板记录".to_string())
    }

    fn get_clip_by_content_hash_with_conn(
        &self,
        conn: &Connection,
        content_hash: &str,
        exclude_id: Option<&str>,
    ) -> Result<Option<ClipItem>, String> {
        conn.query_row(
            "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
             FROM clips
             WHERE content_hash = ?1 AND (?2 IS NULL OR id != ?2)",
            params![content_hash, exclude_id],
            map_clip,
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    /// 应用条目时刷新其最近捕获时间（原 commands.rs 中的裸 SQL，收编入 store 层）。
    pub(crate) fn touch_clip_captured(&self, id: &str) -> Result<(), String> {
        let conn = self.connect()?;
        self.touch_clip_captured_with_conn(&conn, id)
    }

    pub(crate) fn touch_clip_captured_with_conn(&self, conn: &Connection, id: &str) -> Result<(), String> {
        conn.execute(
            "UPDATE clips SET last_captured_at = ?1 WHERE id = ?2",
            params![now(), id],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::models::{CapturedClipboardItem, SearchResult};
    use crate::store::test_support::{seed_clip, seed_n_clips, temp_store};
    use crate::util::hash_text;
    use rusqlite::params;
    use std::time::Instant;

    #[test]
    fn touch_clip_captured_updates_timestamp() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "touch-me", "body");
        let id: String = conn
            .query_row("SELECT id FROM clips LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let before: String = conn
            .query_row("SELECT last_captured_at FROM clips WHERE id = ?1", params![&id], |row| row.get(0))
            .unwrap();
        // now() 为秒级精度（SecondsFormat::Secs），必须跨过秒边界才能观测变化。
        std::thread::sleep(std::time::Duration::from_millis(1100));
        store.touch_clip_captured_with_conn(&conn, &id).unwrap();
        let after: String = conn
            .query_row("SELECT last_captured_at FROM clips WHERE id = ?1", params![&id], |row| row.get(0))
            .unwrap();
        assert_ne!(before, after, "touch 应更新 last_captured_at");
    }

    #[test]
    fn count_clips_matching_respects_query() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello world", "hello world");
        seed_clip(&conn, "text", "rust lang", "rust lang");
        assert_eq!(store.count_clips_matching_with_conn(&conn, "hello").unwrap(), 1);
        assert_eq!(store.count_clips_matching_with_conn(&conn, "").unwrap(), 2);
        assert_eq!(store.count_clips_matching_with_conn(&conn, "nomatch").unwrap(), 0);
    }

    #[test]
    fn fallback_returns_history_when_history_has_hits() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello", "hello");
        seed_clip(&conn, "image", "图片预览", "/tmp/ipaste-image.png");
        let res = store.search_with_fallback(0, 20, "image").unwrap();
        match res {
            SearchResult::History { page } => assert_eq!(page.clips.len(), 1),
            other => panic!("expected History, got {:?}", other),
        }
    }

    #[test]
    fn fallback_returns_category_hits_when_history_empty() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat = crate::store::test_support::create_category(&conn, "A", "#f00", 0);
        crate::store::test_support::seed_category_item(&conn, &cat, "text", "secret token", "secret token");
        let res = store.search_with_fallback(0, 20, "secret").unwrap();
        match res {
            SearchResult::CategoryHits { groups } => {
                assert_eq!(groups.len(), 1);
                assert_eq!(groups[0].items.len(), 1);
            }
            other => panic!("expected CategoryHits, got {:?}", other),
        }
    }

    #[test]
    fn fallback_returns_empty_category_hits_when_nowhere_matches() {
        let store = temp_store();
        let _conn = store.connect();
        let res = store.search_with_fallback(0, 20, "ghost").unwrap();
        match res {
            SearchResult::CategoryHits { groups } => assert!(groups.is_empty()),
            other => panic!("expected CategoryHits, got {:?}", other),
        }
    }

    #[test]
    fn insert_captured_item_merges_on_duplicate_content_hash() {
        let store = temp_store();
        let item = CapturedClipboardItem {
            clip_type: "text".to_string(),
            content_hash: hash_text("hello"),
            preview_text: "hello".to_string(),
            text: "hello".to_string(),
            image_bytes: None,
            display_name: None,
        };
        let first = store.insert_captured_item(item.clone()).unwrap().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let second = store.insert_captured_item(item).unwrap().unwrap();

        assert!(first.2, "first insert should report was_inserted=true");
        assert!(!second.2, "duplicate should report was_inserted=false");
        assert_eq!(first.1, 1);
        assert_eq!(second.1, 1);
        assert!(second.0.last_captured_at >= first.0.last_captured_at);

        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips WHERE content_hash = ?1",
                rusqlite::params![hash_text("hello")],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn list_clips_first_page_1k_under_250ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_clips(&conn, 1000);
        let start = Instant::now();
        let _ = store.list_clips(0, 20, "".to_string()).unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 250, "list_clips 1k took {elapsed:?}");
    }

    #[test]
    fn list_clips_first_page_5k_under_750ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_clips(&conn, 5000);
        let start = Instant::now();
        let _ = store.list_clips(0, 20, "".to_string()).unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 750, "list_clips 5k took {elapsed:?}");
    }

    #[test]
    fn search_with_fallback_5k_under_1000ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_clips(&conn, 5000);
        let start = Instant::now();
        let _ = store.search_with_fallback(0, 20, "hello").unwrap();
        let elapsed = start.elapsed();
        assert!(elapsed.as_millis() < 1000, "search 5k took {elapsed:?}");
    }
}
