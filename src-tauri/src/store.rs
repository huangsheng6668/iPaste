use std::{fs, path::PathBuf};

use rusqlite::Connection;

mod migrations;
mod settings;
mod clips;
mod categories;
mod sync;

#[derive(Clone)]
pub struct Store {
    pub db_path: PathBuf,
}

impl Store {
    pub(crate) fn new(db_path: PathBuf) -> Result<Self, String> {
        let is_first_launch = !db_path.exists();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let store = Self { db_path };
        let conn = store.connect()?;
        store.migrate(&conn)?;
        if is_first_launch {
            store.seed_default_clips(&conn)?;
        }
        Ok(store)
    }

    pub(crate) fn connect(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|error| error.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| error.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SearchResult;

    fn temp_store() -> Store {
        let dir = std::env::temp_dir().join(format!(
            "ipaste-test-{}-{}",
            std::process::id(),
            uuid_like()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("test.db");
        let store = Store::new(db_path).expect("store init");
        // Store::new seeds DEFAULT_CLIPBOARD_SEEDS on first launch; clear all
        // business tables so subsequent tests see a clean database.
        let conn = store.connect().expect("connect for cleanup");
        conn.execute("DELETE FROM category_items", [])
            .expect("clear category_items");
        conn.execute("DELETE FROM categories", [])
            .expect("clear categories");
        conn.execute("DELETE FROM clips", []).expect("clear clips");
        store
    }

    // 简易唯一串，避免引入 uuid 依赖；若项目已有 new_id() 可直接复用
    fn uuid_like() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{nanos}")
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

    fn seed_clip(conn: &rusqlite::Connection, clip_type: &str, preview: &str, text: &str) {
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, 'test', ?6, 0, 0)",
            rusqlite::params![
                crate::new_id(),
                clip_type,
                crate::util::hash_text(text),
                preview,
                text,
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .unwrap();
    }

    #[test]
    fn store_initializes_empty() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "temp_store() should yield a clean database with no seeded clips"
        );
    }

    fn create_category(
        conn: &rusqlite::Connection,
        name: &str,
        color: &str,
        sort_order: i64,
    ) -> String {
        let id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO categories (id, name, color, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            rusqlite::params![id, name, color, sort_order, now],
        )
        .unwrap();
        id
    }

    fn seed_category_item(
        conn: &rusqlite::Connection,
        category_id: &str,
        clip_type: &str,
        preview: &str,
        text: &str,
    ) {
        let id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, 0, ?8, ?8, 'local', 0)",
            rusqlite::params![
                id,
                category_id,
                id,
                clip_type,
                crate::util::hash_text(text),
                preview,
                text,
                now
            ],
        )
        .unwrap();
    }

    #[test]
    fn search_all_category_items_groups_by_category() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat_a = create_category(&conn, "A", "#f00", 0);
        let cat_b = create_category(&conn, "B", "#0f0", 1);
        seed_category_item(&conn, &cat_a, "text", "alpha token", "alpha token");
        seed_category_item(&conn, &cat_a, "text", "beta token", "beta token");
        seed_category_item(&conn, &cat_b, "text", "alpha other", "alpha other");

        let groups = store
            .search_all_category_items_with_conn(&conn, "alpha")
            .unwrap();
        assert_eq!(groups.len(), 2, "two categories have alpha hits");
        assert_eq!(groups[0].category.name, "A", "lower sort_order first");
        assert_eq!(groups[0].items.len(), 1);
        assert_eq!(groups[1].category.name, "B");
        assert_eq!(groups[1].items.len(), 1);
    }

    #[test]
    fn search_all_category_items_empty_query_returns_empty() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat = create_category(&conn, "A", "#f00", 0);
        seed_category_item(&conn, &cat, "text", "x", "x");
        let groups = store
            .search_all_category_items_with_conn(&conn, "")
            .unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn fallback_returns_history_when_history_has_hits() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "hello", "hello");
        // Task 3 deferred minor: cover the `clip_type = 'image'` branch of the
        // history search SQL. Searching "image" must match this clip via the
        // `'图片 image' LIKE ?` clause even though `text` is just a file path.
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
        let cat = create_category(&conn, "A", "#f00", 0);
        seed_category_item(&conn, &cat, "text", "secret token", "secret token");
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
}
