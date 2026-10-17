use std::{fs, path::PathBuf};

use rusqlite::Connection;

mod automations;
mod categories;
mod clips;
pub(crate) mod devices; // lan_sync 读写已配对设备（信任表）
mod migrations;
pub(crate) mod rows;
pub(crate) mod secrets; // lan_sync::identity 读写设备身份私钥
mod settings;
mod sync;

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
        // 一次性遗留清理（明文 Key 迁移 / 已移除引擎的凭据与占位行）：
        // 原先挂在设置读取路径上，读一次设置就写一次库/凭据库。
        store.cleanup_legacy_secrets(&conn);
        if is_first_launch {
            store.seed_default_clips(&conn)?;
        }
        // 存量图片缩略图后台补齐：百张 4K 图约需十几秒，绝不能阻塞启动；
        // 新捕获在 save_image_bytes 内即时生成，这里只处理升级前的存量。
        if store.needs_image_thumbnail_backfill(&conn)? {
            let backfill_store = store.clone();
            let _ = std::thread::Builder::new()
                .name("thumbnail-backfill".to_string())
                .spawn(move || backfill_store.backfill_image_thumbnails());
        }
        Ok(store)
    }

    pub(crate) fn connect(&self) -> Result<Connection, String> {
        let conn = Connection::open(&self.db_path).map_err(|error| error.to_string())?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|error| error.to_string())?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| error.to_string())?;
        // WAL + NORMAL：提交不再逐次 fsync（实测单条提交 ~1.8ms → ~0.01ms），
        // 仅在操作系统崩溃时可能丢最后几笔提交，应用崩溃不丢——剪贴板历史
        // 这类高频低价值写入是标准取舍（SQLite 官方对 WAL 的推荐组合）。
        conn.pragma_update(None, "synchronous", "NORMAL")
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
            count, 0,
            "temp_store() should yield a clean database with no seeded clips"
        );
    }

    /// 探测 bundled SQLite 是否带 FTS5（搜索优化的前置条件）。
    #[test]
    fn fts5_virtual_table_is_available() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        conn.execute(
            "CREATE VIRTUAL TABLE fts_probe USING fts5(text, tokenize='trigram')",
            [],
        )
        .expect("bundled SQLite 应编译 FTS5");
        conn.execute("INSERT INTO fts_probe (text) VALUES ('你好世界 hello')", [])
            .unwrap();
        let hits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_probe WHERE fts_probe MATCH 'hello'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(hits, 1);
        // trigram 支持子串匹配（中英文皆是）
        let substring: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts_probe WHERE fts_probe MATCH '好世'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(substring, 0, "trigram 少于 3 字符无法命中（预期限制）");
    }
}
