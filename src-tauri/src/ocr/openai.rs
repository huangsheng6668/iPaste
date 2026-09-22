//! 通用 OpenAI 兼容云 OCR：POST {base}/chat/completions，视觉模型识图取文。
//! 任意兼容厂商可接（智谱 GLM-4V、OpenAI GPT-4o、Qwen-VL、本地 vLLM/Ollama 等），
//! 由设置提供 Base URL + 模型名 + API Key。
//! 响应为纯文本（无词级坐标），行为对齐 mocr：每个非空行一个零尺寸词——
//! 前端 useImageOcr 会过滤零尺寸词并回退展示整段 text。

use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::models::{ImageOcrResult, ImageOcrWord};

pub(crate) const OPENAI_OCR_ENGINE_ID: &str = "openai";
const OPENAI_OCR_TIMEOUT: Duration = Duration::from_secs(60);
/// 视觉模型通常对长图输出有限额，4096 是各主流厂商都接受的安全默认。
const OPENAI_OCR_MAX_TOKENS: u32 = 4096;

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ChatMessage>,
}

#[derive(Serialize)]
struct ChatMessage {
    role: &'static str,
    content: Vec<ContentPart>,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageOut,
}

#[derive(Deserialize)]
struct ChatMessageOut {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct OpenAiErrorBody {
    error: OpenAiErrorDetail,
}

#[derive(Deserialize)]
struct OpenAiErrorDetail {
    #[serde(default)]
    message: String,
}

/// 基地址 → 完整端点：容忍结尾斜杠与已带 /chat/completions 的输入。
fn chat_completions_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.ends_with("/chat/completions") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/chat/completions")
    }
}

/// 识别语言标识 → 本地化显示名（用于替换 Prompt 中的 {recognition_language} 变量）。
pub(crate) fn localized_language_name(language: Option<&str>) -> &'static str {
    match language {
        Some("zh-Hans") => "简体中文",
        Some("zh-Hant") => "繁体中文",
        Some("en") => "英语",
        Some("ja") => "日语",
        Some("zh-Hans+en") => "中英混合",
        Some("ja+zh+en") => "中日英文",
        _ => "自动检测",
    }
}

/// 云识别入口：读文件 → base64 内联 → chat/completions → 纯文本结果。
pub(crate) fn recognize_image_openai(
    image_path: &str,
    language: Option<&str>,
    base_url: &str,
    model: &str,
    api_key: &str,
    prompts: &[crate::models::CloudOcrPromptMessage],
) -> Result<ImageOcrResult, String> {
    if base_url.trim().is_empty() || model.trim().is_empty() || api_key.trim().is_empty() {
        return Err(
            "OpenAI 兼容接口未配置完整（需要接口地址、模型与 API Key），请在 设置 → 图片 OCR 中保存"
                .to_string(),
        );
    }
    let text = transcribe_image(image_path, language, base_url, model, api_key, prompts)?;
    Ok(build_result(&text, language))
}

/// 设置页「测试」按钮：上传一张空白小图，HTTP 200 且能解析即视为配置有效
/// （空白图无文字，不校验转写内容）。
pub(crate) fn test_connection(
    base_url: &str,
    model: &str,
    api_key: &str,
    prompts: Option<&[crate::models::CloudOcrPromptMessage]>,
) -> Result<(), String> {
    let mut png = Vec::new();
    let blank = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 255, 255, 255]));
    image::DynamicImage::ImageRgba8(blank)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|error| format!("生成测试图片失败：{error}"))?;
    let effective_prompts = prompts.unwrap_or(&[]);
    let request = build_request(&png, "image/png", None, model, effective_prompts);
    let content = send_request(&chat_completions_url(base_url), api_key, &request)?;
    let _ = content;
    Ok(())
}

fn transcribe_image(
    image_path: &str,
    language: Option<&str>,
    base_url: &str,
    model: &str,
    api_key: &str,
    prompts: &[crate::models::CloudOcrPromptMessage],
) -> Result<String, String> {
    let bytes = std::fs::read(image_path).map_err(|error| format!("读取图片失败：{error}"))?;
    let request = build_request(&bytes, image_mime_from_path(image_path), language, model, prompts);
    let content = send_request(&chat_completions_url(base_url), api_key, &request)?;
    if content.trim().is_empty() {
        Ok(String::new())
    } else {
        Ok(content.trim().to_string())
    }
}

fn build_request(
    image_bytes: &[u8],
    mime: &str,
    language: Option<&str>,
    model: &str,
    prompts: &[crate::models::CloudOcrPromptMessage],
) -> ChatRequest {
    let lang_name = localized_language_name(language);
    let effective_prompts: Vec<crate::models::CloudOcrPromptMessage> = if prompts.is_empty() {
        crate::models::default_openai_ocr_prompts()
    } else {
        prompts.to_vec()
    };
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(image_bytes)
    );

    let last_user_index = effective_prompts.iter().rposition(|m| m.role == "user");
    let mut messages = Vec::new();

    for (idx, msg) in effective_prompts.iter().enumerate() {
        let text = msg.content.replace("{recognition_language}", lang_name);
        if Some(idx) == last_user_index {
            messages.push(ChatMessage {
                role: "user",
                content: vec![
                    ContentPart::Text { text },
                    ContentPart::ImageUrl {
                        image_url: ImageUrl {
                            url: data_url.clone(),
                        },
                    },
                ],
            });
        } else {
            messages.push(ChatMessage {
                role: if msg.role == "system" { "system" } else { "user" },
                content: vec![ContentPart::Text { text }],
            });
        }
    }

    if last_user_index.is_none() {
        messages.push(ChatMessage {
            role: "user",
            content: vec![
                ContentPart::Text {
                    text: format!("Recognize all text in this image. Recognition language: {lang_name}."),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl { url: data_url },
                },
            ],
        });
    }

    ChatRequest {
        model: model.to_string(),
        max_tokens: OPENAI_OCR_MAX_TOKENS,
        messages,
    }
}

fn send_request(
    url: &str,
    api_key: &str,
    request: &ChatRequest,
) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(OPENAI_OCR_TIMEOUT)
        .build()
        .map_err(|error| format!("OpenAI 兼容接口客户端构建失败：{error}"))?;

    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .map_err(|error| format!("OpenAI 兼容接口请求失败：{error}"))?;

    let http_status = response.status();
    let body = response
        .text()
        .map_err(|error| format!("OpenAI 兼容接口响应读取失败：{error}"))?;
    if !http_status.is_success() {
        return Err(error_message_from_body(http_status.as_u16(), &body));
    }
    let parsed: ChatResponse = serde_json::from_str(&body)
        .map_err(|error| format!("OpenAI 兼容接口响应解析失败：{error}"))?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| "OpenAI 兼容接口响应缺少 choices".to_string())
}

/// HTTP 错误体 `{error:{message}}` → 用户可读错误（Key 无效/模型不存在/额度不足等）。
fn error_message_from_body(http_status: u16, body: &str) -> String {
    let message = serde_json::from_str::<OpenAiErrorBody>(body)
        .ok()
        .map(|body| body.error.message)
        .filter(|message| !message.is_empty())
        .unwrap_or_default();
    if message.is_empty() {
        format!("OpenAI 兼容接口调用失败（HTTP {http_status}）")
    } else {
        format!("OpenAI 兼容接口调用失败：{message}")
    }
}

fn build_result(text: &str, language: Option<&str>) -> ImageOcrResult {
    let words = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .enumerate()
        .map(|(line_index, line)| ImageOcrWord {
            text: line.trim().to_string(),
            left: 0.0,
            top: 0.0,
            width: 0.0,
            height: 0.0,
            confidence: 99.0,
            block_index: 0,
            paragraph_index: 0,
            line_index: line_index as i64,
            word_index: 0,
        })
        .collect();

    ImageOcrResult {
        text: text.trim().to_string(),
        engine: OPENAI_OCR_ENGINE_ID.to_string(),
        language: language.unwrap_or("auto").to_string(),
        words,
    }
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

    #[test]
    fn chat_completions_url_appends_endpoint() {
        assert_eq!(
            chat_completions_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            chat_completions_url("https://open.bigmodel.cn/api/paas/v4/"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_passes_through_full_endpoint() {
        assert_eq!(
            chat_completions_url("http://localhost:11434/v1/chat/completions/"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn build_result_makes_zero_geometry_word_per_line() {
        let result = build_result("  第一行\n\nsecond line  \n", Some("ja"));
        assert_eq!(result.engine, OPENAI_OCR_ENGINE_ID);
        assert_eq!(result.language, "ja");
        assert_eq!(result.text, "第一行\n\nsecond line");
        assert_eq!(result.words.len(), 2, "空行不产生词");
        assert_eq!(result.words[0].text, "第一行");
        assert_eq!(result.words[0].line_index, 0);
        assert_eq!(result.words[1].text, "second line");
        assert_eq!(result.words[1].line_index, 1);
        assert!(result.words.iter().all(|w| w.width == 0.0 && w.height == 0.0));
        assert_eq!(build_result("   ", None).language, "auto", "未传语言回填 auto");
    }

    #[test]
    fn localized_language_name_maps_supported_ids() {
        assert_eq!(localized_language_name(Some("zh-Hans")), "简体中文");
        assert_eq!(localized_language_name(Some("zh-Hant")), "繁体中文");
        assert_eq!(localized_language_name(Some("en")), "英语");
        assert_eq!(localized_language_name(Some("ja")), "日语");
        assert_eq!(localized_language_name(Some("zh-Hans+en")), "中英混合");
        assert_eq!(localized_language_name(Some("auto")), "自动检测");
        assert_eq!(localized_language_name(None), "自动检测");
    }

    #[test]
    fn error_message_from_body_extracts_message() {
        let body = r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let error = error_message_from_body(401, body);
        assert!(error.contains("Incorrect API key"), "got: {error}");
    }

    #[test]
    fn error_message_from_body_falls_back_to_http_status() {
        let error = error_message_from_body(503, "gateway text");
        assert!(error.contains("503"), "got: {error}");
    }

    #[test]
    fn missing_config_is_guided_error() {
        let error = recognize_image_openai("x.png", None, " ", "model", "", &[]).unwrap_err();
        assert!(error.contains("OpenAI 兼容接口"), "got: {error}");
    }

    #[test]
    fn build_request_interpolates_language_and_attaches_image_to_last_user() {
        let prompts = vec![
            crate::models::CloudOcrPromptMessage {
                role: "system".to_string(),
                content: "Sys: target {recognition_language}.".to_string(),
            },
            crate::models::CloudOcrPromptMessage {
                role: "user".to_string(),
                content: "First user msg for {recognition_language}.".to_string(),
            },
            crate::models::CloudOcrPromptMessage {
                role: "user".to_string(),
                content: "Second user msg.".to_string(),
            },
        ];
        let request = build_request(b"png-bytes", "image/png", Some("zh-Hans"), "glm-4v-flash", &prompts);
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "glm-4v-flash");
        assert_eq!(json["max_tokens"], 4096);
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 3);

        // System message has interpolated language and only text
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"][0]["text"], "Sys: target 简体中文.");
        assert_eq!(messages[0]["content"].as_array().unwrap().len(), 1);

        // First user message has interpolated language and only text
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"][0]["text"], "First user msg for 简体中文.");
        assert_eq!(messages[1]["content"].as_array().unwrap().len(), 1);

        // Second (last) user message has text AND image_url
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["text"], "Second user msg.");
        assert_eq!(messages[2]["content"][1]["type"], "image_url");
        assert!(messages[2]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
    }

    #[test]
    fn build_request_synthesizes_user_message_when_no_user_role() {
        let prompts = vec![crate::models::CloudOcrPromptMessage {
            role: "system".to_string(),
            content: "You are an OCR engine for {recognition_language}.".to_string(),
        }];
        let request = build_request(b"test", "image/png", Some("ja"), "gpt-4o", &prompts);
        let json = serde_json::to_value(&request).unwrap();
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "系统提示 + 自动追加的 user 图像消息");
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[0]["content"][0]["text"], "You are an OCR engine for 日语.");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"][1]["type"], "image_url");
    }

    #[test]
    fn build_request_serializes_default_prompts_when_empty() {
        let request = build_request(b"png-bytes", "image/png", Some("en"), "glm-4v-flash", &[]);
        let json = serde_json::to_value(&request).unwrap();
        let messages = json["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2, "默认 system + user 消息");
        assert_eq!(messages[0]["role"], "system");
        assert!(messages[0]["content"][0]["text"].as_str().unwrap().contains("英语"));
        assert_eq!(messages[1]["role"], "user");
        assert!(messages[1]["content"][0]["text"].as_str().unwrap().contains("英语"));
        assert_eq!(messages[1]["content"][1]["type"], "image_url");
    }
}
