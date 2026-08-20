//! PaddleOCR 识别管线：ocr-rs（MNN 后端）进程内推理 + 行级结果到词级的纯映射。
//! 纯映射部分（PaddleLine / paddle_lines_to_words）平台无关、可脱离模型单测；
//! 引擎缓存与识别入口仅非 macOS 平台编译（与 installer.rs 的逐项 cfg 一致）。

#[cfg(not(target_os = "macos"))]
use crate::ocr::installer::{paddle_model_paths, OCR_ENGINE_ID};

use crate::ocr::tokens::split_line_tokens;

/// 单行识别结果的纯数据形态（映射逻辑可脱离模型单测）。
#[derive(Debug, Clone)]
pub(crate) struct PaddleLine {
    pub text: String,
    pub confidence: f64, // 0.0–1.0
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64, // 像素，轴对齐
}

/// 行级结果 → 词级 ImageOcrWord：
/// 每行 split_line_tokens 切词；
/// - 横向文本（height <= width）：token 按 char 数比例横向切分行框；
/// - 纵向文本（height > width）：token 按 char 数比例纵向切分行框；
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

        let char_count = line.text.chars().count() as f64;
        let is_vertical = line.height > line.width;

        if is_vertical {
            let unit = line.height / char_count;
            for (word_index, token) in tokens.iter().enumerate() {
                let left = line.left.clamp(0.0, image_width);
                let width = line.width.max(1.0).min((image_width - left).max(1.0));
                let top = (line.top + token.char_start as f64 * unit).clamp(0.0, image_height);
                let height = (token.char_len as f64 * unit)
                    .max(1.0)
                    .min((image_height - top).max(1.0));

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
        } else {
            // 比例切分：unit = 行宽 / 行 char 总数（token 含空白间隔，均摊到每 char）
            let unit = line.width / char_count;
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
    }

    words
}

/// 智能重排行级结果：
/// - 竖排为主（或 Manga 模式）：按 X 轴从右向左（递减）分列，列内按 Y 轴从上往下（递增）排序；
/// - 横排为主：按 Y 轴从上往下（递增）分行，行内按 X 轴从左向右（递增）排序。
pub(crate) fn sort_paddle_lines(lines: Vec<PaddleLine>, is_manga_profile: bool) -> Vec<PaddleLine> {
    if lines.len() <= 1 {
        return lines;
    }

    let vertical_count = lines.iter().filter(|l| l.height > l.width).count();
    let is_mostly_vertical = is_manga_profile || (vertical_count * 2 >= lines.len());

    if is_mostly_vertical {
        // 竖排：按中心 X 坐标从右向左（降序）
        let mut sorted = lines;
        sorted.sort_by(|a, b| {
            let cx_a = a.left + a.width / 2.0;
            let cx_b = b.left + b.width / 2.0;
            cx_b.partial_cmp(&cx_a).unwrap_or(std::cmp::Ordering::Equal)
        });

        // 将 X 坐标重叠相近的框聚合成同一竖列
        let mut columns: Vec<Vec<PaddleLine>> = Vec::new();
        for line in sorted {
            let cx = line.left + line.width / 2.0;
            let target_col = columns.iter_mut().find(|col| {
                let col_cx: f64 =
                    col.iter().map(|it| it.left + it.width / 2.0).sum::<f64>() / col.len() as f64;
                let avg_w: f64 = col.iter().map(|it| it.width).sum::<f64>() / col.len() as f64;
                let tolerance = avg_w.max(line.width) * 0.75;
                (cx - col_cx).abs() <= tolerance
            });
            if let Some(col) = target_col {
                col.push(line);
            } else {
                columns.push(vec![line]);
            }
        }

        // 每列内部按 Y 坐标从上往下（升序）
        let mut result = Vec::new();
        for mut col in columns {
            col.sort_by(|a, b| a.top.partial_cmp(&b.top).unwrap_or(std::cmp::Ordering::Equal));
            result.extend(col);
        }
        result
    } else {
        // 横排：按中心 Y 坐标从上往下（升序）
        let mut sorted = lines;
        sorted.sort_by(|a, b| {
            let cy_a = a.top + a.height / 2.0;
            let cy_b = b.top + b.height / 2.0;
            cy_a.partial_cmp(&cy_b).unwrap_or(std::cmp::Ordering::Equal)
        });

        // 将 Y 坐标重叠相近的框聚合成同一横行
        let mut rows: Vec<Vec<PaddleLine>> = Vec::new();
        for line in sorted {
            let cy = line.top + line.height / 2.0;
            let target_row = rows.iter_mut().find(|row| {
                let row_cy: f64 =
                    row.iter().map(|it| it.top + it.height / 2.0).sum::<f64>() / row.len() as f64;
                let avg_h: f64 = col_avg_h(row);
                let tolerance = avg_h.max(line.height) * 0.6;
                (cy - row_cy).abs() <= tolerance
            });
            if let Some(row) = target_row {
                row.push(line);
            } else {
                rows.push(vec![line]);
            }
        }

        // 每行内部按 X 坐标从左向右（升序）
        let mut result = Vec::new();
        for mut row in rows {
            row.sort_by(|a, b| a.left.partial_cmp(&b.left).unwrap_or(std::cmp::Ordering::Equal));
            result.extend(row);
        }
        result
    }
}

fn col_avg_h(items: &[PaddleLine]) -> f64 {
    items.iter().map(|it| it.height).sum::<f64>() / items.len() as f64
}

/// 进程级引擎缓存：键为 "{mode}:{is_manga}"，模式变更时重建。
#[cfg(not(target_os = "macos"))]
static ENGINE: std::sync::Mutex<Option<(String, &'static ocr_rs::OcrEngine)>> =
    std::sync::Mutex::new(None);

/// 取当前模式的引擎：命中缓存直接返回；否则校验模型齐全后构建并缓存。
#[cfg(not(target_os = "macos"))]
fn ensure_engine(
    app: &tauri::AppHandle,
    mode: &str,
    is_manga: bool,
) -> Result<&'static ocr_rs::OcrEngine, String> {
    let cache_key = format!("{mode}:{is_manga}");
    let mut cache = ENGINE.lock().map_err(|error| error.to_string())?;
    if let Some((cached_key, engine)) = cache.as_ref() {
        if cached_key == &cache_key {
            return Ok(engine);
        }
    }

    let paths = paddle_model_paths(app, mode)?;
    if !paths.det.exists() || !paths.rec.exists() || !paths.charset.exists() {
        return Err("请先在偏好设置中下载图片 OCR 资源".to_string());
    }

    let mut config = ocr_rs::OcrEngineConfig::default();
    if is_manga {
        config.det_options.unclip_ratio = 1.32;
        config.det_options.box_threshold = 0.45;
        config.det_options.score_threshold = 0.25;
        config.min_result_confidence = 0.35;
    }

    let engine = ocr_rs::OcrEngine::new(&paths.det, &paths.rec, &paths.charset, Some(config))
        .map_err(|error| format!("初始化 PaddleOCR 引擎失败：{error}"))?;
    let engine: &'static ocr_rs::OcrEngine = Box::leak(Box::new(engine));
    *cache = Some((cache_key, engine));
    Ok(engine)
}

/// 命令入口（被 ocr/mod.rs::recognize_image 的非 macOS 分支调用，spawn_blocking 内执行）：
#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_image_text_paddle(
    app: &tauri::AppHandle,
    store: &crate::store::Store,
    image_path: String,
    profile: Option<String>,
) -> Result<crate::models::ImageOcrResult, String> {
    let mode = store.settings()?.ocr_mode;
    recognize_with_mode(app, &mode, image_path, profile)
}

/// 降级入口：AppState 取不到（无法读设置）时由调度方按默认模式调用。
#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_with_mode(
    app: &tauri::AppHandle,
    mode: &str,
    image_path: String,
    profile: Option<String>,
) -> Result<crate::models::ImageOcrResult, String> {
    let image_path = std::path::PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }
    let image = image::open(&image_path).map_err(|error| format!("无法读取图片：{error}"))?;

    let is_manga = profile
        .as_deref()
        .map(|p| p.eq_ignore_ascii_case("manga") || p.eq_ignore_ascii_case("japanese"))
        .unwrap_or(false);

    let engine = ensure_engine(app, mode, is_manga)?;
    let options = ocr_rs::RecognizeOptions::new()
        .with_rotated_text_mode(ocr_rs::RotatedTextMode::Robust)
        .with_vertical_aspect_ratio(if is_manga { 1.15 } else { 1.3 });
    let items = engine
        .recognize_with_options(&image, &options)
        .map_err(|error| format!("PaddleOCR 识别失败：{error}"))?;

    let raw_lines: Vec<PaddleLine> = items
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

    let lines = sort_paddle_lines(raw_lines, is_manga);
    let words = paddle_lines_to_words(&lines, image.width(), image.height());
    let text = lines
        .iter()
        .map(|line| line.text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    let language = if is_manga {
        "ja+zh+en".to_string()
    } else {
        "zh-Hans+en".to_string()
    };

    Ok(crate::models::ImageOcrResult {
        text,
        engine: OCR_ENGINE_ID.to_string(),
        language,
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

    fn vertical_line(text: &str) -> PaddleLine {
        PaddleLine {
            text: text.to_string(),
            confidence: 0.95,
            left: 20.0,
            top: 10.0,
            width: 15.0,
            height: 100.0,
        }
    }

    #[test]
    fn maps_lines_to_ordered_words_with_indices() {
        let words = paddle_lines_to_words(&[line("你好ab"), line("第二行")], 640, 480);
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

        let cjk = paddle_lines_to_words(&[line("你好好")], 640, 480);
        assert_eq!(cjk.len(), 3);
        let total: f64 = cjk.iter().map(|w| w.width).sum();
        assert!((total - 100.0).abs() < 1e-6);
        assert!(cjk[1].left >= cjk[0].left + cjk[0].width - 1e-6);
    }

    #[test]
    fn vertical_lines_partition_proportional_boxes_vertically() {
        let words = paddle_lines_to_words(&[vertical_line("白日依山尽")], 640, 480);
        assert_eq!(words.len(), 5);
        assert_eq!(words[0].text, "白");
        assert_eq!(words[4].text, "尽");
        assert!((words[0].left - 20.0).abs() < 1e-6);
        assert!((words[0].width - 15.0).abs() < 1e-6);
        assert!((words[0].top - 10.0).abs() < 1e-6);
        assert!((words[0].height - 20.0).abs() < 1e-6);
        assert!((words[1].top - 30.0).abs() < 1e-6);
        assert!((words[4].top - 90.0).abs() < 1e-6);

        let total_height: f64 = words.iter().map(|w| w.height).sum();
        assert!((total_height - 100.0).abs() < 1e-6);
    }

    #[test]
    fn sort_paddle_lines_vertical_right_to_left_order() {
        let col1 = PaddleLine {
            text: "第一列（右）".to_string(),
            confidence: 0.9,
            left: 300.0,
            top: 20.0,
            width: 20.0,
            height: 100.0,
        };
        let col2 = PaddleLine {
            text: "第二列（中）".to_string(),
            confidence: 0.9,
            left: 200.0,
            top: 25.0,
            width: 20.0,
            height: 100.0,
        };
        let col3 = PaddleLine {
            text: "第三列（左）".to_string(),
            confidence: 0.9,
            left: 100.0,
            top: 22.0,
            width: 20.0,
            height: 100.0,
        };
        // 输入逆序输入（从左到右）
        let sorted = sort_paddle_lines(vec![col3, col2, col1], false);
        assert_eq!(sorted[0].text, "第一列（右）");
        assert_eq!(sorted[1].text, "第二列（中）");
        assert_eq!(sorted[2].text, "第三列（左）");
    }

    #[test]
    fn empty_lines_yield_no_words() {
        assert!(paddle_lines_to_words(&[], 640, 480).is_empty());
        assert!(paddle_lines_to_words(&[line("   ")], 640, 480).is_empty());
    }
}
