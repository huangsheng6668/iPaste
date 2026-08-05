// store/test_support.rs — #[cfg(test)] 共享测试 helper
#![cfg(test)]

use crate::store::Store;

pub(crate) fn temp_store() -> Store {
    let dir = std::env::temp_dir().join(format!(
        "ipaste-test-{}-{}",
        std::process::id(),
        uuid_like()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("test.db");
    let store = Store::new(db_path).expect("store init");
    let conn = store.connect().expect("connect for cleanup");
    conn.execute("DELETE FROM category_items", [])
        .expect("clear category_items");
    conn.execute("DELETE FROM categories", [])
        .expect("clear categories");
    conn.execute("DELETE FROM clips", []).expect("clear clips");
    conn.execute("DELETE FROM automation_runs", [])
        .expect("clear automation_runs");
    conn.execute("DELETE FROM automations", []).expect("clear automations");
    store
}

pub(crate) fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{nanos}")
}

pub(crate) fn seed_clip(conn: &rusqlite::Connection, clip_type: &str, preview: &str, text: &str) {
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

pub(crate) fn create_category(
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

pub(crate) fn seed_category_item(
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
        rusqlite::params![id, category_id, id, clip_type, crate::util::hash_text(text), preview, text, now],
    )
    .unwrap();
}

pub(crate) fn seed_n_clips(conn: &rusqlite::Connection, n: usize) {
    for i in 0..n {
        let text = format!("clip item {i} hello world");
        conn.execute(
            "INSERT INTO clips (id, clip_type, content_hash, display_name, preview_text, text, source_app, last_captured_at, favorite_count, is_pinned)
             VALUES (?1, 'text', ?2, NULL, ?3, ?4, 'bench', ?5, 0, 0)",
            rusqlite::params![
                format!("bench-clip-{i}"),
                crate::util::hash_text(&text),
                text,
                text,
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .unwrap();
    }
}

pub(crate) fn seed_n_automations(conn: &rusqlite::Connection, n: usize) {
    for i in 0..n {
        conn.execute(
            "INSERT INTO automations (id, name, command, cwd, run_mode, confirm_before_run, close_panel_on_success, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, 'background', 0, 0, ?4, ?5, ?5)",
            rusqlite::params![
                format!("bench-automation-{i}"),
                format!("action {i}"),
                format!("echo {i}"),
                i as i64,
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .unwrap();
    }
}
