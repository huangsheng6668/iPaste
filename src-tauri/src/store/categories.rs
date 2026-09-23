// store/categories.rs — 分类+分类项 CRUD/排序
mod received;

#[cfg(test)]
mod tests;

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use super::Store;
use crate::models::{Category, CategoryHitGroup, CategoryItem, CategoryWithItem, ClipItem};
use crate::{
    store::rows::{collect_rows, map_category, map_category_item, map_clip},
    util::{clean_category_name, clean_color, new_id, now},
};

fn ensure_unique_ids(ids: &[String]) -> Result<(), String> {
    let mut seen = HashSet::new();
    for id in ids {
        let id = id.trim();
        if id.is_empty() {
            return Err("排序 ID 不能为空".to_string());
        }
        if !seen.insert(id.to_string()) {
            return Err("排序列表包含重复条目".to_string());
        }
    }
    Ok(())
}

fn ensure_category_exists(conn: &Connection, category_id: &str) -> Result<(), String> {
    conn.query_row(
        "SELECT id FROM categories WHERE id = ?1",
        params![category_id],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .map_err(|error| error.to_string())?
    .map(|_| ())
    .ok_or_else(|| "未找到分类".to_string())
}

pub(crate) fn ensure_all_categories_exist(conn: &Connection, category_ids: &[String]) -> Result<(), String> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM categories", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if count as usize != category_ids.len() {
        return Err("分类顺序需要包含全部分类".to_string());
    }

    for id in category_ids {
        ensure_category_exists(conn, id)?;
    }
    Ok(())
}

pub(crate) fn ensure_all_category_items_exist(
    conn: &Connection,
    category_id: &str,
    item_ids: &[String],
) -> Result<(), String> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM category_items WHERE category_id = ?1",
            params![category_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if count as usize != item_ids.len() {
        return Err("条目顺序需要包含该分类下的全部条目".to_string());
    }

    for id in item_ids {
        conn.query_row(
            "SELECT id FROM category_items WHERE id = ?1 AND category_id = ?2",
            params![id, category_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "排序列表包含不属于该分类的条目".to_string())?;
    }
    Ok(())
}

impl Store {
    pub(crate) fn list_categories(&self) -> Result<Vec<Category>, String> {
        let conn = self.connect()?;
        self.list_categories_with_conn(&conn)
    }

    pub(super) fn list_categories_with_conn(&self, conn: &Connection) -> Result<Vec<Category>, String> {
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

    pub(super) fn list_category_items_with_conn(
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

    /// 查单个分组下的全部条目，排序与 `list_category_items_with_conn` 一致
    /// （供 LAN 整组发送按 UI 顺序逐个推帧）。
    pub(crate) fn list_category_items_for_category_with_conn(
        &self,
        conn: &Connection,
        category_id: &str,
    ) -> Result<Vec<CategoryItem>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items WHERE category_id = ?1
                 ORDER BY is_pinned DESC, sort_order ASC, datetime(created_at) DESC",
            )
            .map_err(|error| error.to_string())?;

        let rows = stmt
            .query_map(params![category_id], map_category_item)
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

    pub(crate) fn get_category_item_with_conn(
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

    pub(super) fn get_category_item_by_content_hash_with_conn(
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

    /// 查某分组（按名称）当前条目的最小 sort_order；分组不存在或为空时返回 0。
    /// 供 LAN 接收端在 `CategoryBatchStart` 时预排 sort_order 用。
    pub(crate) fn category_min_sort_order_with_conn(
        &self,
        conn: &Connection,
        category_name: &str,
    ) -> Result<i64, String> {
        conn.query_row(
            "SELECT COALESCE(MIN(ci.sort_order), 0)
             FROM categories c
             LEFT JOIN category_items ci ON ci.category_id = c.id
             WHERE c.name = ?1",
            params![category_name],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn category_min_sort_order(&self, category_name: &str) -> Result<i64, String> {
        let conn = self.connect()?;
        self.category_min_sort_order_with_conn(&conn, category_name)
    }

    /// 按 id 查单个分组。供 lan_send_clip 解析分组名/颜色用。
    pub(crate) fn get_category_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<Category, String> {
        conn.query_row(
            "SELECT id, name, color, sort_order, created_at, updated_at FROM categories WHERE id = ?1",
            params![id],
            map_category,
        )
        .optional()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "未找到分类".to_string())
    }

    /// 查询某条历史 clip 当前所属的分类（按内容 hash 关联 `category_items`）。
    ///
    /// 供 LAN 同步发送历史条目（`ClipSource::Item`）时携带分类信息：同一条目可能
    /// 加入多个分类，取最近更新的那个。无关联分类返回 `Ok(None)`。
    ///
    /// 注意必须按 `content_hash` 而非 `clip_snapshot_id` 关联：历史数据里存在
    /// 大量孤儿 snapshot id（旧版 `insert_received_category_item` 伪造 id 的遗留），
    /// 按 id 关联会把「实际已入分类」的条目误判为未入分类，导致发送时丢失分类。
    pub(crate) fn get_category_for_clip_with_conn(
        &self,
        conn: &Connection,
        content_hash: &str,
    ) -> Result<Option<Category>, String> {
        conn.query_row(
            "SELECT c.id, c.name, c.color, c.sort_order, c.created_at, c.updated_at
             FROM categories c
             JOIN category_items ci ON ci.category_id = c.id
             WHERE ci.content_hash = ?1
             ORDER BY ci.updated_at DESC
             LIMIT 1",
            params![content_hash],
            map_category,
        )
        .optional()
        .map_err(|error| error.to_string())
    }
}
