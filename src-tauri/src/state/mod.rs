//! AppState 的按域聚合（Task 27）：把原本平铺在 AppState 上的字段收进各自的域结构体，
//! 命令层与模块代码仍以 `state.<domain>.<field>` 访问，语义与锁粒度都不变。
//!
//! 迁移按域逐个提交，未迁移的域暂时保留平铺字段（过渡期两种形态共存）。

pub(crate) mod capture;

pub(crate) use capture::CaptureState;
