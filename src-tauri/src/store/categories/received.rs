// store/categories/received.rs — LAN 接收入库：按名找建分类、幂等合并、回填 clips 行
use rusqlite::{params, OptionalExtension};

use super::super::Store;
use crate::models::{Category, CategoryItem};
use crate::{
    store::rows::{map_category, map_category_item},
    util::{clean_category_name, clean_color, clean_display_name, new_id, now},
};

impl Store {
    /// LAN 同步接收侧：把收到的条目落到「按名称匹配的同名分组」下。
    ///
    /// 为保持与本地「加入分组」流程（`add_clip_to_category_with_conn`）及 clips 合并逻辑
    /// 一致，会先确保存在一条对应内容的 `clips` 行：若历史表已有同 content_hash 则复用其
    /// id，否则插入一条占位 clips 行；`clip_snapshot_id` 始终指向真实存在的 clips 记录。
    ///
    /// 幂等：同一分组下相同 content_hash 的条目不重复创建。
    /// color 仅在新建分组时采用；已有同名分组保持其原色。
    ///
    /// `display_name`：对端条目的重命名显示名。仅当本地已有同名条目且本地未重命名时
    /// 才会补齐，不覆盖 B 端用户自己的命名。
    /// `sort_order`：批量接收时由调用方预排的值（保持发送顺序）；`None` 走旧的
    /// 「插到分组顶部」（MIN - 1）行为。
    pub(crate) fn insert_received_category_item(
        &self,
        clip_type: String,
        content_hash: String,
        preview_text: String,
        text: String,
        category_name: String,
        category_color: Option<String>,
        display_name: Option<String>,
        sort_order: Option<i64>,
    ) -> Result<CategoryItem, String> {
        let category_name = clean_category_name(category_name)?;
        let display_name = clean_display_name(display_name)?;
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
        //    对端带重命名且本地未重命名时补齐 display_name（不覆盖本地命名）。
        if let Some(mut existing) = tx
            .query_row(
                "SELECT id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned
                 FROM category_items WHERE category_id = ?1 AND content_hash = ?2",
                params![category.id, content_hash],
                map_category_item,
            )
            .optional()
            .map_err(|error| error.to_string())?
        {
            if existing.display_name.is_none() {
                if let Some(name) = display_name.as_deref() {
                    let updated_at = now();
                    tx.execute(
                        "UPDATE category_items SET display_name = ?1, updated_at = ?2 WHERE id = ?3",
                        params![name, updated_at, existing.id],
                    )
                    .map_err(|error| error.to_string())?;
                    existing.display_name = Some(name.to_string());
                    existing.updated_at = updated_at;
                }
            }
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

        // 4. 插入 category_items。
        //    sort_order：批量接收时用调用方预排的值（保持发送顺序），否则取当前
        //    最小值 - 1，新条目排在分组顶部（旧行为）。
        let sort_order: i64 = match sort_order {
            Some(order) => order,
            None => tx
                .query_row(
                    "SELECT COALESCE(MIN(sort_order), 0) - 1 FROM category_items WHERE category_id = ?1",
                    params![category.id],
                    |row| row.get(0),
                )
                .map_err(|error| error.to_string())?,
        };
        let item = CategoryItem {
            id: new_id(),
            category_id: category.id.clone(),
            clip_snapshot_id: clip_id,
            clip_type,
            content_hash,
            display_name,
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
