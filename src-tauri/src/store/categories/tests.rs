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
            None,
            None,
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
            None,
            None,
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
            None,
            None,
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
            None,
            None,
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

/// LAN 整组接收：条目携带的重命名要落库；再次接收同名条目时，若本地未重命名
/// 则补齐对端重命名，本地已有重命名则不被覆盖。
#[test]
fn insert_received_category_item_stores_and_fills_display_name() {
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
            Some("登录口令".to_string()),
            None,
        )
        .unwrap();
    assert_eq!(first.display_name.as_deref(), Some("登录口令"));

    // 本地先清掉重命名，再收到带重命名的同一条目 → 应补齐。
    let conn = store.connect().unwrap();
    conn.execute(
        "UPDATE category_items SET display_name = NULL WHERE id = ?1",
        rusqlite::params![first.id],
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
            Some("登录口令".to_string()),
            None,
        )
        .unwrap();
    assert_eq!(second.display_name.as_deref(), Some("登录口令"), "本地未重命名时应补齐");

    // 本地已有自己的重命名 → 不被对端值覆盖。
    let third = store
        .insert_received_category_item(
            "text".to_string(),
            hash.clone(),
            text.to_string(),
            text.to_string(),
            "工作".to_string(),
            Some("#0D9488".to_string()),
            Some("对端的新名字".to_string()),
            None,
        )
        .unwrap();
    assert_eq!(third.display_name.as_deref(), Some("登录口令"), "本地重命名优先");
}

/// 批量接收时显式 sort_order 被采用（保持发送顺序），而非插到分组顶部。
#[test]
fn insert_received_category_item_respects_explicit_sort_order() {
    let store = temp_store();
    let conn = store.connect().unwrap();
    let cat_id = create_category(&conn, "工作", "#0D9488", 0);
    // 预置一条已存在的条目（sort_order = 5），模拟分组内已有内容。
    conn.execute(
        "INSERT INTO category_items (id, category_id, clip_snapshot_id, clip_type, content_hash, display_name, preview_text, text, sort_order, created_at, updated_at, sync_state, is_pinned)
         VALUES (?1, ?2, 'snap-old', 'text', 'old-hash', NULL, 'old', 'old', 5, '2024-01-01', '2024-01-01', 'local', 0)",
        rusqlite::params![crate::util::new_id(), cat_id],
    )
    .unwrap();

    // 批量第二、三条：base_order = 5 - 3 = 2，index 1 → 3，index 2 → 4。
    let a = store
        .insert_received_category_item(
            "text".to_string(),
            "hash-a".to_string(),
            "a".to_string(),
            "a".to_string(),
            "工作".to_string(),
            None,
            None,
            Some(3),
        )
        .unwrap();
    let b = store
        .insert_received_category_item(
            "text".to_string(),
            "hash-b".to_string(),
            "b".to_string(),
            "b".to_string(),
            "工作".to_string(),
            None,
            None,
            Some(4),
        )
        .unwrap();
    assert_eq!(a.sort_order, 3);
    assert_eq!(b.sort_order, 4);

    // 列表顺序（与 UI 一致）：a 在 b 前，且都排在旧条目（sort_order=5）之前。
    let items = store
        .list_category_items_for_category_with_conn(&conn, &cat_id)
        .unwrap();
    let ids: Vec<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(&ids[..2], [a.id.as_str(), b.id.as_str()], "新条目按预排顺序位于顶部");
    assert_eq!(items.len(), 3, "旧条目仍在列表中");
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
        None,
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
            None,
            None,
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
            None,
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
    let clip_id = crate::util::new_id();
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
    let item_id = crate::util::new_id();
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

    let clip_id = crate::util::new_id();
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
        (crate::util::new_id(), &cat_a, "2024-01-01T00:00:00Z"),
        (crate::util::new_id(), &cat_b, "2025-01-01T00:00:00Z"),
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
