//! 跨平台端口占用检测（Task 2）。
//!
//! 仅 `get_port_conflict` / `kill_port_process` 两个 `pub(crate)` 入口；
//! `parse_windows_pid` / `parse_macos_lsof` 是纯逻辑辅助函数，方便单测。

use std::process::Command;

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub(crate) struct PortConflict {
    pub(crate) pid: u32,
    pub(crate) name: String,
}

/// 从 Windows `netstat -ano` 输出行解析监听端口对应的 PID。
///
/// 形如 `  TCP    0.0.0.0:45130          0.0.0.0:0              LISTENING       52276`。
/// 同时要求行内出现 `:<port>` 与 `LISTENING` 标记，避免误命中 ESTABLISHED 等
/// 其它状态的连接。最后一个字段即 PID。
fn parse_windows_pid(line: &str, port: u16) -> Option<u32> {
    let needle = format!(":{port}");
    let fields: Vec<&str> = line.split_whitespace().collect();
    if !line.contains(&needle) || !line.contains("LISTENING") {
        return None;
    }
    fields.last()?.parse::<u32>().ok()
}

/// 从 macOS `lsof -nP -iTCP:<port> -sTCP:LISTEN` 输出行解析进程名 + PID。
///
/// 形如 `ipaste  52276  user  5u  IPv4 0x1234 0t0  TCP *:45130 (LISTEN)`：
/// 第一列为进程名、第二列为 PID。要求行内出现 `:<port>` 字面量。
fn parse_macos_lsof(line: &str, port: u16) -> Option<PortConflict> {
    if !line.contains(&format!(":{port}")) {
        return None;
    }
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 2 {
        return None;
    }
    Some(PortConflict {
        name: fields[0].to_string(),
        pid: fields[1].parse::<u32>().ok()?,
    })
}

/// 检查端口是否被占用；若被占用返回占用进程的 PID + 名称。
///
/// - Windows：`netstat -ano` → 行解析 PID → `tasklist /FI "PID eq <pid>"` 取进程名。
/// - macOS：`lsof -nP -iTCP:<port> -sTCP:LISTEN` → 行解析直接得到进程名 + PID。
/// - 其它平台：始终返回 `Ok(None)`。
pub(crate) fn get_port_conflict(port: u16) -> Result<Option<PortConflict>, String> {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("netstat")
            .arg("-ano")
            .output()
            .map_err(|e| format!("无法执行 netstat：{e}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            if let Some(pid) = parse_windows_pid(line, port) {
                let name = process_name_windows(pid).unwrap_or_else(|| "未知进程".to_string());
                return Ok(Some(PortConflict { pid, name }));
            }
        }
        return Ok(None);
    }
    #[cfg(target_os = "macos")]
    {
        let output = Command::new("lsof")
            .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
            .output()
            .map_err(|e| format!("无法执行 lsof：{e}"))?;
        let text = String::from_utf8_lossy(&output.stdout);
        // 跳过 lsof 的标题行（"COMMAND PID USER ..."）。
        for line in text.lines().skip(1) {
            if let Some(conflict) = parse_macos_lsof(line, port) {
                return Ok(Some(conflict));
            }
        }
        return Ok(None);
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = port;
        Ok(None)
    }
}

/// Windows 下通过 `tasklist /FI "PID eq <pid>" /FO CSV /NH` 取进程名。
#[cfg(target_os = "windows")]
fn process_name_windows(pid: u32) -> Option<String> {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().next()?;
    // CSV 行形如 `"ipaste.exe","52276","Console","1","12,345 K"`。
    let name = line.split(',').next()?.trim_matches('"').to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// 校验 pid 确实是当前占用同步端口的进程，防止前端传入任意 pid 杀进程。
pub(crate) fn verify_port_owner(conflict: Option<PortConflict>, pid: u32) -> Result<(), String> {
    match conflict {
        None => Err("同步端口当前未被占用".to_string()),
        Some(c) if c.pid == pid => Ok(()),
        Some(_) => Err("该进程未占用同步端口".to_string()),
    }
}

/// 结束占用端口的进程。
///
/// - Windows：`taskkill /F /PID <pid>`；非零退出视为失败。
/// - macOS：`kill <pid>`；非零退出视为失败。
/// - 其它平台：返回错误。
///
/// 注意：`Command::status()` 返回 `io::Result<ExitStatus>`，仅表示「命令能否启动」；
/// 命令自身执行失败（如 PID 不存在）会以非零 exit code 体现，必须额外检查
/// `ExitStatus::success()` 才能区分。
pub(crate) fn kill_port_process(pid: u32) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        let status = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status()
            .map_err(|e| format!("无法执行 taskkill：{e}"))?;
        if !status.success() {
            return Err(format!("taskkill 失败，退出码：{status}"));
        }
        Ok(())
    }
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("kill")
            .arg(pid.to_string())
            .status()
            .map_err(|e| format!("无法执行 kill：{e}"))?;
        if !status.success() {
            return Err(format!("kill 失败，退出码：{status}"));
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = pid;
        Err("当前平台不支持杀进程".to_string())
    }
}

#[cfg(test)]
mod port_tests {
    use super::*;

    #[test]
    fn parse_windows_netstat_line_finds_pid() {
        // Windows netstat 行：TCP 0.0.0.0:45130 0.0.0.0:0 LISTENING 52276
        let line = "  TCP    0.0.0.0:45130          0.0.0.0:0              LISTENING       52276";
        assert_eq!(parse_windows_pid(line, 45130), Some(52276u32));
    }

    #[test]
    fn parse_macos_lsof_line_finds_name_and_pid() {
        // macOS lsof 行：ipaste 52276 user 5u IPv4 0x... 0t0 TCP *:45130 (LISTEN)
        let line = "ipaste  52276  user  5u  IPv4 0x1234 0t0 TCP *:45130 (LISTEN)";
        let conflict = parse_macos_lsof(line, 45130);
        assert_eq!(conflict.as_ref().map(|c| c.name.as_str()), Some("ipaste"));
        assert_eq!(conflict.as_ref().map(|c| c.pid), Some(52276));
    }

    #[test]
    fn verify_port_owner_accepts_matching_pid() {
        let conflict = PortConflict { pid: 52276, name: "ipaste.exe".into() };
        assert!(verify_port_owner(Some(conflict), 52276).is_ok());
    }

    #[test]
    fn verify_port_owner_rejects_unrelated_pid() {
        let conflict = PortConflict { pid: 52276, name: "ipaste.exe".into() };
        let err = verify_port_owner(Some(conflict), 1234).unwrap_err();
        assert_eq!(err, "该进程未占用同步端口");
    }

    #[test]
    fn verify_port_owner_rejects_when_port_free() {
        let err = verify_port_owner(None, 52276).unwrap_err();
        assert_eq!(err, "同步端口当前未被占用");
    }
}
