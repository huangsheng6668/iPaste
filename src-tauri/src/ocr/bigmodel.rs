//! BigModel 云 OCR 客户端（POST https://open.bigmodel.cn/api/paas/v4/files/ocr）。
//! multipart 上传图片文件（tool_type 目前仅支持 hand_write），同步返回行级
//! words_result（location 整数像素框 + words 文本 + probability 置信度）。
//! 行级结果经 tokens::split_line_tokens 分词 + 比例切框，归一为与本地引擎
//! 相同的 ImageOcrResult 契约（镜像 paddle.rs 的行→词管线）。

use std::time::Duration;

use serde::Deserialize;

use crate::models::{ImageOcrResult, ImageOcrWord};
use crate::ocr::tokens::split_line_tokens;

pub(crate) const BIGMODEL_OCR_ENGINE_ID: &str = "bigmodel";
const BIGMODEL_OCR_ENDPOINT: &str = "https://open.bigmodel.cn/api/paas/v4/files/ocr";
const BIGMODEL_OCR_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct BigModelOcrResponse {
    #[serde(default)]
    message: String,
    status: String,
    #[serde(default)]
    words_result: Vec<BigModelOcrWordItem>,
}

#[derive(Debug, Deserialize)]
struct BigModelOcrWordItem {
    #[serde(default)]
    location: Option<BigModelOcrLocation>,
    words: String,
    #[serde(default)]
    probability: Option<BigModelOcrProbability>,
}

#[derive(Debug, Deserialize)]
struct BigModelOcrLocation {
    #[serde(default)]
    left: i64,
    #[serde(default)]
    top: i64,
    #[serde(default)]
    width: i64,
    #[serde(default)]
    height: i64,
}

#[derive(Debug, Deserialize)]
struct BigModelOcrProbability {
    #[serde(default)]
    average: f64,
}

#[derive(Debug, Deserialize)]
struct BigModelOcrErrorBody {
    error: BigModelOcrErrorDetail,
}

#[derive(Debug, Deserialize)]
struct BigModelOcrErrorDetail {
    #[serde(default)]
    code: String,
    #[serde(default)]
    message: String,
}

/// 行级中间结构（排序后即为阅读顺序）。
#[derive(Debug)]
struct BigModelLine {
    text: String,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    /// 0–1，映射时 ×100 对齐本地引擎的 0–100 约定
    confidence: f64,
}

/// 云识别入口：读文件 → 上传 → 归一化。api_key 为空时给出引导到设置页的明确错误。
pub(crate) fn recognize_image_bigmodel(
    image_path: &str,
    language_code: Option<&str>,
    api_key: &str,
) -> Result<ImageOcrResult, String> {
    if api_key.trim().is_empty() {
        return Err("尚未配置 BigModel API Key，请在 设置 → 图片 OCR 中保存".to_string());
    }
    let bytes = std::fs::read(image_path).map_err(|error| format!("读取图片失败：{error}"))?;
    let response = send_request(&bytes, image_mime_from_path(image_path), language_code, api_key)?;
    build_result(response, language_code)
}

/// 设置页「测试」按钮：上传一张空白小图验证 Key 与连通性（只关心请求是否成功，
/// 不关心识别内容）。
pub(crate) fn test_connection(api_key: &str) -> Result<(), String> {
    let mut png = Vec::new();
    let blank = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 255, 255, 255]));
    image::DynamicImage::ImageRgba8(blank)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| format!("生成测试图片失败：{error}"))?;
    let response = send_request(&png, "image/png", None, api_key)?;
    if response.status == "succeeded" {
        Ok(())
    } else {
        Err(failure_message(&response.message, &response.status))
    }
}

fn send_request(
    image_bytes: &[u8],
    mime: &str,
    language_code: Option<&str>,
    api_key: &str,
) -> Result<BigModelOcrResponse, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(BIGMODEL_OCR_TIMEOUT)
        .build()
        .map_err(|error| format!("BigModel OCR 客户端构建失败：{error}"))?;

    let file_part = reqwest::blocking::multipart::Part::bytes(image_bytes.to_vec())
        .file_name("image")
        .mime_str(mime)
        .map_err(|error| format!("BigModel OCR 请求构造失败：{error}"))?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .text("tool_type", "hand_write")
        .text("probability", "true")
        .part("file", file_part);
    if let Some(code) = language_code {
        form = form.text("language_type", code.to_string());
    }

    let response = client
        .post(BIGMODEL_OCR_ENDPOINT)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|error| format!("BigModel OCR 请求失败：{error}"))?;

    let http_status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("BigModel OCR 响应读取失败：{error}"))?;
    if !http_status.is_success() {
        return Err(error_message_from_body(http_status.as_u16(), &body));
    }
    serde_json::from_str::<BigModelOcrResponse>(&body)
        .map_err(|error| format!("BigModel OCR 响应解析失败：{error}"))
}

/// HTTP 错误体 `{error:{code,message}}` → 用户可读错误（Key 无效/额度不足等）。
fn error_message_from_body(http_status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<BigModelOcrErrorBody>(body).ok();
    let message = detail
        .as_ref()
        .map(|body| {
            let error = &body.error;
            if error.message.is_empty() {
                error.code.clone()
            } else if error.code.is_empty() {
                error.message.clone()
            } else {
                format!("{}（{}）", error.message, error.code)
            }
        })
        .unwrap_or_default();
    if message.is_empty() {
        format!("BigModel OCR 失败（HTTP {http_status}）")
    } else {
        format!("BigModel OCR 失败：{message}")
    }
}

fn failure_message(message: &str, status: &str) -> String {
    if message.is_empty() {
        format!("BigModel OCR 识别失败（{status}）")
    } else {
        format!("BigModel OCR 识别失败：{message}")
    }
}

/// 响应 → ImageOcrResult：行按（中心 top, left）排序为阅读顺序，行内比例切框。
fn build_result(
    response: BigModelOcrResponse,
    language_code: Option<&str>,
) -> Result<ImageOcrResult, String> {
    if response.status != "succeeded" {
        return Err(failure_message(&response.message, &response.status));
    }

    let mut lines: Vec<BigModelLine> = response
        .words_result
        .into_iter()
        .filter(|item| !item.words.trim().is_empty())
        .map(|item| {
            let location = item.location.unwrap_or(BigModelOcrLocation {
                left: 0,
                top: 0,
                width: 0,
                height: 0,
            });
            BigModelLine {
                text: item.words.trim().to_string(),
                left: location.left.max(0) as f64,
                top: location.top.max(0) as f64,
                width: location.width.max(0) as f64,
                height: location.height.max(0) as f64,
                confidence: item.probability.map(|p| p.average).unwrap_or(1.0),
            }
        })
        .collect();

    // API 不保证返回顺序，按几何位置排成阅读顺序（横排假设，与截图场景一致）
    lines.sort_by(|a, b| {
        let center_a = a.top + a.height / 2.0;
        let center_b = b.top + b.height / 2.0;
        center_a
            .partial_cmp(&center_b)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.left
                    .partial_cmp(&b.left)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });

    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(ImageOcrResult {
        text,
        engine: BIGMODEL_OCR_ENGINE_ID.to_string(),
        language: language_code.unwrap_or("auto").to_string(),
        words: build_words(&lines),
    })
}

/// 行 → 词：镜像 paddle.rs 的比例切分（竖排按高度均摊、横排按宽度均摊）。
/// BigModel 只给行级框，token 框为合成值，仅供前端选择高亮使用。
fn build_words(lines: &[BigModelLine]) -> Vec<ImageOcrWord> {
    let mut words = Vec::new();
    for (line_index, line) in lines.iter().enumerate() {
        let tokens = split_line_tokens(&line.text);
        let char_count = line.text.chars().count() as f64;
        if tokens.is_empty() || char_count <= 0.0 {
            continue;
        }

        let is_vertical = line.height > line.width;
        let unit = if is_vertical {
            line.height / char_count
        } else {
            line.width / char_count
        };

        for (word_index, token) in tokens.iter().enumerate() {
            let (left, top) = if is_vertical {
                (line.left, line.top + token.char_start as f64 * unit)
            } else {
                (line.left + token.char_start as f64 * unit, line.top)
            };
            let (width, height) = if is_vertical {
                (line.width, token.char_len as f64 * unit)
            } else {
                (token.char_len as f64 * unit, line.height)
            };
            words.push(ImageOcrWord {
                text: token.text.clone(),
                left,
                top,
                width: width.max(1.0),
                height: height.max(1.0),
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

fn image_mime_from_path(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else {
        "image/png"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word_item(words: &str, left: i64, top: i64, width: i64, height: i64, average: f64) -> BigModelOcrWordItem {
        BigModelOcrWordItem {
            location: Some(BigModelOcrLocation { left, top, width, height }),
            words: words.to_string(),
            probability: Some(BigModelOcrProbability { average }),
        }
    }

    #[test]
    fn build_result_sorts_lines_into_reading_order() {
        let response = BigModelOcrResponse {
            message: "成功".to_string(),
            status: "succeeded".to_string(),
            words_result: vec![
                word_item("second line", 0, 200, 400, 20, 0.9),
                word_item("first line", 10, 100, 380, 20, 0.8),
            ],
        };
        let result = build_result(response, Some("CHN_ENG")).unwrap();
        assert_eq!(result.text, "first line\nsecond line");
        assert_eq!(result.engine, BIGMODEL_OCR_ENGINE_ID);
        assert_eq!(result.language, "CHN_ENG");
        // first line 两个 token 都在 line 0；second line 在 line 1
        assert!(result.words.iter().all(|w| w.line_index == 0 || w.line_index == 1));
        assert_eq!(result.words[0].text, "first");
        assert_eq!(result.words[0].line_index, 0);
        assert_eq!(result.words[2].text, "second");
        assert_eq!(result.words[2].line_index, 1);
    }

    #[test]
    fn build_result_splits_words_proportionally() {
        let response = BigModelOcrResponse {
            message: "成功".to_string(),
            status: "succeeded".to_string(),
            words_result: vec![word_item("ab cd", 100, 50, 200, 40, 0.5)],
        };
        let result = build_result(response, None).unwrap();
        // 5 个 char（含空格）均摊 200px → 每 char 40px；token "ab" 占 [100,180)
        assert_eq!(result.words.len(), 2);
        assert_eq!(result.words[0].text, "ab");
        assert!((result.words[0].left - 100.0).abs() < 1e-9);
        assert!((result.words[0].width - 80.0).abs() < 1e-9);
        assert!((result.words[1].left - (100.0 + 3.0 * 40.0)).abs() < 1e-9, "cd 从第 3 个 char 开始");
        // 置信度 0.5 → 50（0–100 约定）
        assert!((result.words[0].confidence - 50.0).abs() < 1e-9);
        assert_eq!(result.language, "auto", "未传语言时回填 auto");
    }

    #[test]
    fn build_result_vertical_line_distributes_by_height() {
        let response = BigModelOcrResponse {
            message: "成功".to_string(),
            status: "succeeded".to_string(),
            // 高 > 宽：竖排，两个 CJK 字按高度均摊
            words_result: vec![word_item("你好", 10, 20, 30, 100, 1.0)],
        };
        let result = build_result(response, Some("CHN_ENG")).unwrap();
        assert_eq!(result.words.len(), 2);
        assert!((result.words[1].top - 70.0).abs() < 1e-9, "第二个字 top = 20 + 50");
        assert!((result.words[1].height - 50.0).abs() < 1e-9);
    }

    #[test]
    fn build_result_failed_status_is_error_with_message() {
        let response = BigModelOcrResponse {
            message: "图片尺寸超限".to_string(),
            status: "failed".to_string(),
            words_result: vec![],
        };
        let error = build_result(response, None).unwrap_err();
        assert!(error.contains("图片尺寸超限"), "got: {error}");
    }

    #[test]
    fn build_result_skips_blank_words_and_empty_result_is_ok() {
        let response = BigModelOcrResponse {
            message: "成功".to_string(),
            status: "succeeded".to_string(),
            words_result: vec![word_item("   ", 0, 0, 10, 10, 0.9)],
        };
        let result = build_result(response, None).unwrap();
        assert_eq!(result.text, "");
        assert!(result.words.is_empty(), "空白行不产生词");
    }

    #[test]
    fn error_message_from_body_formats_code_and_message() {
        let body = r#"{"error":{"code":"401","message":"令牌无效"}}"#;
        let error = error_message_from_body(401, body);
        assert!(error.contains("令牌无效") && error.contains("401"), "got: {error}");
    }

    #[test]
    fn error_message_from_body_falls_back_to_http_status() {
        let error = error_message_from_body(500, "not json");
        assert!(error.contains("500"), "got: {error}");
    }

    #[test]
    fn missing_api_key_is_guided_error() {
        let error = recognize_image_bigmodel("whatever.png", None, "  ").unwrap_err();
        assert!(error.contains("BigModel API Key"), "got: {error}");
    }
}
