// automation.rs — 快捷动作后台执行器
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;
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
            vec!["-NoProfile".into(), "-Command".into()],
        )
    }
    #[cfg(not(target_os = "windows"))]
    {
        ("/bin/zsh", vec!["-lc".into()])
    }
}

/// 确认门判定：`confirm_before_run=true` 每次确认；否则与已确认命令哈希比对，
/// 未确认过或命令已变都需要重新确认。纯函数，便于单测。
pub(crate) fn needs_confirm(action: &AutomationAction, stored_hash: Option<&str>) -> bool {
    if action.confirm_before_run {
        return true;
    }
    stored_hash != Some(crate::util::hash_text(action.command.trim()).as_str())
}

/// execute_automation 的错误：Declined 区分"用户在原生对话框拒绝"（不产生运行记录）。
#[derive(Debug)]
pub(crate) enum AutomationRunError {
    Declined,
    Other(String),
}

/// 全局单对话框守卫：同一时刻只允许一个原生确认对话框，防对话框洪泛。
/// 核心逻辑参数化在 *_in 上便于单测；static 委托。
static CONFIRM_DIALOG_GUARD: Mutex<Option<String>> = Mutex::new(None);

fn acquire_confirm_slot_in(
    slot: &Mutex<Option<String>>,
    action_id: &str,
    busy_message: &str,
) -> Result<(), String> {
    let mut guard = slot.lock().expect("确认对话框守卫锁中毒");
    if guard.is_some() {
        return Err(busy_message.to_string());
    }
    *guard = Some(action_id.to_string());
    Ok(())
}

fn release_confirm_slot_in(slot: &Mutex<Option<String>>, action_id: &str) {
    let mut guard = slot.lock().expect("确认对话框守卫锁中毒");
    if guard.as_deref() == Some(action_id) {
        *guard = None;
    }
}

fn acquire_confirm_slot(action_id: &str, language: &str) -> Result<(), AutomationRunError> {
    acquire_confirm_slot_in(
        &CONFIRM_DIALOG_GUARD,
        action_id,
        crate::util::localized_text(language, "automation_confirm_busy"),
    )
    .map_err(AutomationRunError::Other)
}

fn release_confirm_slot(action_id: &str) {
    release_confirm_slot_in(&CONFIRM_DIALOG_GUARD, action_id);
}

/// 原生确认对话框（webview 外，XSS 无法合成或点击）。
/// 显示命令超 1200 字符截断加标记；哈希始终按全文计算（见调用方）。
fn show_confirm_dialog(app: &tauri::AppHandle, action: &AutomationAction, language: &str) -> bool {
    // tauri-plugin-dialog 2.7.3 无 blocking_ask 快捷方法，走 builder 等价形式：
    // Yes/No 双钮、阻塞等待、Yes 返回 true；须在非主线程调用（此处为 async worker）。
    use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
    let title = crate::util::localized_text(language, "automation_confirm_title");
    let (command_display, truncated) = truncate_log(&action.command, 1200);
    let command_display = if truncated {
        format!("{command_display}…")
    } else {
        command_display
    };
    let body = crate::util::localized_text(language, "automation_confirm_body")
        .replace("{name}", &action.name)
        .replace("{command}", &command_display);
    app.dialog()
        .message(body)
        .title(title)
        .buttons(MessageDialogButtons::YesNo)
        .blocking_show()
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
) -> Result<AutomationRunSummary, AutomationRunError> {
    if let Some(cwd) = action.cwd.as_deref() {
        if !PathBuf::from(cwd).is_dir() {
            return Err(AutomationRunError::Other(format!("工作目录不存在: {cwd}")));
        }
    }

    // has_running 先于确认对话框：对着已运行动作不弹框。
    let stored_hash = {
        let conn = store.connect().map_err(AutomationRunError::Other)?;
        if store
            .has_running_automation_run(&conn, &action.id)
            .map_err(AutomationRunError::Other)?
        {
            return Err(AutomationRunError::Other("该动作已在运行".to_string()));
        }
        store
            .confirmed_command_hash_with_conn(&conn, &action.id)
            .map_err(AutomationRunError::Other)?
    };

    // sudo 模型确认门：confirm_before_run 每次确认；否则命令哈希不匹配（含从未确认）时确认。
    if needs_confirm(&action, stored_hash.as_deref()) {
        // 语言读取与 tray.rs 同一模式；settings() 失败回落默认语言。
        let language = store
            .settings()
            .map(|settings| settings.language.as_str())
            .unwrap_or(crate::DEFAULT_LANGUAGE);
        acquire_confirm_slot(&action.id, language)?;
        let approved = show_confirm_dialog(&app, &action, language);
        let remember_hash = approved && !action.confirm_before_run;
        release_confirm_slot(&action.id);
        if !approved {
            return Err(AutomationRunError::Declined);
        }
        if remember_hash {
            let hash = crate::util::hash_text(action.command.trim());
            let conn = store.connect().map_err(AutomationRunError::Other)?;
            store
                .set_confirmed_command_hash_with_conn(&conn, &action.id, &hash)
                .map_err(AutomationRunError::Other)?;
        }
    }

    let run_id = {
        let conn = store.connect().map_err(AutomationRunError::Other)?;
        store
            .insert_automation_run(&conn, &action.id)
            .map_err(AutomationRunError::Other)?
    };
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
        Some(error) => Err(AutomationRunError::Other(error)),
        None => finished.map_err(AutomationRunError::Other),
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

    #[test]
    fn needs_confirm_covers_all_paths() {
        let mut action = AutomationAction {
            id: "id".into(),
            name: "n".into(),
            command: "echo 1".into(),
            cwd: None,
            run_mode: "background".into(),
            confirm_before_run: false,
            close_panel_on_success: false,
            sort_order: 0,
            created_at: String::new(),
            updated_at: String::new(),
            last_run: None,
        };
        let hash = crate::util::hash_text("echo 1");

        // confirm_before_run=true：无论哈希如何都确认
        action.confirm_before_run = true;
        assert!(needs_confirm(&action, Some(&hash)));

        // 从未确认 / 哈希不匹配：确认
        action.confirm_before_run = false;
        assert!(needs_confirm(&action, None));
        assert!(needs_confirm(&action, Some("deadbeef")));

        // 哈希匹配当前命令：放行
        assert!(!needs_confirm(&action, Some(&hash)));
    }

    #[test]
    fn confirm_slot_guard_rejects_concurrent_and_releases_by_id() {
        let slot = std::sync::Mutex::new(None::<String>);

        assert!(acquire_confirm_slot_in(&slot, "a", "busy").is_ok());
        // 第二个确认请求被拒
        assert_eq!(
            acquire_confirm_slot_in(&slot, "b", "busy").unwrap_err(),
            "busy"
        );
        // 释放别人的 id 不生效
        release_confirm_slot_in(&slot, "wrong");
        assert!(acquire_confirm_slot_in(&slot, "b", "busy").is_err());
        // 释放自己的 id 后可重新占用
        release_confirm_slot_in(&slot, "a");
        assert!(acquire_confirm_slot_in(&slot, "b", "busy").is_ok());
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn command_program_has_no_execution_policy_bypass() {
        let (_, args) = command_program();
        assert!(!args.iter().any(|a| a == "-ExecutionPolicy"));
        assert!(!args.iter().any(|a| a == "Bypass"));
    }
}
