use std::{fs, path::PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::models::*;
use crate::util::*;

// 自由函数与常量仍定义在 crate root（lib.rs），此处按需通过 `crate::` 引用。
use crate::{
    clean_color, cloud_post, collect_rows, ensure_all_categories_exist,
    ensure_all_category_items_exist, ensure_category_exists, ensure_unique_ids,
    is_syncable_clip_type, map_category, map_category_item, map_clip,
    new_id, test_cloud_connection,
};

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

    pub(crate) fn update_cloud_settings(
        &self,
        api_address: String,
        api_key: String,
    ) -> Result<AppSettings, String> {
        let api_address = clean_api_address(api_address)?;
        let api_key = clean_api_key(api_key)?;
        test_cloud_connection(&api_address, &api_key)?;

        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('cloud_api_address', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![api_address],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('cloud_api_key', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![api_key],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('cloud_last_connected_at', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![now()],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn disable_cloud_sync(&self) -> Result<AppSettings, String> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM settings WHERE key IN ('cloud_api_address', 'cloud_api_key', 'cloud_last_connected_at')",
            [],
        )
        .map_err(|error| error.to_string())?;
        self.settings_with_conn(&conn)
    }

    pub(crate) fn sync_cloud(&self) -> Result<(), String> {
        let conn = self.connect()?;
        let cloud = self.cloud_settings_with_conn(&conn)?;
        if !cloud.enabled {
            return Ok(());
        }

        let categories = self.list_categories_with_conn(&conn)?;
        let category_items = self.list_syncable_category_items_with_conn(&conn)?;
        let tombstones = self.list_tombstones_with_conn(&conn)?;
        drop(conn);

        let payload = CloudPushPayload {
            categories,
            category_items,
            deleted_category_ids: tombstones
                .iter()
                .filter(|item| item.entity == "category")
                .map(|item| item.entity_id.clone())
                .collect(),
            deleted_category_item_ids: tombstones
                .iter()
                .filter(|item| item.entity == "category_item")
                .map(|item| item.entity_id.clone())
                .collect(),
        };
        let snapshot: CloudSnapshot = cloud_post(
            &cloud.api_address,
            &cloud.api_key,
            "/api/sync/push",
            &payload,
        )?;

        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        self.merge_cloud_snapshot_with_conn(&tx, snapshot)?;
        self.clear_tombstones_with_conn(&tx)?;
        tx.execute(
            "INSERT INTO settings (key, value) VALUES ('cloud_last_connected_at', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![now()],
        )
        .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn list_categories(&self) -> Result<Vec<Category>, String> {
        let conn = self.connect()?;
        self.list_categories_with_conn(&conn)
    }

    fn list_categories_with_conn(&self, conn: &Connection) -> Result<Vec<Category>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, color, sort_order, created_at, updated_at
                 FROM categories ORDER BY sort_order ASC, datetime(created_at) ASC",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map([], map_category)
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    pub(crate) fn list_category_items(&self) -> Result<Vec<CategoryItem>, String> {
        let conn = self.connect()?;
        self.list_category_items_with_conn(&conn)
    }

    fn list_category_items_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<Vec<CategoryItem>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items ORDER BY is_pinned DESC, sort_order ASC, datetime(created_at) DESC",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map([], map_category_item)
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    /// 跨分类搜索 `category_items`，按命中条目的分类分组返回。
    ///
    /// 组间顺序由分类 `sort_order`（见 `list_categories_with_conn`）决定；
    /// 组内条目顺序由 SQL `ORDER BY is_pinned DESC, sort_order ASC,
    /// datetime(created_at) DESC` 决定（与 `list_category_items_with_conn` 一致）。
    /// 仅返回有命中的分类。
    pub(crate) fn search_all_category_items_with_conn(
        &self,
        conn: &Connection,
        search: &str,
    ) -> Result<Vec<CategoryHitGroup>, String> {
        let query = search.trim().to_lowercase();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let pattern = format!("%{query}%");
        let mut stmt = conn
            .prepare(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items
                 WHERE lower(COALESCE(display_name, '')) LIKE ?1
                    OR lower(preview_text) LIKE ?1
                    OR lower(clip_type) LIKE ?1
                    OR (clip_type != 'image' AND lower(text) LIKE ?1)
                    OR (clip_type = 'image' AND '图片 image' LIKE ?1)
                 ORDER BY is_pinned DESC, sort_order ASC, datetime(created_at) DESC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![pattern.as_str()], map_category_item)
            .map_err(|error| error.to_string())?;
        let items: Vec<CategoryItem> = collect_rows(rows)?;

        // 按分类 sort_order 分组：先用 list_categories_with_conn 拿到有序分类，
        // 再按分类 id 收集命中条目，最后按分类顺序输出（保证组按 sort_order 升序）。
        let ordered_categories = self.list_categories_with_conn(conn)?;
        let mut groups: std::collections::HashMap<String, Vec<CategoryItem>> =
            std::collections::HashMap::new();
        for item in items {
            groups.entry(item.category_id.clone()).or_default().push(item);
        }
        let result: Vec<CategoryHitGroup> = ordered_categories
            .into_iter()
            .filter_map(|category| {
                groups
                    .remove(&category.id)
                    .map(|items| CategoryHitGroup { category, items })
            })
            .collect();
        Ok(result)
    }

    pub(crate) fn reorder_categories(
        &self,
        category_ids: Vec<String>,
    ) -> Result<Vec<Category>, String> {
        if category_ids.is_empty() {
            return Err("请提供分类顺序".to_string());
        }

        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        ensure_unique_ids(&category_ids)?;
        ensure_all_categories_exist(&tx, &category_ids)?;

        let updated_at = now();
        for (index, id) in category_ids.iter().enumerate() {
            tx.execute(
                "UPDATE categories SET sort_order = ?1, updated_at = ?2 WHERE id = ?3",
                params![index as i64, updated_at, id],
            )
            .map_err(|error| error.to_string())?;
        }

        let categories = self.list_categories_with_conn(&tx)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(categories)
    }

    pub(crate) fn reorder_category_items(
        &self,
        category_id: String,
        item_ids: Vec<String>,
    ) -> Result<Vec<CategoryItem>, String> {
        if category_id.trim().is_empty() {
            return Err("请选择分类".to_string());
        }

        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        ensure_category_exists(&tx, &category_id)?;
        ensure_unique_ids(&item_ids)?;
        ensure_all_category_items_exist(&tx, &category_id, &item_ids)?;

        let updated_at = now();
        for (index, id) in item_ids.iter().enumerate() {
            tx.execute(
                "UPDATE category_items SET sort_order = ?1, sync_state = 'local', updated_at = ?2 WHERE id = ?3 AND category_id = ?4",
                params![index as i64, updated_at, id, category_id],
            )
            .map_err(|error| error.to_string())?;
        }

        let items = self.list_category_items_with_conn(&tx)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(items)
    }

    #[allow(dead_code)]
    fn list_syncable_category_items_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<Vec<CategoryItem>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items
                 WHERE clip_type IN ('text', 'link', 'color', 'html')
                 ORDER BY sort_order ASC, datetime(created_at) DESC",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map([], map_category_item)
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    pub(crate) fn create_category(
        &self,
        name: String,
        color: String,
    ) -> Result<Category, String> {
        let name = clean_category_name(name)?;
        let color = clean_color(color);
        let conn = self.connect()?;
        let sort_order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM categories",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let now = now();
        let category = Category {
            id: new_id(),
            name,
            color,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
        };

        conn.execute(
            "INSERT INTO categories (id, name, color, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                category.id,
                category.name,
                category.color,
                category.sort_order,
                category.created_at,
                category.updated_at
            ],
        )
        .map_err(|error| error.to_string())?;

        Ok(category)
    }

    pub(crate) fn create_category_with_clip(
        &self,
        name: String,
        color: String,
        clip_id: String,
    ) -> Result<CategoryWithItem, String> {
        let name = clean_category_name(name)?;
        let color = clean_color(color);
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let sort_order: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM categories",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let now = now();
        let category = Category {
            id: new_id(),
            name,
            color,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
        };

        tx.execute(
            "INSERT INTO categories (id, name, color, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                category.id,
                category.name,
                category.color,
                category.sort_order,
                category.created_at,
                category.updated_at
            ],
        )
        .map_err(|error| error.to_string())?;

        let item = self.add_clip_to_category_with_conn(&tx, &clip_id, &category.id)?;
        tx.commit().map_err(|error| error.to_string())?;

        Ok(CategoryWithItem { category, item })
    }

    pub(crate) fn update_category(
        &self,
        id: String,
        name: String,
        color: String,
    ) -> Result<Category, String> {
        let name = clean_category_name(name)?;
        let color = clean_color(color);
        let updated_at = now();
        let conn = self.connect()?;

        conn.execute(
            "UPDATE categories SET name = ?1, color = ?2, updated_at = ?3 WHERE id = ?4",
            params![name, color, updated_at, id],
        )
        .map_err(|error| error.to_string())?;

        conn.query_row(
            "SELECT id, name, color, sort_order, created_at, updated_at FROM categories WHERE id = ?1",
            params![id],
            map_category,
        )
        .optional()
        .map_err(|error| error.to_string())?
            .ok_or_else(|| "未找到分类".to_string())
    }

    pub(crate) fn delete_category(&self, id: String) -> Result<(), String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        self.record_tombstone_with_conn(&tx, "category", &id)?;
        tx.execute("DELETE FROM categories WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub(crate) fn add_clip_to_category(
        &self,
        clip_id: String,
        category_id: String,
    ) -> Result<CategoryItem, String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        let item = self.add_clip_to_category_with_conn(&tx, &clip_id, &category_id)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(item)
    }

    fn add_clip_to_category_with_conn(
        &self,
        conn: &Connection,
        clip_id: &str,
        category_id: &str,
    ) -> Result<CategoryItem, String> {
        let clip: ClipItem = conn
            .query_row(
                "SELECT id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned
                 FROM clips WHERE id = ?1",
                params![clip_id],
                map_clip,
            )
            .optional()
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "未找到剪贴板记录".to_string())?;

        conn.query_row(
            "SELECT id FROM categories WHERE id = ?1",
            params![category_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到分类".to_string())?;

        if let Some(existing) = conn
            .query_row(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items WHERE category_id = ?1 AND content_hash = ?2",
                params![category_id, clip.content_hash],
                map_category_item,
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            return Ok(existing);
        }

        let sort_order: i64 = conn
            .query_row(
                "SELECT COALESCE(MIN(sort_order), 0) - 1 FROM category_items WHERE category_id = ?1",
                params![category_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let now = now();
        let item = CategoryItem {
            id: new_id(),
            category_id: category_id.to_string(),
            clip_snapshot_id: clip.id,
            clip_type: clip.clip_type,
            content_hash: clip.content_hash,
            display_name: clip.display_name,
            preview_text: clip.preview_text,
            text: clip.text,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
            sync_state: "local".to_string(),
            is_pinned: false,
        };

        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                item.id,
                item.category_id,
                item.clip_snapshot_id,
                item.clip_type,
                item.content_hash,
                item.display_name,
                item.preview_text,
                item.text,
                item.sort_order,
                item.created_at,
                item.updated_at,
                item.sync_state,
                item.is_pinned
            ],
        )
        .map_err(|error| error.to_string())?;

        conn.execute(
            "UPDATE clips SET favorite_count = favorite_count + 1 WHERE id = ?1",
            params![item.clip_snapshot_id],
        )
        .map_err(|error| error.to_string())?;

        Ok(item)
    }

    pub(crate) fn remove_category_item(&self, id: String) -> Result<(), String> {
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;
        self.record_tombstone_with_conn(&tx, "category_item", &id)?;
        tx.execute("DELETE FROM category_items WHERE id = ?1", params![id])
            .map_err(|error| error.to_string())?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    fn get_category_item_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<CategoryItem, String> {
        conn.query_row(
            "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
             FROM category_items WHERE id = ?1",
            params![id],
            map_category_item,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到分类条目".to_string())
    }

    fn get_category_item_by_content_hash_with_conn(
        &self,
        conn: &Connection,
        category_id: &str,
        content_hash: &str,
        exclude_id: Option<&str>,
    ) -> Result<Option<CategoryItem>, String> {
        conn.query_row(
            "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
             FROM category_items
             WHERE category_id = ?1 AND content_hash = ?2 AND (?3 IS NULL OR id != ?3)",
            params![category_id, content_hash, exclude_id],
            map_category_item,
        )
        .optional()
        .map_err(|error| error.to_string())
    }

    fn list_tombstones_with_conn(&self, conn: &Connection) -> Result<Vec<Tombstone>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT entity, entity_id FROM sync_tombstones ORDER BY datetime(created_at) ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Tombstone {
                    entity: row.get(0)?,
                    entity_id: row.get(1)?,
                })
            })
            .map_err(|error| error.to_string())?;
        collect_rows(rows)
    }

    fn record_tombstone_with_conn(
        &self,
        conn: &Connection,
        entity: &str,
        entity_id: &str,
    ) -> Result<(), String> {
        conn.execute(
            "INSERT INTO sync_tombstones (entity, entity_id, created_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(entity, entity_id) DO UPDATE SET created_at = excluded.created_at",
            params![entity, entity_id, now()],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn clear_tombstones_with_conn(&self, conn: &Connection) -> Result<(), String> {
        conn.execute("DELETE FROM sync_tombstones", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn merge_cloud_snapshot_with_conn(
        &self,
        conn: &Connection,
        snapshot: CloudSnapshot,
    ) -> Result<(), String> {
        for id in snapshot.deleted_category_ids {
            conn.execute("DELETE FROM categories WHERE id = ?1", params![id])
                .map_err(|error| error.to_string())?;
        }

        for id in snapshot.deleted_category_item_ids {
            conn.execute("DELETE FROM category_items WHERE id = ?1", params![id])
                .map_err(|error| error.to_string())?;
        }

        for category in snapshot.categories {
            conn.execute(
                "INSERT INTO categories (id, name, color, sort_order, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                   name = excluded.name,
                   color = excluded.color,
                   sort_order = excluded.sort_order,
                   updated_at = excluded.updated_at
                 WHERE datetime(excluded.updated_at) >= datetime(categories.updated_at)",
                params![
                    category.id,
                    category.name,
                    category.color,
                    category.sort_order,
                    category.created_at,
                    category.updated_at,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        for item in snapshot.category_items {
            if !is_syncable_clip_type(&item.clip_type) {
                continue;
            }

            conn.execute(
                "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'synced', ?12)
                 ON CONFLICT(id) DO UPDATE SET
                   category_id = excluded.category_id,
                   clip_snapshot_id = excluded.clip_snapshot_id,
                   clip_type = excluded.clip_type,
                   content_hash = excluded.content_hash,
                   display_name = excluded.display_name,
                   preview_text = excluded.preview_text,
                   text = excluded.text,
                   sort_order = excluded.sort_order,
                   updated_at = excluded.updated_at,
                   sync_state = 'synced',
                   is_pinned = excluded.is_pinned
                 WHERE datetime(excluded.updated_at) >= datetime(category_items.updated_at)",
                params![
                    item.id,
                    item.category_id,
                    item.clip_snapshot_id,
                    item.clip_type,
                    item.content_hash,
                    item.display_name,
                    item.preview_text,
                    item.text,
                    item.sort_order,
                    item.created_at,
                    item.updated_at,
                    item.is_pinned,
                ],
            )
            .map_err(|error| error.to_string())?;
        }

        conn.execute(
            "UPDATE category_items
             SET sync_state = 'synced'
             WHERE clip_type IN ('text', 'link', 'color', 'html')",
            [],
        )
        .map_err(|error| error.to_string())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
