use std::path::{Path, PathBuf};

use objc2::{
    ffi::NSUInteger,
    msg_send,
    rc::{autoreleasepool, Retained},
    runtime::{AnyClass, AnyObject, Bool},
    sel,
};
use objc2_core_foundation::CGRect;
use objc2_foundation::{NSArray, NSError, NSRange, NSString, NSURL};


#[cfg(target_os = "macos")]
pub(crate) const MACOS_OCR_ENGINE_ID: &str = "apple-vision";
#[cfg(target_os = "macos")]
const MACOS_OCR_LANGUAGE: &str = "zh-Hans+en";
#[cfg(target_os = "macos")]
const MACOS_OCR_RECOGNITION_LEVEL_ACCURATE: isize = 0;

#[cfg(target_os = "macos")]
pub(crate) fn recognize_image_text_macos(image_path: String) -> Result<ImageOcrResult, String> {
    let image_path = PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }

    let (image_width, image_height) = image::image_dimensions(&image_path)
        .map_err(|error| format!("无法读取图片尺寸：{error}"))?;
    if image_width == 0 || image_height == 0 {
        return Err("图片尺寸无效".to_string());
    }

    autoreleasepool(|_| recognize_image_text_macos_inner(&image_path, image_width, image_height))
}

#[cfg(target_os = "macos")]
fn recognize_image_text_macos_inner(
    image_path: &Path,
    image_width: u32,
    image_height: u32,
) -> Result<ImageOcrResult, String> {
    let url = NSURL::from_file_path(image_path).ok_or_else(|| "无法读取图片路径".to_string())?;
    let request_class = AnyClass::get(c"VNRecognizeTextRequest")
        .ok_or_else(|| "当前 macOS 不支持系统图片 OCR".to_string())?;
    let handler_class = AnyClass::get(c"VNImageRequestHandler")
        .ok_or_else(|| "当前 macOS 不支持系统图片 OCR".to_string())?;

    let request: Retained<AnyObject> = unsafe { msg_send![request_class, new] };
    configure_macos_text_request(&request);

    let handler_alloc: *mut AnyObject = unsafe { msg_send![handler_class, alloc] };
    let handler_raw: *mut AnyObject = unsafe {
        msg_send![
            handler_alloc,
            initWithURL: &*url,
            options: None::<&AnyObject>
        ]
    };
    let handler = unsafe { Retained::from_raw(handler_raw) }
        .ok_or_else(|| "无法初始化系统图片 OCR".to_string())?;
    let requests = NSArray::from_slice(&[&*request]);
    let mut error: Option<Retained<NSError>> = None;
    let performed: Bool = unsafe {
        msg_send![
            &*handler,
            performRequests: &*requests,
            error: &mut error
        ]
    };

    if !performed.as_bool() {
        return Err(error
            .map(|error| format!("系统图片 OCR 识别失败：{error}"))
            .unwrap_or_else(|| "系统图片 OCR 识别失败".to_string()));
    }

    let observations: Option<Retained<NSArray<AnyObject>>> =
        unsafe { msg_send![&*request, results] };
    let Some(observations) = observations else {
        return Ok(ImageOcrResult {
            text: String::new(),
            engine: MACOS_OCR_ENGINE_ID.to_string(),
            language: MACOS_OCR_LANGUAGE.to_string(),
            words: Vec::new(),
        });
    };

    let mut words = Vec::new();
    let mut lines = Vec::new();
    let observation_count = observations.count();
    for observation_index in 0..observation_count {
        let observation = observations.objectAtIndex(observation_index);
        let candidates = macos_top_text_candidates(&observation, 1);
        let Some(candidate) =
            candidates.and_then(|items| (items.count() > 0).then(|| items.objectAtIndex(0)))
        else {
            continue;
        };

        let line_text = macos_recognized_text_string(&candidate);
        if line_text.trim().is_empty() {
            continue;
        }
        lines.push(line_text.clone());

        let line_confidence = macos_recognized_text_confidence(&candidate) as f64 * 100.0;
        let tokens = macos_ocr_tokens(&line_text);
        if tokens.is_empty() {
            if let Some(bounding_box) = macos_recognized_text_bounding_box(
                &candidate,
                NSRange::new(0, candidate_string_utf16_len(&candidate)),
            ) {
                words.push(macos_ocr_word_from_bounding_box(
                    line_text.trim().to_string(),
                    bounding_box,
                    image_width,
                    image_height,
                    line_confidence,
                    observation_index as i64,
                    0,
                    observation_index as i64,
                    0,
                ));
            }
            continue;
        }

        for (word_index, token) in tokens.into_iter().enumerate() {
            let bounding_box =
                macos_recognized_text_bounding_box(&candidate, token.range).or_else(|| {
                    macos_recognized_text_bounding_box(
                        &candidate,
                        NSRange::new(0, candidate_string_utf16_len(&candidate)),
                    )
                });
            if let Some(bounding_box) = bounding_box {
                words.push(macos_ocr_word_from_bounding_box(
                    token.text,
                    bounding_box,
                    image_width,
                    image_height,
                    line_confidence,
                    observation_index as i64,
                    0,
                    observation_index as i64,
                    word_index as i64,
                ));
            }
        }
    }

    Ok(ImageOcrResult {
        text: lines.join("\n"),
        engine: MACOS_OCR_ENGINE_ID.to_string(),
        language: MACOS_OCR_LANGUAGE.to_string(),
        words,
    })
}

#[cfg(target_os = "macos")]
fn configure_macos_text_request(request: &AnyObject) {
    unsafe {
        let _: () = msg_send![
            request,
            setRecognitionLevel: MACOS_OCR_RECOGNITION_LEVEL_ACCURATE
        ];
        let _: () = msg_send![request, setUsesLanguageCorrection: Bool::YES];
        let supports_languages: Bool =
            msg_send![request, respondsToSelector: sel!(setRecognitionLanguages:)];
        if supports_languages.as_bool() {
            let zh = NSString::from_str("zh-Hans");
            let en = NSString::from_str("en-US");
            let languages = NSArray::from_slice(&[&*zh, &*en]);
            let _: () = msg_send![request, setRecognitionLanguages: &*languages];
        }
        let supports_language_detection: Bool =
            msg_send![request, respondsToSelector: sel!(setAutomaticallyDetectsLanguage:)];
        if supports_language_detection.as_bool() {
            let _: () = msg_send![request, setAutomaticallyDetectsLanguage: Bool::YES];
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_top_text_candidates(
    observation: &AnyObject,
    max_candidates: NSUInteger,
) -> Option<Retained<NSArray<AnyObject>>> {
    unsafe { msg_send![observation, topCandidates: max_candidates] }
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_string(candidate: &AnyObject) -> String {
    let value: Retained<NSString> = unsafe { msg_send![candidate, string] };
    value.to_string()
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_confidence(candidate: &AnyObject) -> f32 {
    unsafe { msg_send![candidate, confidence] }
}

#[cfg(target_os = "macos")]
fn macos_recognized_text_bounding_box(candidate: &AnyObject, range: NSRange) -> Option<CGRect> {
    if range.length == 0 {
        return None;
    }

    let mut error: Option<Retained<NSError>> = None;
    let box_observation: Option<Retained<AnyObject>> = unsafe {
        msg_send![
            candidate,
            boundingBoxForRange: range,
            error: &mut error
        ]
    };
    let box_observation = box_observation?;
    let bounding_box: CGRect = unsafe { msg_send![&*box_observation, boundingBox] };

    if error.is_some() || bounding_box.size.width <= 0.0 || bounding_box.size.height <= 0.0 {
        None
    } else {
        Some(bounding_box)
    }
}

#[cfg(target_os = "macos")]
fn macos_ocr_word_from_bounding_box(
    text: String,
    bounding_box: CGRect,
    image_width: u32,
    image_height: u32,
    confidence: f64,
    block_index: i64,
    paragraph_index: i64,
    line_index: i64,
    word_index: i64,
) -> ImageOcrWord {
    let image_width = image_width as f64;
    let image_height = image_height as f64;
    let left = bounding_box.origin.x * image_width;
    let top = (1.0 - bounding_box.origin.y - bounding_box.size.height) * image_height;
    let width = bounding_box.size.width * image_width;
    let height = bounding_box.size.height * image_height;

    ImageOcrWord {
        text,
        left: left.max(0.0),
        top: top.max(0.0),
        width: width.max(1.0),
        height: height.max(1.0),
        confidence,
        block_index,
        paragraph_index,
        line_index,
        word_index,
    }
}

#[cfg(target_os = "macos")]
#[derive(Debug)]
struct MacOcrToken {
    text: String,
    range: NSRange,
}

#[cfg(target_os = "macos")]
fn macos_ocr_tokens(text: &str) -> Vec<MacOcrToken> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_start = 0_usize;
    let mut utf16_offset = 0_usize;
    let mut current_is_cjk = false;

    for char in text.chars() {
        let char_len = char.len_utf16();
        if char.is_whitespace() {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            utf16_offset += char_len;
            current_is_cjk = false;
            continue;
        }

        let is_cjk = is_cjk_char(char);
        if current.is_empty() {
            current_start = utf16_offset;
            current_is_cjk = is_cjk;
        } else if is_cjk || current_is_cjk {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            current_start = utf16_offset;
            current_is_cjk = is_cjk;
        }

        current.push(char);
        utf16_offset += char_len;

        if is_cjk {
            push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
            current_is_cjk = false;
        }
    }

    push_macos_ocr_token(&mut tokens, &mut current, current_start, utf16_offset);
    tokens
}

#[cfg(target_os = "macos")]
fn push_macos_ocr_token(
    tokens: &mut Vec<MacOcrToken>,
    current: &mut String,
    start: usize,
    end: usize,
) {
    let value = current.trim();
    if !value.is_empty() && end > start {
        tokens.push(MacOcrToken {
            text: value.to_string(),
            range: NSRange::new(start, end - start),
        });
    }
    current.clear();
}

#[cfg(target_os = "macos")]
fn candidate_string_utf16_len(candidate: &AnyObject) -> usize {
    let value: Retained<NSString> = unsafe { msg_send![candidate, string] };
    value.length()
}

#[cfg(target_os = "macos")]
fn is_cjk_char(char: char) -> bool {
    matches!(
        char as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}
