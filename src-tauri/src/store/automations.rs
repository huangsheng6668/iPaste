// store/automations.rs — 快捷动作 CRUD + 运行记录
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{AutomationAction, AutomationInput, AutomationRunDetail, AutomationRunSummary};
use crate::{
    store::rows::{map_automation, map_automation_run_detail, map_automation_run_summary},
    util::new_id,
};

use super::Store;

pub(crate) const AUTOMATION_COMMAND_MAX_LEN: usize = 4000;
pub(crate) const AUTOMATION_LOG_LIMIT_BYTES: usize = 200 * 1024;
/// 每个动作保留的最近运行记录数：超出部分在运行结束时清理，
/// 避免带 200KB 日志的运行记录无限累积。
const AUTOMATION_RUN_HISTORY_LIMIT: i64 = 50;

impl Store {
    pub(crate) fn list_automations(&self) -> Result<Vec<AutomationAction>, String> {
        let conn = self.connect()?;
        self.list_automations_with_conn(&conn)
    }

    fn list_automations_with_conn(
        &self,
        conn: &Connection,
    ) -> Result<Vec<AutomationAction>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, command, cwd, run_mode, confirm_before_run, close_panel_on_success, sort_order, created_at, updated_at
                 FROM automations ORDER BY sort_order, created_at",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], map_automation)
            .map_err(|e| e.to_string())?;
        let mut actions = Vec::new();
        for row in rows {
            let mut action = row.map_err(|e| e.to_string())?;
            action.last_run = self.get_latest_automation_run(conn, &action.id)?;
            actions.push(action);
        }
        Ok(actions)
    }

    pub(crate) fn create_automation(
        &self,
        input: AutomationInput,
    ) -> Result<AutomationAction, String> {
        validate_automation_input(&input)?;
        let conn = self.connect()?;
        let id = new_id();
        let created_at = crate::util::now();
        let sort_order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM automations",
                [],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO automations (id, name, command, cwd, run_mode, confirm_before_run, close_panel_on_success, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'background', ?5, ?6, ?7, ?8, ?8)",
            params![
                id,
                input.name.trim(),
                input.command.trim(),
                input.cwd,
                input.confirm_before_run,
                input.close_panel_on_success,
                sort_order,
                created_at
            ],
        )
        .map_err(|e| e.to_string())?;
        self.get_automation_with_conn(&conn, &id)
    }

    pub(crate) fn update_automation(
        &self,
        id: &str,
        input: AutomationInput,
    ) -> Result<AutomationAction, String> {
        validate_automation_input(&input)?;
        let conn = self.connect()?;
        let updated_at = crate::util::now();
        let changed = conn
            .execute(
                // 哈希材料绑定命令+cwd：任一变化都清空确认记忆（cwd 决定相对
                // 路径命令实际执行哪个文件）。SET 表达式中的列引用旧值；
                // `IS` 做 NULL 安全比较（绑定的 ?3 为 Option<String> 可为 NULL）。
                "UPDATE automations
                 SET name = ?1, command = ?2, cwd = ?3, confirm_before_run = ?4, close_panel_on_success = ?5, updated_at = ?6,
                     confirmed_command_hash = CASE WHEN command = ?2 AND cwd IS ?3 THEN confirmed_command_hash ELSE NULL END
                 WHERE id = ?7",
                params![
                    input.name.trim(),
                    input.command.trim(),
                    input.cwd,
                    input.confirm_before_run,
                    input.close_panel_on_success,
                    updated_at,
                    id
                ],
            )
            .map_err(|e| e.to_string())?;
        if changed == 0 {
            return Err("动作不存在".to_string());
        }
        self.get_automation_with_conn(&conn, id)
    }

    pub(crate) fn delete_automation(&self, id: &str) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM automations WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn get_automation_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<AutomationAction, String> {
        let mut action: AutomationAction = conn
            .query_row(
                "SELECT id, name, command, cwd, run_mode, confirm_before_run, close_panel_on_success, sort_order, created_at, updated_at
                 FROM automations WHERE id = ?1",
                params![id],
                map_automation,
            )
            .optional()
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "动作不存在".to_string())?;
        action.last_run = self.get_latest_automation_run(conn, id)?;
        Ok(action)
    }

    pub(crate) fn insert_automation_run(
        &self,
        conn: &Connection,
        automation_id: &str,
    ) -> Result<String, String> {
        let id = new_id();
        let started_at = crate::util::now();
        conn.execute(
            "INSERT INTO automation_runs (id, automation_id, status, exit_code, stdout, stderr, started_at, finished_at, duration_ms)
             VALUES (?1, ?2, 'running', NULL, '', '', ?3, NULL, NULL)",
            params![id, automation_id, started_at],
        )
        .map_err(|e| e.to_string())?;
        Ok(id)
    }

    /// 追加一段输出（调用方已按行/字节/时间阈值合并成批）。
    /// 单条 SQL 完成拼接与截断：substr 按 TEXT 字符计数，与原先
    /// 「SELECT 全量 → 内存拼接 → chars().take 截断 → UPDATE 全量」
    /// 语义一致，但省一次往返，也不再把 200KB 日志来回搬。
    pub(crate) fn append_automation_run_output(
        &self,
        conn: &Connection,
        run_id: &str,
        stream: &str,
        chunk: &str,
    ) -> Result<(), String> {
        let column = if stream == "stderr" {
            "stderr"
        } else {
            "stdout"
        };
        conn.execute(
            &format!(
                "UPDATE automation_runs
                 SET {column} = substr({column} || ?1, 1, ?2)
                 WHERE id = ?3"
            ),
            params![chunk, AUTOMATION_LOG_LIMIT_BYTES as i64, run_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn finish_automation_run(
        &self,
        conn: &Connection,
        run_id: &str,
        status: &str,
        exit_code: Option<i64>,
    ) -> Result<(), String> {
        let finished_at = crate::util::now();
        let (started_at, automation_id): (String, String) = conn
            .query_row(
                "SELECT started_at, automation_id FROM automation_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|e| e.to_string())?;
        let duration_ms = chrono::DateTime::parse_from_rfc3339(&finished_at)
            .ok()
            .zip(chrono::DateTime::parse_from_rfc3339(&started_at).ok())
            .map(|(finish, start)| (finish - start).num_milliseconds().max(0));
        conn.execute(
            "UPDATE automation_runs SET status = ?1, exit_code = ?2, finished_at = ?3, duration_ms = ?4 WHERE id = ?5",
            params![status, exit_code, finished_at, duration_ms, run_id],
        )
        .map_err(|e| e.to_string())?;
        if let Err(reason) =
            self.prune_automation_runs_with_conn(conn, &automation_id, AUTOMATION_RUN_HISTORY_LIMIT)
        {
            eprintln!("[automation] 清理历史运行记录失败：{reason}");
        }
        Ok(())
    }

    /// 启动时清掉上一次进程遗留的 running 记录（运行器在应用进程内）。
    /// 未引入 single-instance 插件：同时启动两个实例时，后启动者会把前者
    /// 正在运行的记录标记失败；按单实例托盘应用假设接受。
    pub(crate) fn reap_running_automation_runs(&self, conn: &Connection) -> Result<usize, String> {
        let finished_at = crate::util::now();
        conn.execute(
            "UPDATE automation_runs
             SET status = 'failed', finished_at = ?1, duration_ms = NULL
             WHERE status = 'running'",
            params![finished_at],
        )
        .map_err(|e| e.to_string())
    }

    fn prune_automation_runs_with_conn(
        &self,
        conn: &Connection,
        automation_id: &str,
        keep: i64,
    ) -> Result<(), String> {
        conn.execute(
            "DELETE FROM automation_runs
             WHERE automation_id = ?1
               AND id NOT IN (
                 SELECT id FROM automation_runs
                 WHERE automation_id = ?1
                 ORDER BY started_at DESC, rowid DESC
                 LIMIT ?2
               )",
            params![automation_id, keep],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn get_latest_automation_run(
        &self,
        conn: &Connection,
        automation_id: &str,
    ) -> Result<Option<AutomationRunSummary>, String> {
        conn.query_row(
            "SELECT id, status, exit_code, started_at, finished_at, duration_ms
             FROM automation_runs WHERE automation_id = ?1 ORDER BY started_at DESC LIMIT 1",
            params![automation_id],
            map_automation_run_summary,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub(crate) fn get_automation_run_detail(
        &self,
        conn: &Connection,
        run_id: &str,
    ) -> Result<AutomationRunDetail, String> {
        conn.query_row(
            "SELECT id, automation_id, status, exit_code, stdout, stderr,
                    length(stdout) >= ?1 AS stdout_truncated,
                    length(stderr) >= ?1 AS stderr_truncated,
                    started_at, finished_at, duration_ms
             FROM automation_runs WHERE id = ?2",
            params![AUTOMATION_LOG_LIMIT_BYTES as i64, run_id],
            map_automation_run_detail,
        )
        .map_err(|e| e.to_string())
    }

    pub(crate) fn has_running_automation_run(
        &self,
        conn: &Connection,
        automation_id: &str,
    ) -> Result<bool, String> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE automation_id = ?1 AND status = 'running'",
                params![automation_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(count > 0)
    }

    /// 读取动作当前已确认的命令哈希（NULL = 从未确认；动作不存在也返回 None，
    /// 让确认门走"需要确认"的安全方向）。
    pub(crate) fn confirmed_command_hash_with_conn(
        &self,
        conn: &Connection,
        id: &str,
    ) -> Result<Option<String>, String> {
        let hash = conn
            .query_row(
                "SELECT confirmed_command_hash FROM automations WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        Ok(hash)
    }

    /// 记录用户在原生对话框中确认过的命令哈希。
    pub(crate) fn set_confirmed_command_hash_with_conn(
        &self,
        conn: &Connection,
        id: &str,
        hash: &str,
    ) -> Result<(), String> {
        conn.execute(
            "UPDATE automations SET confirmed_command_hash = ?2 WHERE id = ?1",
            params![id, hash],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

fn validate_automation_input(input: &AutomationInput) -> Result<(), String> {
    if input.name.trim().is_empty() {
        return Err("名称不能为空".to_string());
    }
    if input.command.trim().is_empty() {
        return Err("命令不能为空".to_string());
    }
    if input.command.trim().chars().count() > AUTOMATION_COMMAND_MAX_LEN {
        return Err("命令过长（最多 4000 字符）".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::test_support::{seed_n_automations, temp_store};
    use std::time::Instant;

    fn input(name: &str, command: &str) -> AutomationInput {
        input_with_cwd(name, command, None)
    }

    fn input_with_cwd(name: &str, command: &str, cwd: Option<&str>) -> AutomationInput {
        AutomationInput {
            name: name.to_string(),
            command: command.to_string(),
            cwd: cwd.map(|c| c.to_string()),
            confirm_before_run: false,
            close_panel_on_success: false,
        }
    }

    #[test]
    fn create_and_list_automations_round_trip() {
        let store = temp_store();
        let created = store.create_automation(input("pull", "git pull")).unwrap();
        assert_eq!(created.name, "pull");
        let all = store.list_automations().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].command, "git pull");
        assert!(all[0].last_run.is_none());
    }

    #[test]
    fn create_automation_rejects_empty_command() {
        let store = temp_store();
        assert!(store.create_automation(input("x", "  ")).is_err());
    }

    #[test]
    fn create_automation_rejects_overlong_command() {
        let store = temp_store();
        let long = "a".repeat(4001);
        assert!(store.create_automation(input("x", &long)).is_err());
    }

    #[test]
    fn update_and_delete_automation() {
        let store = temp_store();
        let created = store.create_automation(input("a", "echo 1")).unwrap();
        let updated = store
            .update_automation(&created.id, input("b", "echo 2"))
            .unwrap();
        assert_eq!(updated.name, "b");
        assert_eq!(updated.command, "echo 2");
        store.delete_automation(&created.id).unwrap();
        assert!(store.list_automations().unwrap().is_empty());
    }

    #[test]
    fn automation_runs_round_trip_with_truncation() {
        let store = temp_store();
        let action = store.create_automation(input("a", "echo 1")).unwrap();
        let conn = store.connect().unwrap();
        let run_id = store.insert_automation_run(&conn, &action.id).unwrap();
        let big = "x".repeat(AUTOMATION_LOG_LIMIT_BYTES + 100);
        store
            .append_automation_run_output(&conn, &run_id, "stdout", &big)
            .unwrap();
        store
            .finish_automation_run(&conn, &run_id, "success", Some(0))
            .unwrap();
        let latest = store
            .get_latest_automation_run(&conn, &action.id)
            .unwrap()
            .unwrap();
        assert_eq!(latest.status, "success");
        assert_eq!(latest.exit_code, Some(0));
        let detail = store.get_automation_run_detail(&conn, &run_id).unwrap();
        assert!(detail.stdout_truncated);
        assert_eq!(detail.stdout.chars().count(), AUTOMATION_LOG_LIMIT_BYTES);
    }

    #[test]
    fn reap_running_automation_runs_marks_failed_and_unblocks() {
        let store = temp_store();
        let action = store.create_automation(input("a", "echo 1")).unwrap();
        let conn = store.connect().unwrap();
        let run_id = store.insert_automation_run(&conn, &action.id).unwrap();
        assert!(store.has_running_automation_run(&conn, &action.id).unwrap());

        store.reap_running_automation_runs(&conn).unwrap();

        assert!(!store.has_running_automation_run(&conn, &action.id).unwrap());
        let (status, finished_at, duration): (String, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT status, finished_at, duration_ms FROM automation_runs WHERE id = ?1",
                params![run_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(status, "failed");
        assert!(finished_at.is_some());
        assert_eq!(duration, None);
    }

    #[test]
    fn automation_runs_are_pruned_to_latest_limit() {
        let store = temp_store();
        let action = store.create_automation(input("a", "echo 1")).unwrap();
        let conn = store.connect().unwrap();
        for _ in 0..55 {
            store.insert_automation_run(&conn, &action.id).unwrap();
        }
        let latest: String = conn
            .query_row(
                "SELECT id FROM automation_runs ORDER BY started_at DESC, rowid DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        store
            .finish_automation_run(&conn, &latest, "success", Some(0))
            .unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE automation_id = ?1",
                params![action.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, AUTOMATION_RUN_HISTORY_LIMIT);
        let kept: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE id = ?1",
                params![latest],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1, "最新运行记录必须保留");
    }

    #[test]
    fn confirmed_command_hash_round_trip_and_update_reset() {
        let store = temp_store();
        let created = store.create_automation(input("a", "echo 1")).unwrap();
        let conn = store.connect().unwrap();

        // 新建动作从未确认
        assert_eq!(
            store
                .confirmed_command_hash_with_conn(&conn, &created.id)
                .unwrap(),
            None
        );

        // 写入后可读回（哈希材料 = 命令 + cwd，与执行器同一helper绑定）
        let hash = crate::automation::automation_command_hash("echo 1", None);
        store
            .set_confirmed_command_hash_with_conn(&conn, &created.id, &hash)
            .unwrap();
        assert_eq!(
            store
                .confirmed_command_hash_with_conn(&conn, &created.id)
                .unwrap(),
            Some(hash.clone())
        );

        // 命令与 cwd 都未变：update 保留哈希
        store
            .update_automation(&created.id, input("a", "echo 1"))
            .unwrap();
        assert_eq!(
            store
                .confirmed_command_hash_with_conn(&conn, &created.id)
                .unwrap(),
            Some(hash)
        );

        // 命令未变但 cwd 变化：update 必须清空哈希
        // （相对路径命令换了 cwd 就是换了执行对象，不能沿用旧确认）
        store
            .update_automation(&created.id, input_with_cwd("a", "echo 1", Some("C:\\tmp")))
            .unwrap();
        assert_eq!(
            store
                .confirmed_command_hash_with_conn(&conn, &created.id)
                .unwrap(),
            None
        );

        // 带 cwd 的组合确认后，命令与 cwd 均未变：update 保留哈希
        let hash_cwd = crate::automation::automation_command_hash("echo 1", Some("C:\\tmp"));
        store
            .set_confirmed_command_hash_with_conn(&conn, &created.id, &hash_cwd)
            .unwrap();
        store
            .update_automation(&created.id, input_with_cwd("a", "echo 1", Some("C:\\tmp")))
            .unwrap();
        assert_eq!(
            store
                .confirmed_command_hash_with_conn(&conn, &created.id)
                .unwrap(),
            Some(hash_cwd)
        );

        // 命令变化：update 清空哈希
        store
            .update_automation(&created.id, input_with_cwd("a", "echo 2", Some("C:\\tmp")))
            .unwrap();
        assert_eq!(
            store
                .confirmed_command_hash_with_conn(&conn, &created.id)
                .unwrap(),
            None
        );
    }

    #[test]
    fn has_running_automation_run_detects_running() {
        let store = temp_store();
        let action = store.create_automation(input("a", "echo 1")).unwrap();
        let conn = store.connect().unwrap();
        let run_id = store.insert_automation_run(&conn, &action.id).unwrap();
        assert!(store.has_running_automation_run(&conn, &action.id).unwrap());
        store
            .finish_automation_run(&conn, &run_id, "success", Some(0))
            .unwrap();
        assert!(!store.has_running_automation_run(&conn, &action.id).unwrap());
    }

    #[test]
    fn list_automations_500_under_250ms() {
        let store = temp_store();
        let conn = store.connect().unwrap();
        seed_n_automations(&conn, 500);
        let start = Instant::now();
        let all = store.list_automations().unwrap();
        let elapsed = start.elapsed();
        assert_eq!(all.len(), 500);
        assert!(
            elapsed.as_millis() < 250,
            "list_automations 500 took {elapsed:?}"
        );
    }
}
