//! OCR 域运行时状态：截图 OCR 会话与结果窗载荷缓存。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::models::OcrResultPayload;

/// OCR 域上下文（Task 27 聚合）。
pub struct OcrRuntime {
    /// 截图 OCR 的进行中会话（遮罩窗生命周期）。
    pub capture_session: Arc<Mutex<Option<crate::capture::CaptureSession>>>,
    /// 结果窗按 label 取用的载荷缓存。
    pub ocr_result_payloads: Arc<Mutex<HashMap<String, OcrResultPayload>>>,
}
