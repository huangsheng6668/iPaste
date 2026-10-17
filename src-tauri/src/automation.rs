// automation.rs — 快捷动作后台执行器
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use rusqlite::Connection;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use tauri::Emitter;

use crate::events::{
    EVENT_AUTOMATION_RUN_FINISHED, EVENT_AUTOMATION_RUN_OUTPUT, EVENT_AUTOMATION_RUN_STARTED,
};
use crate::models::{AutomationAction, AutomationRunSummary};
use crate::store::Store;

pub(crate) const AUTOMATION_TIMEOUT_SECS: u64 = 600; // 10 分钟

/// 运行输出写库的批量阈值：攒够这么多行，或距上次落盘超过这么久，就 flush 一次。
const LOG_FLUSH_LINES: usize = 16;
const LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(100);

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

// clippy 门禁豁免：保留实现（诊断/平台能力预留），当前无调用方。
#[allow(dead_code)]
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

    let (status, exit_code) = match tokio::time::timeout(
        Duration::from_secs(AUTOMATION_TIMEOUT_SECS),
        child.wait(),
    )
    .await
    {
        Ok(result) => {
            let exit = result.map_err(|e| format!("等待命令失败: {e}"))?;
            let code = exit.code();
            (
                if code == Some(0) { "success" } else { "failed" },
                code.map(|c| c as i64),
            )
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
    // 连接按流复用：原实现每行都 store.connect() 一次——WAL / foreign_keys /
    // busy_timeout 三个 pragma 加上文件打开，每行都重付一遍。
    let conn = store.connect().ok();
    let mut pending = String::new();
    let mut pending_lines = 0usize;
    let mut last_flush = std::time::Instant::now();

    while let Ok(Some(line)) = lines.next_line().await {
        let chunk = format!("{line}\n");
        // 前端输出保持逐行实时；批量只作用于 DB 落盘。
        let _ = app.emit(
            EVENT_AUTOMATION_RUN_OUTPUT,
            serde_json::json!({
                "runId": run_id,
                "automationId": automation_id,
                "stream": stream,
                "chunk": chunk,
            }),
        );
        if conn.is_none() {
            continue;
        }
        pending.push_str(&chunk);
        pending_lines += 1;
        // 攒够行数或距上次落盘够久就 flush：逐行落盘时每行都要全量读改
        // 整段日志（O(n^2)），批量后写库次数降一个量级。
        if pending_lines >= LOG_FLUSH_LINES || last_flush.elapsed() >= LOG_FLUSH_INTERVAL {
            flush_pending(conn.as_ref(), &store, &run_id, stream, &mut pending);
            pending_lines = 0;
            last_flush = std::time::Instant::now();
        }
    }
    // 流结束把残余一次写完。
    flush_pending(conn.as_ref(), &store, &run_id, stream, &mut pending);
}

/// 把攒批的输出一次性追加进运行记录。无连接可写时静默丢弃（前端已实时收到，
/// 与原先每行 connect 失败即跳过的行为一致）。
fn flush_pending(
    conn: Option<&Connection>,
    store: &Store,
    run_id: &str,
    stream: &str,
    pending: &mut String,
) {
    if pending.is_empty() {
        return;
    }
    if let Some(conn) = conn {
        let _ = store.append_automation_run_output(conn, run_id, stream, pending);
    }
    pending.clear();
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
