use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

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
    /// 常驻锚连接：只持有、不查询。职责：建库时一次性完成 WAL 切换
    /// （之后 connect() 不再逐连接执行 journal_mode pragma——那是 Windows 上
    /// 打开 -wal 的又一个竞态入口），并常驻持有库文件句柄。注意：文件删除
    /// 竞态的真正修复是每条连接的 PERSIST_WAL（见 persist_wal 注释）；
    /// 仅靠本连接并挡不住「连接关闭时删除 -wal/-shm」的路径。
    anchor: Arc<Mutex<Option<Connection>>>,
}

/// SQLITE_FCNTL_PERSIST_WAL（sqlite3.h 定义，libsqlite3-sys 未导出该符号 +
/// file_control 绑定，故此处自行声明；实现在 bundled 的 amalgamation 里）。
const SQLITE_FCNTL_PERSIST_WAL: std::os::raw::c_int = 10;

extern "C" {
    fn sqlite3_file_control(
        db: *mut rusqlite::ffi::sqlite3,
        z_db_name: *const std::os::raw::c_char,
        op: std::os::raw::c_int,
        p_arg: *mut std::os::raw::c_void,
    ) -> std::os::raw::c_int;
}

/// 禁止本连接在关闭时删除 -wal/-shm（SQLITE_FCNTL_PERSIST_WAL）。
///
/// 背景：WAL 库的「最后一个连接关闭」路径会删除 -wal/-shm；Windows 上这个
/// 删除与并发连接的重新打开/重建会互相干扰，而 winOpen 对读写创建失败会
/// **静默降级为只读**（sqlite3WalOpen 收到 SQLITE_OPEN_READONLY 后置
/// pWal->readOnly = WAL_RDONLY），该连接之后所有写事务都报
/// "attempt to write a readonly database"（SQLITE_READONLY / ext=8）——
/// 本文件 concurrent_connect_close_never_readonly 即稳定复现。PERSIST_WAL
/// 让关闭路径不再删除这两个文件（SQLite 官方为 Windows 场景提供的开关），
/// 删除/重建竞态从根源消失。标志按连接（文件对象）生效，故每条连接都要设。
fn persist_wal(conn: &Connection) -> Result<(), String> {
    let mut enabled: std::os::raw::c_int = 1;
    // SAFETY: 句柄来自活跃的 Connection；"main" 是本连接唯一库名；
    // 参数按 sqlite3_file_control 约定传入 *int。
    let rc = unsafe {
        sqlite3_file_control(
            conn.handle(),
            b"main\0".as_ptr().cast(),
            SQLITE_FCNTL_PERSIST_WAL,
            (&mut enabled as *mut std::os::raw::c_int).cast(),
        )
    };
    if rc != rusqlite::ffi::SQLITE_OK {
        return Err(format!("设置持久 WAL 失败（code {rc}）"));
    }
    Ok(())
}

/// 每操作连接（connect() 使用）：连接级配置——外键 + busy_timeout + PERSIST_WAL。
///
/// **刻意不含 `PRAGMA journal_mode = WAL`**：WAL 是数据库文件的持久属性，
/// 由 Store::new 的锚连接一次性切换并写入文件头，之后所有连接由 SQLite
/// 按文件头自动以 WAL 打开。
fn open_connection(db_path: &std::path::Path) -> Result<Connection, String> {
    let conn = Connection::open(db_path).map_err(|error| error.to_string())?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    // 多写方并发（v0.9.2 A3）：watcher 线程、N 个会话读任务与命令层各自
    // 持有独立连接，WAL 下写锁竞争时默认立即报 SQLITE_BUSY——捕获/收包会
    // 被整条丢弃。5s busy_timeout 让竞争方在 SQLite 内部排队重试。
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|error| error.to_string())?;
    persist_wal(&conn)?;
    Ok(conn)
}

/// 锚连接（Store::new / 兜底补齐使用）：在 open_connection 基础上做一次
/// WAL 切换。仅此一条路径执行 journal_mode pragma——见 open_connection 注释。
fn open_wal_connection(db_path: &std::path::Path) -> Result<Connection, String> {
    let conn = open_connection(db_path)?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    Ok(conn)
}

impl Store {
    pub(crate) fn new(db_path: PathBuf) -> Result<Self, String> {
        let is_first_launch = !db_path.exists();
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        // 锚连接先于一切建立：一次性完成 WAL 切换（见 Store.anchor 注释）。
        let anchor = open_wal_connection(&db_path)?;
        let store = Self {
            db_path,
            anchor: Arc::new(Mutex::new(Some(anchor))),
        };
        let conn = store.connect()?;
        store.migrate(&conn)?;
        if is_first_launch {
            store.seed_default_clips(&conn)?;
        }
        Ok(store)
    }

    pub(crate) fn connect(&self) -> Result<Connection, String> {
        let conn = open_connection(&self.db_path)?;
        // 兜底：任何构造路径漏建锚时在此补齐（正常流程 Store::new 已建好）。
        let mut anchor = self.anchor.lock().map_err(|error| error.to_string())?;
        if anchor.is_none() {
            *anchor = Some(open_wal_connection(&self.db_path)?);
        }
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

    /// 回归：并发高频 open→write/read→close 不得出现 "attempt to write a
    /// readonly database"。修复前 Store::connect 每条操作开关独立连接，
    /// Windows 上连接关闭路径删除/截断 -wal/-shm 与并发打开/重建竞态，winOpen
    /// 会静默降级为只读（见 persist_wal 注释）；修复 = PERSIST_WAL 常驻
    /// -wal/-shm + Store::new 锚连接一次性完成 WAL 切换（connect 不再重复
    /// 执行 journal_mode pragma）。LAN 集成测试（链接轮询 + 会话写库）即为
    /// 现实触发形态。
    #[test]
    fn concurrent_connect_close_never_readonly() {
        use std::sync::{Arc, Mutex};

        fn text_item(text: &str) -> crate::models::CapturedClipboardItem {
            crate::models::CapturedClipboardItem {
                clip_type: "text".to_string(),
                content_hash: crate::util::hash_text(text),
                preview_text: text.to_string(),
                text: text.to_string(),
                image_bytes: None,
                display_name: None,
            }
        }

        let store = Arc::new(temp_store());
        store
            .insert_captured_item(text_item("seed"))
            .expect("seed write");

        let failures = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut handles = Vec::new();
        for t in 0..4u64 {
            // 写线程：连接快速开关
            let store = Arc::clone(&store);
            let failures = Arc::clone(&failures);
            handles.push(std::thread::spawn(move || {
                for i in 0..150u64 {
                    let text = format!("hammer-{t}-{i}");
                    if let Err(reason) = store.insert_captured_item(text_item(&text)) {
                        failures.lock().unwrap().push(reason);
                        return;
                    }
                }
            }));
        }
        for _ in 0..4u64 {
            // 读线程：同样连接快速开关（模拟链接任务的高频 get_paired_device 轮询）
            let store = Arc::clone(&store);
            let failures = Arc::clone(&failures);
            handles.push(std::thread::spawn(move || {
                for _ in 0..150u64 {
                    if let Err(reason) = store.list_paired_devices() {
                        failures.lock().unwrap().push(reason);
                        return;
                    }
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let failures = failures.lock().unwrap();
        assert!(
            failures.is_empty(),
            "并发连接开关出现失败 {} 条：{:?}",
            failures.len(),
            &failures[..failures.len().min(5)]
        );
    }
}
