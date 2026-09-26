//! AppState 的按域聚合（Task 27）：把原本平铺在 AppState 上的字段收进各自的域结构体，
//! 命令层与模块代码仍以 `state.<domain>.<field>` 访问，语义与锁粒度都不变。

pub(crate) mod capture;
pub(crate) mod ocr;
pub(crate) mod shortcuts;
pub(crate) mod ui;
pub(crate) mod window;

pub(crate) use capture::CaptureState;
pub(crate) use ocr::OcrRuntime;
pub(crate) use shortcuts::ShortcutState;
pub(crate) use ui::UiHandles;
pub(crate) use window::WindowState;
