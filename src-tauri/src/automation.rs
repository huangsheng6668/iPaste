// automation.rs — 快捷动作后台执行器
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use tauri::Emitter;

use crate::events::*;
use crate::models::{AutomationAction, AutomationRunSummary};
use crate::store::Store;

pub(crate) const AUTOMATION_TIMEOUT_SECS: u64 = 600; // 10 分钟

pub(crate) fn command_program() -> (&'static str, Vec<String>) {
    #[cfg(target_os = "windows")]
    {
        (
            "powershell.exe",
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-Command".into(),
            ],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        ("/bin/zsh", vec!["-lc".into()])
    }
}

pub(crate) fn truncate_log(text: &str, limit: usize) -> (String, bool) {
    if text.chars().count() <= limit {
        return (text.to_string(), false);
    }
    (text.chars().take(limit).collect(), true)
}

pub(crate) async fn execute_automation(
    app: tauri::AppHandle,
    store: &Store,
    action: AutomationAction,
) -> Result<AutomationRunSummary, String> {
    if let Some(cwd) = action.cwd.as_deref() {
        if !PathBuf::from(cwd).is_dir() {
            return Err(format!("工作目录不存在: {cwd}"));
        }
    }

    let conn = store.connect()?;
    if store.has_running_automation_run(&conn, &action.id)? {
        return Err("该动作已在运行".to_string());
    }

    let run_id = store.insert_automation_run(&conn, &action.id)?;
    let started_at = crate::util::now();
    let automation_id = action.id.clone();
    let _ = app.emit(
        EVENT_AUTOMATION_RUN_STARTED,
        serde_json::json!({
            "runId": run_id,
            "automationId": automation_id,
            "startedAt": started_at,
        }),
    );

    let (program, mut base_args) = command_program();
    base_args.push(action.command.clone());
    let mut child = Command::new(program)
        .args(&base_args)
        .current_dir(action.cwd.as_deref().unwrap_or("."))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动命令失败: {e}"))?;

    let stdout = child.stdout.take().ok_or("无法读取 stdout")?;
    let stderr = child.stderr.take().ok_or("无法读取 stderr")?;

    let stdout_task = tokio::spawn(drain_stream(
        app.clone(),
        store.clone(),
        run_id.clone(),
        automation_id.clone(),
        "stdout",
        stdout,
    ));
    let stderr_task = tokio::spawn(drain_stream(
        app.clone(),
        store.clone(),
        run_id.clone(),
        automation_id.clone(),
        "stderr",
        stderr,
    ));

    let (status, exit_code) = match tokio::time::timeout(Duration::from_secs(AUTOMATION_TIMEOUT_SECS), child.wait()).await {
        Ok(result) => {
            let exit = result.map_err(|e| format!("等待命令失败: {e}"))?;
            let code = exit.code();
            (if code == Some(0) { "success" } else { "failed" }, code.map(|c| c as i64))
        }
        Err(_) => {
            let _ = child.kill().await;
            ("timed_out", None)
        }
    };

    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let conn = store.connect()?;
    store.finish_automation_run(&conn, &run_id, status, exit_code)?;
    let summary = store
        .get_latest_automation_run(&conn, &action.id)?
        .ok_or_else(|| "运行记录缺失".to_string())?;

    let _ = app.emit(
        EVENT_AUTOMATION_RUN_FINISHED,
        serde_json::json!({
            "runId": run_id,
            "automationId": automation_id,
            "status": summary.status,
            "exitCode": summary.exit_code,
            "startedAt": summary.started_at,
            "finishedAt": summary.finished_at,
        }),
    );

    Ok(summary)
}

async fn drain_stream<R>(
    app: tauri::AppHandle,
    store: Store,
    run_id: String,
    automation_id: String,
    stream: &'static str,
    reader: R,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    let mut lines = BufReader::new(reader).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let chunk = format!("{line}\n");
        let _ = app.emit(
            EVENT_AUTOMATION_RUN_OUTPUT,
            serde_json::json!({
                "runId": run_id,
                "automationId": automation_id,
                "stream": stream,
                "chunk": chunk,
            }),
        );
        if let Ok(conn) = store.connect() {
            let _ = store.append_automation_run_output(&conn, &run_id, stream, &chunk);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_program_wraps_for_shell() {
        let (program, args) = command_program();
        assert!(!program.is_empty());
        assert!(args.iter().any(|a| a == "-Command" || a == "-lc"));
    }

    #[test]
    fn truncate_log_keeps_head_and_marks_truncated() {
        let (text, truncated) = truncate_log(&"x".repeat(500), 200);
        assert!(truncated);
        assert_eq!(text.chars().count(), 200);
        let (text2, truncated2) = truncate_log("short", 200);
        assert!(!truncated2);
        assert_eq!(text2, "short");
    }
}
