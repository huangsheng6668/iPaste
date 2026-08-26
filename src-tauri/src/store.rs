use std::{fs, path::PathBuf};

use rusqlite::Connection;

mod migrations;
mod settings;
mod clips;
mod categories;
mod sync;
mod automations;
pub(crate) mod secrets; // lan_sync::identity 读写设备身份私钥
pub(crate) mod devices; // lan_sync 读写已配对设备（信任表）
pub(crate) mod rows;

#[cfg(test)]
pub(crate) mod test_support;

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
        // 多写方并发（v0.9.2 A3）：watcher 线程、N 个会话读任务与命令层各自
        // 持有独立连接，WAL 下写锁竞争时默认立即报 SQLITE_BUSY——捕获/收包会
        // 被整条丢弃。5s busy_timeout 让竞争方在 SQLite 内部排队重试。
        conn.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| error.to_string())?;
        Ok(conn)
    }

}

#[cfg(test)]
mod tests {
    use crate::store::test_support::temp_store;

    #[test]
    fn store_initializes_empty() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM clips", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            count,
            0,
            "temp_store() should yield a clean database with no seeded clips"
        );
    }
}

