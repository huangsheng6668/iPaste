//! 应用错误契约：所有 Tauri 命令的统一错误类型。
//!
//! 序列化形状（前端据此分支，只允许按 code 判断，不许解析 message）：
//! `{ "code": "internal", "message": "…", "params": null }`
//!
//! 约定：新增错误先在此加变体与 code；`Internal` 兜底包装既有的 String 错误
//! （store 层等），后续阶段逐步消化为具体变体。
//!
//! v4 迁移说明：`PortInUse`/`SessionBusy`（TCP 固定端口会话的守门错误）已随
//! 旧传输栈移除；iroh 拨号无固定端口，Task 8 若需新的会话守门错误在此重建。

use serde::{ser::SerializeStruct, Serialize, Serializer};

#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// 兜底：包装既有 String 错误文案。
    #[error("{0}")]
    Internal(String),
}

impl AppError {
    pub(crate) fn code(&self) -> &'static str {
        match self {
            AppError::Internal(_) => "internal",
        }
    }

    fn params(&self) -> serde_json::Value {
        serde_json::Value::Null
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
