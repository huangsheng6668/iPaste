//! PaddleOCR 识别管线：ocr-rs（MNN 后端）进程内推理 + 行级结果到词级的纯映射。
//! 纯映射部分（PaddleLine / paddle_lines_to_words）平台无关、可脱离模型单测；
//! 引擎缓存与识别入口仅非 macOS 平台编译（与 installer.rs 的逐项 cfg 一致）。

#[cfg(not(target_os = "macos"))]
use crate::ocr::installer::{paddle_model_paths, OCR_ENGINE_ID};

use crate::ocr::tokens::split_line_tokens;

/// 单行识别结果的纯数据形态（映射逻辑可脱离模型单测）。
#[derive(Debug)]
pub(crate) struct PaddleLine {
    pub text: String,
    pub confidence: f64, // 0.0–1.0
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64, // 像素，轴对齐
}

/// 行级结果 → 词级 ImageOcrWord：
/// 每行 split_line_tokens 切词；行内 token 按 char 数比例横向切分行框；
/// blockIndex=0, paragraphIndex=0, lineIndex=行号, wordIndex=token 序号；
/// confidence 映射到 0–100。
pub(crate) fn paddle_lines_to_words(
    lines: &[PaddleLine],
    image_width: u32,
    image_height: u32,
) -> Vec<crate::models::ImageOcrWord> {
    let image_width = image_width as f64;
    let image_height = image_height as f64;
    let mut words = Vec::new();

    for (line_index, line) in lines.iter().enumerate() {
        let tokens = split_line_tokens(&line.text);
        if tokens.is_empty() {
            // 空白行既无 token，也避免下行 0 char 除零
            continue;
        }

        // 比例切分：unit = 行宽 / 行 char 总数（token 含空白间隔，均摊到每 char）
        let unit = line.width / line.text.chars().count() as f64;
        for (word_index, token) in tokens.iter().enumerate() {
            let left = (line.left + token.char_start as f64 * unit).clamp(0.0, image_width);
            let width = (token.char_len as f64 * unit)
                .max(1.0)
                .min((image_width - left).max(1.0));
            let top = line.top.clamp(0.0, image_height);
            let height = line.height.max(1.0).min((image_height - top).max(1.0));

            words.push(crate::models::ImageOcrWord {
                text: token.text.clone(),
                left,
                top,
                width,
                height,
                confidence: (line.confidence * 100.0).clamp(0.0, 100.0),
                block_index: 0,
                paragraph_index: 0,
                line_index: line_index as i64,
                word_index: word_index as i64,
            });
        }
    }

    words
}

/// 进程级引擎缓存：键为 ocr_mode（fast/best），模式变更时重建。
/// 模型加载约百毫秒级，识别频繁时避免重复构建；引擎 Box::leak 成
/// 进程寿命单例换取无锁识别路径，模式切换时旧引擎随之泄漏（每次切换
/// 最多泄漏一份模型，KB~MB 级，与 App 同生命周期，权衡后可接受）。
#[cfg(not(target_os = "macos"))]
static ENGINE: std::sync::Mutex<Option<(String, &'static ocr_rs::OcrEngine)>> =
    std::sync::Mutex::new(None);

/// 取当前模式的引擎：命中缓存直接返回；否则校验模型齐全后构建并缓存。
/// 仅在构建/查缓存时持锁，识别调用完全不持锁。
#[cfg(not(target_os = "macos"))]
fn ensure_engine(app: &tauri::AppHandle, mode: &str) -> Result<&'static ocr_rs::OcrEngine, String> {
    let mut cache = ENGINE.lock().map_err(|error| error.to_string())?;
    if let Some((cached_mode, engine)) = cache.as_ref() {
        if cached_mode == mode {
            return Ok(engine);
        }
    }

    let paths = paddle_model_paths(app, mode)?;
    if !paths.det.exists() || !paths.rec.exists() || !paths.charset.exists() {
        return Err("请先在偏好设置中下载图片 OCR 资源".to_string());
    }

    let engine = ocr_rs::OcrEngine::new(&paths.det, &paths.rec, &paths.charset, None)
        .map_err(|error| format!("初始化 PaddleOCR 引擎失败：{error}"))?;
    // 进程级单例：泄漏换取 &'static，识别无需持锁（见 ENGINE 注释）
    let engine: &'static ocr_rs::OcrEngine = Box::leak(Box::new(engine));
    *cache = Some((mode.to_string(), engine));
    Ok(engine)
}

/// 命令入口（被 ocr/mod.rs::recognize_image 的非 macOS 分支调用，spawn_blocking 内执行）：
/// 从 store 设置解析 ocr_mode 后进入识别管线。
#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_image_text_paddle(
    app: &tauri::AppHandle,
    store: &crate::store::Store,
    image_path: String,
) -> Result<crate::models::ImageOcrResult, String> {
    let mode = store.settings()?.ocr_mode;
    recognize_with_mode(app, &mode, image_path)
}

/// 降级入口：AppState 取不到（无法读设置）时由调度方按默认模式调用。
#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_with_mode(
    app: &tauri::AppHandle,
    mode: &str,
    image_path: String,
) -> Result<crate::models::ImageOcrResult, String> {
    let image_path = std::path::PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }
    let image = image::open(&image_path).map_err(|error| format!("无法读取图片：{error}"))?;

    let engine = ensure_engine(app, mode)?;
    let items = engine
        .recognize(&image)
        .map_err(|error| format!("PaddleOCR 识别失败：{error}"))?;

    let lines: Vec<PaddleLine> = items
        .iter()
        .map(|item| PaddleLine {
            text: item.text.clone(),
            confidence: item.confidence as f64,
            left: item.bbox.rect.left() as f64,
            top: item.bbox.rect.top() as f64,
            width: item.bbox.rect.width() as f64,
            height: item.bbox.rect.height() as f64,
        })
        .collect();

    let words = paddle_lines_to_words(&lines, image.width(), image.height());
    let text = lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(crate::models::ImageOcrResult {
        text,
        engine: OCR_ENGINE_ID.to_string(),
        language: "zh-Hans+en".to_string(),
        words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str) -> PaddleLine {
        PaddleLine {
            text: text.to_string(),
            confidence: 0.9,
            left: 10.0,
            top: 20.0,
            width: 100.0,
            height: 10.0,
        }
    }

    #[test]
    fn maps_lines_to_ordered_words_with_indices() {
        let words = paddle_lines_to_words(&[line("你好ab"), line("第二行")], 640, 480);
        // 行 1：你/好/ab → 3 词；行 2：3 词
        assert_eq!(words.len(), 6);
        assert_eq!(words[0].text, "你");
        assert_eq!(words[0].block_index, 0);
        assert_eq!(words[0].paragraph_index, 0);
        assert_eq!(words[0].line_index, 0);
        assert_eq!(words[0].word_index, 0);
        assert_eq!(words[2].text, "ab");
        assert_eq!(words[2].word_index, 2);
        assert_eq!(words[3].line_index, 1);
        assert!((words[3].confidence - 90.0).abs() < 1e-6);
    }

    #[test]
    fn proportional_boxes_partition_line_and_stay_inside() {
        let words = paddle_lines_to_words(&[line("abcde")], 640, 480);
        assert_eq!(words.len(), 1);
        let w = &words[0];
        assert!((w.left - 10.0).abs() < 1e-6);
        assert!(w.left + w.width <= 10.0 + 100.0 + 1e-6);

        // 拉丁连续串是 1 个 token，用 CJK 强制多 token：你好好 → 3 个 token，
        // 各占 1/3 行宽
        let cjk = paddle_lines_to_words(&[line("你好好")], 640, 480);
        assert_eq!(cjk.len(), 3);
        let total: f64 = cjk.iter().map(|w| w.width).sum();
        assert!((total - 100.0).abs() < 1e-6);
        assert!(cjk[1].left >= cjk[0].left + cjk[0].width - 1e-6);
    }

    #[test]
    fn empty_lines_yield_no_words() {
        assert!(paddle_lines_to_words(&[], 640, 480).is_empty());
        assert!(paddle_lines_to_words(&[line("   ")], 640, 480).is_empty());
    }
}
