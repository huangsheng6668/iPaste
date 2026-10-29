//! 通用 OpenAI 兼容云 OCR：POST {base}/chat/completions，视觉模型识图取文。
//! 任意兼容厂商可接（智谱 GLM-4V、OpenAI GPT-4o、Qwen-VL、本地 vLLM/Ollama 等）。
//! 支持多个 Base URL + 模型名 + API Key 配置：主接口优先，仅 HTTP 429 时按配置
//! 顺序切换备用接口；其他错误立即返回，避免无意义重复请求与计费。
//! 响应为纯文本（无词级坐标），行为对齐 mocr：每个非空行一个零尺寸词——
//! 前端 useImageOcr 会过滤零尺寸词并回退展示整段 text。

use std::time::Duration;

use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::models::{ImageOcrResult, ImageOcrWord, OpenaiOcrProfile};

pub(crate) const OPENAI_OCR_ENGINE_ID: &str = "openai";
const OPENAI_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const OPENAI_OCR_TIMEOUT: Duration = Duration::from_secs(60);
/// 视觉模型通常对长图输出有限额，4096 是各主流厂商都接受的安全默认。
const OPENAI_OCR_MAX_TOKENS: u32 = 4096;
/// 云端 OCR 自适应压缩阈值：长边不超过 2048px 且体积不超过 512KB 的小图原样直传。
pub(crate) const OPENAI_IMAGE_MAX_DIMENSION: u32 = 2048;
pub(crate) const OPENAI_IMAGE_RAW_MAX_BYTES: usize = 512 * 1024;
pub(crate) const OPENAI_JPEG_QUALITY: u8 = 85;

/// HTTP 层错误保留状态码，供上层仅对 429 做接口切换。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OpenaiRequestError {
    pub(crate) status: Option<u16>,
    pub(crate) message: String,
}

impl OpenaiRequestError {
    fn new(status: Option<u16>, message: String) -> Self {
        Self { status, message }
    }

    #[cfg(test)]
    fn http(status: u16, message: &str) -> Self {
        Self::new(Some(status), message.to_string())
    }
}

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

/// API Key 不是必需项：本地 vLLM/Ollama 等无鉴权端点允许留空。
fn profile_is_complete(profile: &OpenaiOcrProfile) -> bool {
    !profile.base_url.trim().is_empty() && !profile.model.trim().is_empty()
}

fn profile_label(profile: &OpenaiOcrProfile) -> &str {
    if profile.name.trim().is_empty() {
        &profile.model
    } else {
        &profile.name
    }
}

fn run_profile_failover<T>(
    profiles: &[OpenaiOcrProfile],
    mut attempt: impl FnMut(&OpenaiOcrProfile) -> Result<T, OpenaiRequestError>,
) -> Result<T, String> {
    let mut last_rate_limit = None;
    for profile in profiles {
        match attempt(profile) {
            Ok(value) => return Ok(value),
            Err(error) if error.status == Some(429) => {
                last_rate_limit = Some(error.message);
            }
            Err(error) => {
                return Err(format!(
                    "接口「{}」{}",
                    profile_label(profile),
                    error.message
                ));
            }
        }
    }

    match last_rate_limit {
        Some(message) => Err(format!(
            "所有可用的 OpenAI 兼容接口均触发限流（HTTP 429），请稍后重试或补充接口。最后一次响应：{message}"
        )),
        None => Err(
            "OpenAI 兼容接口未配置完整（需要接口地址与模型；本地无鉴权端点可留空 API Key），请在 设置 → 图片 OCR 中保存"
                .to_string(),
        ),
    }
}

/// 多接口识别：主接口由调用方排在首位，后续按设置中的顺序尝试。
/// 只有 429 才切下一个；其他错误保留接口名并立即返回，避免重复计费。
pub(crate) fn recognize_image_openai_profiles(
    image_path: &str,
    language: Option<&str>,
    profiles: &[OpenaiOcrProfile],
    prompts: &[crate::models::CloudOcrPromptMessage],
) -> Result<ImageOcrResult, String> {
    let usable: Vec<OpenaiOcrProfile> = profiles
        .iter()
        .filter(|profile| profile_is_complete(profile))
        .cloned()
        .collect();
    if usable.is_empty() {
        return Err(
            "OpenAI 兼容接口未配置完整（需要接口地址与模型；本地无鉴权端点可留空 API Key），请在 设置 → 图片 OCR 中保存"
                .to_string(),
        );
    }

    let bytes = std::fs::read(image_path).map_err(|error| format!("读取图片失败：{error}"))?;
    let mime = image_mime_from_path(image_path);
    let data_url = prepare_image_data_url_from_bytes(&bytes, mime);
    let text = run_profile_failover(&usable, |profile| {
        let request = build_request_with_data_url(&data_url, language, &profile.model, prompts);
        send_request(
            &chat_completions_url(&profile.base_url),
            &profile.api_key,
            &request,
        )
    })?;
    Ok(build_result(&text, language))
}

fn composite_rgba_on_white(rgba: &image::RgbaImage) -> image::RgbImage {
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (src, dst) in rgba.pixels().zip(rgb.pixels_mut()) {
        let a = src[3] as u16;
        if a == 255 {
            *dst = image::Rgb([src[0], src[1], src[2]]);
        } else if a == 0 {
            *dst = image::Rgb([255, 255, 255]);
        } else {
            let inv_a = 255 - a;
            let r = ((src[0] as u16 * a + 255 * inv_a) / 255) as u8;
            let g = ((src[1] as u16 * a + 255 * inv_a) / 255) as u8;
            let b = ((src[2] as u16 * a + 255 * inv_a) / 255) as u8;
            *dst = image::Rgb([r, g, b]);
        }
    }
    rgb
}

/// 自适应图片预处理：
/// - 长边 <= 2048 且字节数 <= 512KB：零损耗直接 Base64；
/// - 超阈值：等比缩放至长边 <= 2048、透明通道合成白底并编码为 85% JPEG；
/// - 任何解码或编码异常均安全回退为原始字节 Base64。
pub(crate) fn prepare_image_data_url_from_bytes(bytes: &[u8], fallback_mime: &str) -> String {
    let raw_data_url = || {
        format!(
            "data:{fallback_mime};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    };

    let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());

    if let Some((width, height)) = dimensions {
        if width.max(height) <= OPENAI_IMAGE_MAX_DIMENSION
            && bytes.len() <= OPENAI_IMAGE_RAW_MAX_BYTES
        {
            return raw_data_url();
        }
    }

    let Ok(decoded) = image::load_from_memory(bytes) else {
        return raw_data_url();
    };
    let resized = if decoded.width().max(decoded.height()) > OPENAI_IMAGE_MAX_DIMENSION {
        decoded.resize(
            OPENAI_IMAGE_MAX_DIMENSION,
            OPENAI_IMAGE_MAX_DIMENSION,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        decoded
    };
    let rgb = composite_rgba_on_white(&resized.to_rgba8());
    let mut jpeg_buf = Vec::new();
    let mut encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg_buf, OPENAI_JPEG_QUALITY);
    if encoder
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .is_err()
    {
        return raw_data_url();
    }

    format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&jpeg_buf)
    )
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
    let content = send_request(&chat_completions_url(base_url), api_key, &request)
        .map_err(|error| error.message)?;
    let _ = content;
    Ok(())
}

fn build_request(
    image_bytes: &[u8],
    mime: &str,
    language: Option<&str>,
    model: &str,
    prompts: &[crate::models::CloudOcrPromptMessage],
) -> ChatRequest {
    let data_url = prepare_image_data_url_from_bytes(image_bytes, mime);
    build_request_with_data_url(&data_url, language, model, prompts)
}

fn build_request_with_data_url(
    data_url: &str,
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

    let last_user_index = effective_prompts.iter().rposition(|m| m.role == "user");
    let mut messages = Vec::new();

    for msg in effective_prompts.iter() {
        let text = msg.content.replace("{recognition_language}", lang_name);
        messages.push(ChatMessage {
            role: if msg.role == "system" {
                "system"
            } else {
                "user"
            },
            content: vec![ContentPart::Text { text }],
        });
    }
    // 图片 data url 在循环外一次性填充进最后一条 user 消息。
    match last_user_index {
        Some(idx) => {
            if let Some(message) = messages.get_mut(idx) {
                message.content.push(ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: data_url.to_string(),
                    },
                });
            }
        }
        None => messages.push(ChatMessage {
            role: "user",
            content: vec![
                ContentPart::Text {
                    text: format!(
                        "Recognize all text in this image. Recognition language: {lang_name}."
                    ),
                },
                ContentPart::ImageUrl {
                    image_url: ImageUrl {
                        url: data_url.to_string(),
                    },
                },
            ],
        }),
    }

    ChatRequest {
        model: model.to_string(),
        max_tokens: OPENAI_OCR_MAX_TOKENS,
        messages,
    }
}

fn shared_http_client() -> Result<&'static reqwest::blocking::Client, OpenaiRequestError> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::blocking::Client, String>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .connect_timeout(OPENAI_CONNECT_TIMEOUT)
                .timeout(OPENAI_OCR_TIMEOUT)
                .build()
                .map_err(|error| format!("OpenAI 兼容接口客户端构建失败：{error}"))
        })
        .as_ref()
        .map_err(|msg| OpenaiRequestError::new(None, msg.clone()))
}

fn send_request(
    url: &str,
    api_key: &str,
    request: &ChatRequest,
) -> Result<String, OpenaiRequestError> {
    let client = shared_http_client()?;

    let mut builder = client.post(url);
    // 空 Key 用于本地无鉴权端点：不发 Authorization 头，避免服务端误判。
    if !api_key.trim().is_empty() {
        builder = builder.bearer_auth(api_key);
    }
    let response = builder.json(request).send().map_err(|error| {
        OpenaiRequestError::new(None, format!("OpenAI 兼容接口请求失败：{error}"))
    })?;

    let http_status = response.status();
    let body = response.text().map_err(|error| {
        OpenaiRequestError::new(None, format!("OpenAI 兼容接口响应读取失败：{error}"))
    })?;
    if !http_status.is_success() {
        return Err(error_message_from_body(http_status.as_u16(), &body));
    }
    let parsed: ChatResponse = serde_json::from_str(&body).map_err(|error| {
        OpenaiRequestError::new(None, format!("OpenAI 兼容接口响应解析失败：{error}"))
    })?;
    parsed
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| OpenaiRequestError::new(None, "OpenAI 兼容接口响应缺少 choices".to_string()))
}

/// HTTP 错误体 `{error:{message}}` → 用户可读错误（Key 无效/模型不存在/额度不足等）。
fn error_message_from_body(http_status: u16, body: &str) -> OpenaiRequestError {
    let message = serde_json::from_str::<OpenAiErrorBody>(body)
        .ok()
        .map(|body| body.error.message)
        .filter(|message| !message.is_empty())
        .unwrap_or_default();
    let message = if message.is_empty() {
        format!("OpenAI 兼容接口调用失败（HTTP {http_status}）")
    } else {
        format!("OpenAI 兼容接口调用失败：{message}")
    };
    OpenaiRequestError::new(Some(http_status), message)
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
        assert!(result
            .words
            .iter()
            .all(|w| w.width == 0.0 && w.height == 0.0));
        assert_eq!(
            build_result("   ", None).language,
            "auto",
            "未传语言回填 auto"
        );
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
        let body =
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let error = error_message_from_body(401, body);
        assert_eq!(error.status, Some(401));
        assert!(
            error.message.contains("Incorrect API key"),
            "got: {}",
            error.message
        );
    }

    #[test]
    fn error_message_from_body_falls_back_to_http_status() {
        let error = error_message_from_body(503, "gateway text");
        assert_eq!(error.status, Some(503));
        assert!(error.message.contains("503"), "got: {}", error.message);
    }

    #[test]
    fn error_message_from_body_marks_429_rate_limit() {
        let error = error_message_from_body(429, r#"{"error":{"message":"quota exceeded"}}"#);
        assert_eq!(error.status, Some(429));
        assert!(error.message.contains("quota exceeded"));
    }

    #[test]
    fn missing_config_is_guided_error() {
        let mut incomplete = profile("missing");
        incomplete.base_url.clear();
        let error = recognize_image_openai_profiles("x.png", None, &[incomplete], &[]).unwrap_err();
        assert!(error.contains("OpenAI 兼容接口"), "got: {error}");
    }

    #[test]
    fn profile_without_api_key_is_complete_for_local_endpoints() {
        let mut local = profile("local");
        local.base_url = "http://localhost:11434/v1".to_string();
        local.api_key.clear();
        assert!(profile_is_complete(&local));
    }

    fn profile(id: &str) -> crate::models::OpenaiOcrProfile {
        crate::models::OpenaiOcrProfile {
            id: id.to_string(),
            name: format!("接口 {id}"),
            base_url: format!("https://{id}.example.com/v1"),
            model: format!("model-{id}"),
            api_key: format!("key-{id}"),
        }
    }

    #[test]
    fn profile_failover_switches_only_after_429() {
        let profiles = vec![profile("a"), profile("b")];
        let mut attempted = Vec::new();
        let result = run_profile_failover(&profiles, |candidate| {
            attempted.push(candidate.id.clone());
            if candidate.id == "a" {
                Err(OpenaiRequestError::http(429, "quota exceeded"))
            } else {
                Ok("recognized")
            }
        })
        .unwrap();
        assert_eq!(attempted, vec!["a", "b"]);
        assert_eq!(result, "recognized");
    }

    #[test]
    fn profile_failover_does_not_switch_on_non_429() {
        let profiles = vec![profile("a"), profile("b")];
        let mut attempted = Vec::new();
        let error = run_profile_failover(&profiles, |candidate| {
            attempted.push(candidate.id.clone());
            Err::<(), _>(OpenaiRequestError::http(401, "invalid key"))
        })
        .unwrap_err();
        assert_eq!(attempted, vec!["a"], "鉴权错误不应无意义地打其他接口");
        assert!(error.contains("接口 a"), "got: {error}");
        assert!(error.contains("invalid key"), "got: {error}");
    }

    #[test]
    fn profile_failover_reports_rate_limit_after_all_candidates() {
        let profiles = vec![profile("a"), profile("b")];
        let mut attempted = Vec::new();
        let error = run_profile_failover::<()>(&profiles, |candidate| {
            attempted.push(candidate.id.clone());
            Err(OpenaiRequestError::http(429, "quota exceeded"))
        })
        .unwrap_err();
        assert_eq!(attempted, vec!["a", "b"]);
        assert!(error.contains("429"), "got: {error}");
        assert!(error.contains("限流"), "got: {error}");
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
        let request = build_request(
            b"png-bytes",
            "image/png",
            Some("zh-Hans"),
            "glm-4v-flash",
            &prompts,
        );
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
        assert_eq!(
            messages[1]["content"][0]["text"],
            "First user msg for 简体中文."
        );
        assert_eq!(messages[1]["content"].as_array().unwrap().len(), 1);

        // Second (last) user message has text AND image_url
        assert_eq!(messages[2]["role"], "user");
        assert_eq!(messages[2]["content"][0]["text"], "Second user msg.");
        assert_eq!(messages[2]["content"][1]["type"], "image_url");
        assert!(messages[2]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
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
        assert_eq!(
            messages[0]["content"][0]["text"],
            "You are an OCR engine for 日语."
        );
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
        assert!(messages[0]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("英语"));
        assert_eq!(messages[1]["role"], "user");
        assert!(messages[1]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("英语"));
        assert_eq!(messages[1]["content"][1]["type"], "image_url");
    }

    #[test]
    fn small_image_under_threshold_is_kept_lossless() {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_pixel(120, 80, image::Rgba([10, 20, 30, 255]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        assert!(png.len() <= OPENAI_IMAGE_RAW_MAX_BYTES);

        let data_url = prepare_image_data_url_from_bytes(&png, "image/png");
        assert!(
            data_url.starts_with("data:image/png;base64,"),
            "小图应保持原始 PNG 直传，实际: {}",
            &data_url[..data_url.len().min(40)]
        );
    }

    #[test]
    fn oversized_dimension_image_is_downscaled_and_composited_to_jpeg() {
        let mut png = Vec::new();
        // 2400x1200 超过 2048 阈值，且带全透明像素（0, 0, 0, 0）验证白底合成
        let img = image::RgbaImage::from_pixel(2400, 1200, image::Rgba([0, 0, 0, 0]));
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();

        let data_url = prepare_image_data_url_from_bytes(&png, "image/png");
        assert!(
            data_url.starts_with("data:image/jpeg;base64,"),
            "超尺寸图应重编码为 JPEG，实际前缀: {}",
            &data_url[..data_url.len().min(40)]
        );

        let b64 = data_url.strip_prefix("data:image/jpeg;base64,").unwrap();
        let decoded_bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let decoded_img = image::load_from_memory(&decoded_bytes).unwrap().to_rgb8();
        assert_eq!(decoded_img.width(), 2048);
        assert_eq!(decoded_img.height(), 1024);
        // 全透明像素经白底合成后应接近纯白 (255, 255, 255)
        let px = decoded_img.get_pixel(100, 100);
        assert!(px[0] >= 250 && px[1] >= 250 && px[2] >= 250, "透明背景未合成白底: {px:?}");
    }
}
