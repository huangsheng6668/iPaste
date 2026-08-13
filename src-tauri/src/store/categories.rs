// store/categories.rs — 分类+分类项 CRUD/排序
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::*;
use crate::util::*;
use super::Store;
use crate::{
    clean_color, collect_rows, ensure_all_categories_exist, ensure_all_category_items_exist,
    ensure_category_exists, ensure_unique_ids, map_category, map_category_item, map_clip, new_id,
    now,
};

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

    /// LAN 同步接收侧：把收到的条目落到「按名称匹配的同名分组」下。
    ///
    /// 为保持与本地「加入分组」流程（`add_clip_to_category_with_conn`）及 clips 合并逻辑
    /// 一致，会先确保存在一条对应内容的 `clips` 行：若历史表已有同 content_hash 则复用其
    /// id，否则插入一条占位 clips 行；`clip_snapshot_id` 始终指向真实存在的 clips 记录。
    ///
    /// 幂等：同一分组下相同 content_hash 的条目不重复创建。
    /// color 仅在新建分组时采用；已有同名分组保持其原色。
    pub(crate) fn insert_received_category_item(
        &self,
        clip_type: String,
        content_hash: String,
        preview_text: String,
        text: String,
        category_name: String,
        category_color: Option<String>,
    ) -> Result<CategoryItem, String> {
        let category_name = clean_category_name(category_name)?;
        let mut conn = self.connect()?;
        let tx = conn.transaction().map_err(|error| error.to_string())?;

        // 1. 按名称查分组；不存在则新建（color 用传入值或默认灰）。
        let category: Category = match tx
            .query_row(
                "SELECT id, name, color, sort_order, created_at, updated_at FROM categories WHERE name = ?1",
                params![category_name],
                map_category,
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            Some(cat) => cat,
            None => {
                let sort_order: i64 = tx
                    .query_row(
                        "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM categories",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;
                let now = now();
                let color = category_color
                    .map(|c| clean_color(c))
                    .unwrap_or_else(|| "#9CA3AF".to_string());
                let cat = Category {
                    id: new_id(),
                    name: category_name.clone(),
                    color,
                    sort_order,
                    created_at: now.clone(),
                    updated_at: now,
                };
                tx.execute(
                    "INSERT INTO categories (id, name, color, sort_order, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        cat.id,
                        cat.name,
                        cat.color,
                        cat.sort_order,
                        cat.created_at,
                        cat.updated_at
                    ],
                )
                .map_err(|error| error.to_string())?;
                cat
            }
        };

        // 2. 幂等：同分组同 content_hash 已存在则直接返回。
        if let Some(existing) = tx
            .query_row(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items WHERE category_id = ?1 AND content_hash = ?2",
                params![category.id, content_hash],
                map_category_item,
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            tx.commit().map_err(|error| error.to_string())?;
            return Ok(existing);
        }

        // 3. 为该内容找到/创建对应的 clips 行，作为 clip_snapshot_id 的引用。
        //    clips.content_hash 是 UNIQUE 的：若历史表已有同内容（比如用户先复制过、
        //    又从分组同步过来），复用已有 clips.id；否则插入一条占位 clips 行。
        //    这样 clip_snapshot_id 始终指向真实存在的 clips 记录，与本地「加入分组」
        //    流程（add_clip_to_category_with_conn）以及 clips 合并逻辑保持一致，
        //    避免「孤立」snapshot id 在未来引发查询/合并问题。
        let now = now();
        let clip_id: String = match tx
            .query_row(
                "SELECT id FROM clips WHERE content_hash = ?1",
                params![content_hash],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            Some(existing_id) => existing_id,
            None => {
                // 不存在则插入占位 clips 行（ON CONFLICT 兜底并发/重复场景）。
                let new_clip_id = new_id();
                tx.execute(
                    "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5, NULL, ?6, 0, 0)
                     ON CONFLICT(content_hash) DO NOTHING",
                    params![new_clip_id, clip_type, content_hash, preview_text, text, now],
                )
                .map_err(|error| error.to_string())?;
                // 再查一次：并发或已存在时拿到真实 id（new_clip_id 可能因 ON CONFLICT 未写入）。
                // INSERT 已成功的情况下此行必然存在，失败必须向上传播而非伪造 id。
                tx.query_row(
                    "SELECT id FROM clips WHERE content_hash = ?1",
                    params![content_hash],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?
            }
        };

        // 4. 插入 category_items。sort_order 取当前最小值 - 1，新条目排在分组顶部。
        let sort_order: i64 = tx
            .query_row(
                "SELECT COALESCE(MIN(sort_order), 0) - 1 FROM category_items WHERE category_id = ?1",
                params![category.id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        let item = CategoryItem {
            id: new_id(),
            category_id: category.id.clone(),
            clip_snapshot_id: clip_id,
            clip_type,
            content_hash,
            display_name: None,
            preview_text,
            text,
            sort_order,
            created_at: now.clone(),
            updated_at: now,
            sync_state: "local".to_string(),
            is_pinned: false,
        };

        tx.execute(
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

        tx.commit().map_err(|error| error.to_string())?;
        Ok(item)
    }
}

#[cfg(test)]
mod tests {
    use crate::store::test_support::{create_category, seed_category_item, seed_clip, temp_store};

    #[test]
    fn search_all_category_items_groups_by_category() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat_a = create_category(&conn, "A", "#f00", 0);
        let cat_b = create_category(&conn, "B", "#0f0", 1);
        seed_category_item(&conn, &cat_a, "text", "alpha token", "alpha token");
        seed_category_item(&conn, &cat_a, "text", "beta token", "beta token");
        seed_category_item(&conn, &cat_b, "text", "alpha other", "alpha other");

        let groups = store.search_all_category_items_with_conn(&conn, "alpha").unwrap();
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
        let groups = store.search_all_category_items_with_conn(&conn, "").unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn reorder_categories_persists_sort_order() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let a = create_category(&conn, "A", "#f00", 0);
        let b = create_category(&conn, "B", "#0f0", 1);
        let c = create_category(&conn, "C", "#00f", 2);

        let reordered = store.reorder_categories(vec![c.clone(), b.clone(), a.clone()]).unwrap();
        assert_eq!(reordered.len(), 3);
        assert_eq!(reordered[0].id, c);
        assert_eq!(reordered[1].id, b);
        assert_eq!(reordered[2].id, a);
        assert_eq!(reordered[0].sort_order, 0);
        assert_eq!(reordered[1].sort_order, 1);
        assert_eq!(reordered[2].sort_order, 2);
    }

    #[test]
    fn delete_category_records_tombstone() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let cat_id = create_category(&conn, "A", "#f00", 0);

        store.delete_category(cat_id.clone()).unwrap();

        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE id = ?1",
                rusqlite::params![cat_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(exists, 0);

        let tomb: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_tombstones WHERE entity = 'category' AND entity_id = ?1",
                rusqlite::params![cat_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tomb, 1, "delete_category should record a tombstone");
    }

    /// LAN 同步接收：分组不存在时自动创建，并落到该分组下。
    #[test]
    fn insert_received_category_item_creates_category_and_item() {
        let store = temp_store();
        let text = "hello-sync";
        let hash = crate::util::hash_text(text);
        let item = store
            .insert_received_category_item(
                "text".to_string(),
                hash.clone(),
                text.to_string(),
                text.to_string(),
                "工作".to_string(),
                Some("#0D9488".to_string()),
            )
            .unwrap();

        let conn = store.connect().unwrap();
        // 分组被创建，颜色采用传入值
        let cat_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = '工作' AND color = '#0D9488'",
                rusqlite::params![],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cat_count, 1);
        // 条目落到该分组下
        let item_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM category_items WHERE category_id = ?1 AND content_hash = ?2",
                rusqlite::params![item.category_id, hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(item_count, 1);
    }

    /// 同名分组已存在时复用，不重复创建；颜色保持原色（不被传入值覆盖）。
    #[test]
    fn insert_received_category_item_reuses_existing_category() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let existing_id = create_category(&conn, "工作", "#ff0000", 0);

        let text = "hello-sync";
        let item = store
            .insert_received_category_item(
                "text".to_string(),
                crate::util::hash_text(text),
                text.to_string(),
                text.to_string(),
                "工作".to_string(),
                Some("#0D9488".to_string()),
            )
            .unwrap();

        assert_eq!(item.category_id, existing_id, "should reuse existing category");
        // 仍是单分组，且颜色不变
        let cat_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM categories WHERE name = '工作'",
                rusqlite::params![],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cat_count, 1);
        let color: String = conn
            .query_row(
                "SELECT color FROM categories WHERE id = ?1",
                rusqlite::params![existing_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(color, "#ff0000", "existing category color must be preserved");
    }

    /// 同分组同内容幂等：重复同步不产生副本。
    #[test]
    fn insert_received_category_item_is_idempotent() {
        let store = temp_store();
        let text = "hello-sync";
        let hash = crate::util::hash_text(text);

        let first = store
            .insert_received_category_item(
                "text".to_string(),
                hash.clone(),
                text.to_string(),
                text.to_string(),
                "工作".to_string(),
                Some("#0D9488".to_string()),
            )
            .unwrap();
        let second = store
            .insert_received_category_item(
                "text".to_string(),
                hash.clone(),
                text.to_string(),
                text.to_string(),
                "工作".to_string(),
                Some("#0D9488".to_string()),
            )
            .unwrap();

        assert_eq!(first.id, second.id, "duplicate sync should return existing item");
        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM category_items WHERE category_id = ?1 AND content_hash = ?2",
                rusqlite::params![first.category_id, hash],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "no duplicate category_items row");
    }

    /// 空分组名应被拒绝（复用 clean_category_name 校验）。
    #[test]
    fn insert_received_category_item_rejects_empty_name() {
        let store = temp_store();
        let result = store.insert_received_category_item(
            "text".to_string(),
            crate::util::hash_text("x"),
            "x".to_string(),
            "x".to_string(),
            "   ".to_string(),
            None,
        );
        assert!(result.is_err());
    }

    /// 接收分组条目时，clip_snapshot_id 必须指向一条真实存在的 clips 行
    /// （而非孤立的随机 id），保证与本地「加入分组」及 clips 合并逻辑一致。
    #[test]
    fn insert_received_category_item_creates_backing_clip_row() {
        let store = temp_store();
        let text = "hello-sync";
        let hash = crate::util::hash_text(text);
        let item = store
            .insert_received_category_item(
                "text".to_string(),
                hash.clone(),
                text.to_string(),
                text.to_string(),
                "工作".to_string(),
                Some("#0D9488".to_string()),
            )
            .unwrap();

        let conn = store.connect().unwrap();
        let clip_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM clips WHERE id = ?1 AND content_hash = ?2 AND text = ?3",
                rusqlite::params![item.clip_snapshot_id, hash, text],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(clip_count, 1, "clip_snapshot_id must reference a real clips row");
    }

    /// 历史表已有同内容时，复用已有 clips.id 作 snapshot 引用，不新建占位行。
    #[test]
    fn insert_received_category_item_reuses_existing_clip_row() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        // 预置一条历史记录
        seed_clip(&conn, "text", "hello-sync", "hello-sync");
        let existing_clip_id: String = conn
            .query_row(
                "SELECT id FROM clips WHERE text = 'hello-sync'",
                rusqlite::params![],
                |row| row.get(0),
            )
            .unwrap();
        let clips_before: i64 = conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .unwrap();

        let item = store
            .insert_received_category_item(
                "text".to_string(),
                crate::util::hash_text("hello-sync"),
                "hello-sync".to_string(),
                "hello-sync".to_string(),
                "工作".to_string(),
                None,
            )
            .unwrap();

        assert_eq!(item.clip_snapshot_id, existing_clip_id, "should reuse existing clips.id");
        let clips_after: i64 = conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .unwrap();
        assert_eq!(clips_after, clips_before, "no new clips row should be created");
    }

    /// get_category_with_conn 能按 id 取到分组。
    #[test]
    fn get_category_with_conn_returns_existing() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let id = create_category(&conn, "A", "#abc", 3);
        let cat = store.get_category_with_conn(&conn, &id).unwrap();
        assert_eq!(cat.name, "A");
        assert_eq!(cat.color, "#abc");
        assert_eq!(cat.sort_order, 3);
    }

    #[test]
    fn get_category_with_conn_missing_is_error() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        assert!(store.get_category_with_conn(&conn, "nope").is_err());
    }

    /// 历史条目已加入分类时，`get_category_for_clip_with_conn` 按内容 hash 查到所属分类。
    /// 刻意让 category_items.clip_snapshot_id 指向另一个 id（模拟历史孤儿 snapshot 遗留），
    /// 验证按 content_hash 关联不依赖 snapshot id。
    #[test]
    fn get_category_for_clip_returns_joined_category() {
        let store = temp_store();
        let conn = store.connect().unwrap();

        // 手工插入一条已知 id 的历史 clip。
        let clip_id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        let text = "api-key-123";
        let hash = crate::util::hash_text(text);
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, 'text', ?2, NULL, ?3, ?4, 'test', ?5, 0, 0)",
            rusqlite::params![clip_id, hash, text, text, now],
        )
        .unwrap();
        let cat_id = create_category(&conn, "api_key", "#3B82F6", 0);
        let item_id = crate::new_id();
        conn.execute(
            "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
             VALUES (?1, ?2, 'orphan-snapshot-id', 'text', ?3, NULL, ?4, ?4, 0, ?5, ?5, 'local', 0)",
            rusqlite::params![item_id, cat_id, hash, text, chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        let found = store
            .get_category_for_clip_with_conn(&conn, &hash)
            .unwrap()
            .expect("joined clip should resolve to its category");
        assert_eq!(found.name, "api_key");
        assert_eq!(found.color, "#3B82F6");
    }

    /// 未加入任何分类的历史条目返回 None（发送侧保持无分组旧行为）。
    #[test]
    fn get_category_for_clip_returns_none_when_not_joined() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_clip(&conn, "text", "plain", "plain text");
        let clip_id: String = conn
            .query_row("SELECT id FROM clips LIMIT 1", [], |row| row.get(0))
            .unwrap();
        let hash: String = conn
            .query_row("SELECT content_hash FROM clips WHERE id = ?1", [clip_id], |row| row.get(0))
            .unwrap();

        let found = store.get_category_for_clip_with_conn(&conn, &hash).unwrap();
        assert!(found.is_none(), "unjoined clip must resolve to None");
    }

    /// 同一条目加入多个分类时取最近更新的那个。
    #[test]
    fn get_category_for_clip_prefers_latest_category() {
        let store = temp_store();
        let conn = store.connect().unwrap();

        let clip_id = crate::new_id();
        let now = chrono::Utc::now().to_rfc3339();
        let hash = crate::util::hash_text("multi-cat");
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, 'text', ?2, NULL, ?3, ?4, 'test', ?5, 0, 0)",
            rusqlite::params![clip_id, hash, "multi-cat", "multi-cat", now],
        )
        .unwrap();

        let cat_a = create_category(&conn, "older", "#111111", 0);
        let cat_b = create_category(&conn, "newer", "#222222", 1);
        // 两条 category_items 指向同一内容 hash，updated_at 明确一旧一新。
        for (item_id, cat_id, updated) in [
            (crate::new_id(), &cat_a, "2024-01-01T00:00:00Z"),
            (crate::new_id(), &cat_b, "2025-01-01T00:00:00Z"),
        ] {
            conn.execute(
                "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
                 VALUES (?1, ?2, ?3, 'text', ?4, NULL, ?5, ?5, 0, ?6, ?6, 'local', 0)",
                rusqlite::params![item_id, cat_id, clip_id, hash, "multi-cat", updated],
            )
            .unwrap();
        }

        let found = store
            .get_category_for_clip_with_conn(&conn, &hash)
            .unwrap()
            .expect("joined clip should resolve");
        assert_eq!(found.name, "newer", "most recently updated category wins");
    }
}
