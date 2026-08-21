//! Manga-OCR ONNX 推理 sidecar（mocr_engine）的路径发现与就绪判定。
//!
//! 推理本体在独立进程 `mocr_engine`（src/bin/mocr_engine.rs）——onnxruntime
//! 与主程序内 mnn 的 CRT 假设不同，进程隔离规避链接冲突。本模块只负责：
//! - sidecar 可执行文件发现（与当前进程同目录：dev 下同 target，安装后同安装目录）
//! - onnx 模型资产就绪判定（设置页下载的 encoder/decoder/vocab 三件套）
//! 进程生命周期（常驻行协议/超时/空闲回收）复用 mocr.rs 的服务管理。

use std::path::PathBuf;

use tauri::Manager;

/// onnx 模型目录内的必需三件套（与安装器清单、export 脚本产物一致）。
pub(crate) const ENCODER_FILE: &str = "encoder.onnx";
pub(crate) const DECODER_FILE: &str = "decoder.onnx";
pub(crate) const VOCAB_FILE: &str = "vocab.txt";

/// 与主进程同目录的 sidecar 可执行文件（tauri externalBin 布局；
/// dev 模式下两者同在 target/{debug,release}/）。
pub(crate) fn sidecar_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = if cfg!(windows) { "mocr_engine.exe" } else { "mocr_engine" };
    let path = dir.join(name);
    path.is_file().then_some(path)
}

/// 设置页下载的 onnx 模型就绪时返回其目录（app-data/ocr/mocr/models）。
pub(crate) fn installed_model_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    let dir = app
        .path()
        .app_data_dir()
        .ok()?
        .join("ocr")
        .join("mocr")
        .join("models");
    if dir.join(ENCODER_FILE).is_file()
        && dir.join(DECODER_FILE).is_file()
        && dir.join(VOCAB_FILE).is_file()
    {
        Some(dir)
    } else {
        None
    }
}
