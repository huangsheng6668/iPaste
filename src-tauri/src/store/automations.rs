// store/automations.rs — 快捷动作 CRUD + 运行记录
use rusqlite::{params, Connection, OptionalExtension};

use crate::models::{AutomationAction, AutomationInput, AutomationRunDetail, AutomationRunSummary};
use crate::{
    map_automation, map_automation_run_detail, map_automation_run_summary, new_id,
};

use super::Store;

pub(crate) const AUTOMATION_COMMAND_MAX_LEN: usize = 4000;
pub(crate) const AUTOMATION_LOG_LIMIT_BYTES: usize = 200 * 1024;

impl Store {
    pub(crate) fn list_automations(&self) -> Result<Vec<AutomationAction>, String> {
        let conn = self.connect()?;
        self.list_automations_with_conn(&conn)
    }

    fn list_automations_with_conn(&self, conn: &Connection) -> Result<Vec<AutomationAction>, String> {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, command, cwd, run_mode, confirm_before_run, close_panel_on_success, sort_order, created_at, updated_at
                 FROM automations ORDER BY sort_order, created_at",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], map_automation).map_err(|e| e.to_string())?;
        let mut actions = Vec::new();
        for row in rows {
            let mut action = row.map_err(|e| e.to_string())?;
            action.last_run = self.get_latest_automation_run(conn, &action.id)?;
            actions.push(action);
        }
        Ok(actions)
    }

    pub(crate) fn create_automation(&self, input: AutomationInput) -> Result<AutomationAction, String> {
        validate_automation_input(&input)?;
        let conn = self.connect()?;
        let id = new_id();
        let created_at = crate::util::now();
        let sort_order: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort_order), -1) + 1 FROM automations", [], |row| row.get(0))
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

    pub(crate) fn update_automation(&self, id: &str, input: AutomationInput) -> Result<AutomationAction, String> {
        validate_automation_input(&input)?;
        let conn = self.connect()?;
        let updated_at = crate::util::now();
        let changed = conn
            .execute(
                "UPDATE automations SET name = ?1, command = ?2, cwd = ?3, confirm_before_run = ?4, close_panel_on_success = ?5, updated_at = ?6 WHERE id = ?7",
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

    pub(crate) fn get_automation_with_conn(&self, conn: &Connection, id: &str) -> Result<AutomationAction, String> {
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

    pub(crate) fn insert_automation_run(&self, conn: &Connection, automation_id: &str) -> Result<String, String> {
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

    pub(crate) fn append_automation_run_output(&self, conn: &Connection, run_id: &str, stream: &str, chunk: &str) -> Result<(), String> {
        let column = if stream == "stderr" { "stderr" } else { "stdout" };
        let current: String = conn
            .query_row(
                &format!("SELECT {column} FROM automation_runs WHERE id = ?1"),
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        let mut next = format!("{current}{chunk}");
        if next.chars().count() > AUTOMATION_LOG_LIMIT_BYTES {
            next = next.chars().take(AUTOMATION_LOG_LIMIT_BYTES).collect();
        }
        conn.execute(
            &format!("UPDATE automation_runs SET {column} = ?1 WHERE id = ?2"),
            params![next, run_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(crate) fn finish_automation_run(&self, conn: &Connection, run_id: &str, status: &str, exit_code: Option<i64>) -> Result<(), String> {
        let finished_at = crate::util::now();
        let started_at: String = conn
            .query_row("SELECT started_at FROM automation_runs WHERE id = ?1", params![run_id], |row| row.get(0))
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
        Ok(())
    }

    pub(crate) fn get_latest_automation_run(&self, conn: &Connection, automation_id: &str) -> Result<Option<AutomationRunSummary>, String> {
        conn.query_row(
            "SELECT id, status, exit_code, started_at, finished_at, duration_ms
             FROM automation_runs WHERE automation_id = ?1 ORDER BY started_at DESC LIMIT 1",
            params![automation_id],
            map_automation_run_summary,
        )
        .optional()
        .map_err(|e| e.to_string())
    }

    pub(crate) fn get_automation_run_detail(&self, conn: &Connection, run_id: &str) -> Result<AutomationRunDetail, String> {
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

    pub(crate) fn has_running_automation_run(&self, conn: &Connection, automation_id: &str) -> Result<bool, String> {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_runs WHERE automation_id = ?1 AND status = 'running'",
                params![automation_id],
                |row| row.get(0),
            )
            .map_err(|e| e.to_string())?;
        Ok(count > 0)
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
    use crate::store::test_support::temp_store;

    fn input(name: &str, command: &str) -> AutomationInput {
        AutomationInput {
            name: name.to_string(),
            command: command.to_string(),
            cwd: None,
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
        let updated = store.update_automation(&created.id, input("b", "echo 2")).unwrap();
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
        store.finish_automation_run(&conn, &run_id, "success", Some(0)).unwrap();
        let latest = store.get_latest_automation_run(&conn, &action.id).unwrap().unwrap();
        assert_eq!(latest.status, "success");
        assert_eq!(latest.exit_code, Some(0));
        let detail = store.get_automation_run_detail(&conn, &run_id).unwrap();
        assert!(detail.stdout_truncated);
        assert_eq!(detail.stdout.chars().count(), AUTOMATION_LOG_LIMIT_BYTES);
    }

    #[test]
    fn has_running_automation_run_detects_running() {
        let store = temp_store();
        let action = store.create_automation(input("a", "echo 1")).unwrap();
        let conn = store.connect().unwrap();
        let run_id = store.insert_automation_run(&conn, &action.id).unwrap();
        assert!(store.has_running_automation_run(&conn, &action.id).unwrap());
        store.finish_automation_run(&conn, &run_id, "success", Some(0)).unwrap();
        assert!(!store.has_running_automation_run(&conn, &action.id).unwrap());
    }
}
