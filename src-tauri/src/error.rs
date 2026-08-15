//! 应用错误契约：所有 Tauri 命令的统一错误类型。
//!
//! 序列化形状（前端据此分支，只允许按 code 判断，不许解析 message）：
//! `{ "code": "port_in_use", "message": "端口 51777 被 xxx（PID 123）占用。…", "params": { "port": 51777, "name": "xxx", "pid": 123 } }`
//!
//! 约定：新增错误先在此加变体与 code；`Internal` 兜底包装既有的 String 错误
//! （store 层等），后续阶段逐步消化为具体变体。

use serde::{ser::SerializeStruct, Serialize, Serializer};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// 固定端口被其他进程占用（lan_create_session）。params: { port, name, pid }。
    #[error("端口 {port} 被 {name}（PID {pid}）占用。{detail}")]
    PortInUse {
        port: u16,
        name: String,
        pid: u32,
        detail: String,
    },
    /// 已有进行中的 LAN 会话，拒绝新建/加入。
    #[error("已有进行中的会话")]
    SessionBusy,
    /// 兜底：包装既有 String 错误文案。
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            AppError::PortInUse { .. } => "port_in_use",
            AppError::SessionBusy => "session_busy",
            AppError::Internal(_) => "internal",
        }
    }

    fn params(&self) -> serde_json::Value {
        match self {
            AppError::PortInUse { port, name, pid, .. } => json!({
                "port": port,
                "name": name,
                "pid": pid,
            }),
            _ => serde_json::Value::Null,
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> Self {
        AppError::Internal(message.into())
    }
}

impl From<String> for AppError {
    fn from(message: String) -> Self {
        AppError::Internal(message)
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut state = serializer.serialize_struct("AppError", 3)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("params", &self.params())?;
        state.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_in_use_serializes_code_message_params() {
        let error = AppError::PortInUse {
            port: 51777,
            name: "xxx.exe".to_string(),
            pid: 123,
            detail: "bind 失败".to_string(),
        };
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "port_in_use");
        assert_eq!(json["message"], "端口 51777 被 xxx.exe（PID 123）占用。bind 失败");
        assert_eq!(json["params"]["port"], 51777);
        assert_eq!(json["params"]["name"], "xxx.exe");
        assert_eq!(json["params"]["pid"], 123);
    }

    #[test]
    fn session_busy_serializes_without_params() {
        let json = serde_json::to_value(&AppError::SessionBusy).unwrap();
        assert_eq!(json["code"], "session_busy");
        assert_eq!(json["message"], "已有进行中的会话");
        assert!(json["params"].is_null());
    }

    #[test]
    fn string_converts_to_internal() {
        let error: AppError = "无效的放大窗口标签".to_string().into();
        let json = serde_json::to_value(&error).unwrap();
        assert_eq!(json["code"], "internal");
        assert_eq!(json["message"], "无效的放大窗口标签");
    }

    #[test]
    fn internal_helper_accepts_str() {
        assert_eq!(AppError::internal("x").code(), "internal");
    }
}
