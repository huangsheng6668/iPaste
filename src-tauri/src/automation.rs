// automation.rs — 快捷动作后台执行器
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use rusqlite::Connection;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;

use tauri::Emitter;

use crate::events::{
    EVENT_AUTOMATION_RUN_FINISHED, EVENT_AUTOMATION_RUN_OUTPUT, EVENT_AUTOMATION_RUN_STARTED,
};
use crate::models::{AutomationAction, AutomationRunSummary};
use crate::store::Store;

pub(crate) const AUTOMATION_TIMEOUT_SECS: u64 = 600; // 10 分钟

/// 主进程退出后等待输出流收口的时长。孙进程可能仍持有管道写端，不能让
/// drain 任务无限等下去，否则运行记录又会卡在 running。
const DRAIN_TIMEOUT: Duration = Duration::from_secs(5);

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
    // running 记录已落库，后面要跨多个 await 等子进程，连接不能一直握着。
    drop(conn);

    let _ = app.emit(
        EVENT_AUTOMATION_RUN_STARTED,
        serde_json::json!({
            "runId": run_id,
            "automationId": automation_id,
            "startedAt": started_at,
        }),
    );

    // 所有路径都走同一收口逻辑：spawn/take/wait 失败如果直接 `?` 返回，
    // 记录会永远停在 running（has_running_automation_run 让该动作再也跑不了），
    // 前端也收不到 FINISHED 事件。
    let (status, exit_code, run_error) =
        match run_command_process(&app, store, &run_id, &automation_id, &action).await {
            Ok((status, exit_code)) => (status, exit_code, None),
            Err(error) => ("failed", None, Some(error)),
        };

    let finished = (|| -> Result<AutomationRunSummary, String> {
        let conn = store.connect()?;
        store.finish_automation_run(&conn, &run_id, status, exit_code)?;
        store
            .get_latest_automation_run(&conn, &action.id)?
            .ok_or_else(|| "运行记录缺失".to_string())
    })();
    if let Err(reason) = &finished {
        eprintln!("[automation] 更新运行记录失败：{reason}");
    }

    // 更新记录失败也要广播结束事件，至少让前端把 running 状态摘掉。
    let (emit_status, emit_exit_code, emit_started_at, emit_finished_at) = match &finished {
        Ok(summary) => (
            summary.status.clone(),
            summary.exit_code,
            summary.started_at.clone(),
            summary.finished_at.clone(),
        ),
        Err(_) => (
            status.to_string(),
            exit_code,
            started_at,
            Some(crate::util::now()),
        ),
    };
    let _ = app.emit(
        EVENT_AUTOMATION_RUN_FINISHED,
        serde_json::json!({
            "runId": run_id,
            "automationId": automation_id,
            "status": emit_status,
            "exitCode": emit_exit_code,
            "startedAt": emit_started_at,
            "finishedAt": emit_finished_at,
        }),
    );

    match run_error {
        Some(error) => Err(error),
        None => finished,
    }
}

/// 启动子进程并等待结束。超时或等待失败时终止整棵进程树，返回
/// `("success" | "failed" | "timed_out", exitCode)` 或错误描述。
async fn run_command_process(
    app: &tauri::AppHandle,
    store: &Store,
    run_id: &str,
    automation_id: &str,
    action: &AutomationAction,
) -> Result<(&'static str, Option<i64>), String> {
    let (program, mut base_args) = command_program();
    base_args.push(action.command.clone());
    let mut command = Command::new(program);
    command
        .args(&base_args)
        .current_dir(action.cwd.as_deref().unwrap_or("."))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // Unix 下让子进程自成进程组，超时才能用 kill(-pid) 连孙进程一起收掉。
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command.spawn().map_err(|e| format!("启动命令失败: {e}"))?;
    let pid = child.id();

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            terminate_process_tree(&mut child, pid).await;
            return Err("无法读取 stdout".to_string());
        }
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => {
            terminate_process_tree(&mut child, pid).await;
            return Err("无法读取 stderr".to_string());
        }
    };

    let stdout_task = tokio::spawn(drain_stream(
        app.clone(),
        store.clone(),
        run_id.to_string(),
        automation_id.to_string(),
        "stdout",
        stdout,
    ));
    let stderr_task = tokio::spawn(drain_stream(
        app.clone(),
        store.clone(),
        run_id.to_string(),
        automation_id.to_string(),
        "stderr",
        stderr,
    ));

    let outcome = match tokio::time::timeout(
        Duration::from_secs(AUTOMATION_TIMEOUT_SECS),
        child.wait(),
    )
    .await
    {
        Ok(Ok(exit)) => {
            let code = exit.code();
            Ok((
                if code == Some(0) { "success" } else { "failed" },
                code.map(|c| c as i64),
            ))
        }
        Ok(Err(error)) => {
            terminate_process_tree(&mut child, pid).await;
            Err(format!("等待命令失败: {error}"))
        }
        Err(_) => {
            terminate_process_tree(&mut child, pid).await;
            Ok(("timed_out", None))
        }
    };

    drain_bounded(stdout_task, stderr_task).await;
    outcome
}

/// 终止子进程及其后代。Windows 用 taskkill /T（sh -c 包装出的孙进程不在
/// 同一作业对象里，只杀直接子进程会留下孤儿），Unix 杀整个进程组。
async fn terminate_process_tree(child: &mut tokio::process::Child, pid: Option<u32>) {
    if let Some(pid) = pid {
        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        #[cfg(unix)]
        {
            // 负 pid 表示进程组；子进程已被 process_group(0) 设为组长。
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}

/// 有界等待两个输出 drain 任务收尾；孙进程仍持有管道时放弃等待并 abort，
/// 保证运行记录不会因为输出流不关闭而永久 running。
async fn drain_bounded(mut stdout_task: JoinHandle<()>, mut stderr_task: JoinHandle<()>) {
    if tokio::time::timeout(DRAIN_TIMEOUT, &mut stdout_task)
        .await
        .is_err()
    {
        stdout_task.abort();
    }
    if tokio::time::timeout(DRAIN_TIMEOUT, &mut stderr_task)
        .await
        .is_err()
    {
        stderr_task.abort();
    }
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
