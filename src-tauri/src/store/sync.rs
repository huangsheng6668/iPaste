// store/sync.rs — 云同步
use rusqlite::{params, Connection};

use crate::models::*;
use crate::util::*;
use super::Store;
use crate::{
    cloud::{cloud_post, is_syncable_clip_type, test_cloud_connection},
    store::rows::{collect_rows, map_category_item},
    util::now,
};

impl Store {
    pub(crate) fn update_cloud_settings(
        &self,
        api_address: String,
        api_key: String,
    ) -> Result<AppSettings, String> {
        let api_address = clean_api_address(api_address)?;
        let api_key = clean_api_key(api_key)?;
        test_cloud_connection(&api_address, &api_key)?;

        let conn = self.connect()?;
        // v0.3.29+：API Key 不再明文入库——写系统凭据库，settings 列存空串占位。
        // 凭据库写入失败直接报错，不做明文静默回退（安全优先于可用性）。
        super::secrets::put_api_key(&api_key)?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('cloud_api_address', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![api_address],
        )
        .map_err(|error| error.to_string())?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('cloud_api_key', '')
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [],
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
        // 先删凭据库条目（幂等），再清 settings，保证停用后密钥彻底移除。
        super::secrets::delete_api_key()?;
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

    pub(super) fn record_tombstone_with_conn(
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
