//! 设置读写（Task 32 从 `store/settings.rs` 单文件按职责拆出）。
//!
//! - `read`：快照入口、AppSettings 组装、云 OCR 就绪判定、同步相关读取；
//! - `write`：各设置项的 update_*（含 keyring 迁移与遗留清理的调用点）；
//! - `tests`：原先内联在文件尾部的单元测试。
//!
//! 注：计划中的 `secrets.rs` / `legacy.rs` 未单独拆出——keyring 访问早已独立在
//! `store/secrets.rs`（本模块只调用），遗留值清理则是 `settings()` 内的两行内联逻辑，
//! 为它单开模块属于"为拆而拆"，故保留在 `read` 内。

mod read;
#[cfg(test)]
mod tests;
mod write;
