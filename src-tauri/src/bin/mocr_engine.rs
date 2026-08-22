//! Manga-OCR ONNX 推理 sidecar（mocr_engine）。
//!
//! 独立进程：onnxruntime 静态库的 /MD CRT 与主程序里 mnn（ocr-rs）的 /MT
//! 假设冲突，进程隔离是唯一无侵入的共存方式（微软对混链直接报 LNK2038）。
//!
//! 协议与 Python 版常驻服务（mocr.rs MOCR_SERVER_SCRIPT）同构：
//! - 逐行读取 {"image": "...", "model": "<models-dir>"} → 逐行回写
//!   {"text": "..."} / {"error": "..."}；
//! - {"warmup": true, "model": "<models-dir>"} → {"ready": true}（惰性建会话）。
//! models-dir 布局：encoder.onnx + decoder.onnx + vocab.txt。
//!
//! 识别流程：图像 → 224² 双线性 + (x-0.5)/0.5 → NCHW → encoder 一次前向 →
//! decoder 无 kv-cache 的 greedy 自回归（2 层 decoder，逐步重算很便宜；
//! 实测与官方 beam=4 输出一致）→ vocab.txt 行号映射回文本。
//!
//! ort 依赖仅声明在 [target.'cfg(windows)'.dependencies]：非 Windows 平台
//! 本 bin 编译为占位空壳（macOS 的 ort 构建问题解决前不分发 sidecar，
//! mocr 识别走 Python/Paddle 回退）。

#[cfg(not(windows))]
fn main() {
    eprintln!("mocr_engine: Windows-only sidecar build (mocr falls back to Python/Paddle elsewhere)");
}

#[cfg(windows)]
fn main() {
    windows_impl::run();
}

#[cfg(windows)]
mod windows_impl {
use std::io::{BufRead, Write};

use ort::{
    session::{Session, builder::GraphOptimizationLevel},
    value::Tensor,
};

const ENCODER_FILE: &str = "encoder.onnx";
const DECODER_FILE: &str = "decoder.onnx";
const VOCAB_FILE: &str = "vocab.txt";
const DECODER_START_TOKEN_ID: i64 = 2;
const EOS_TOKEN_ID: i64 = 3;
const MAX_LENGTH: usize = 300;
const IMAGE_SIZE: u32 = 224;
/// 词表前 5 个 special token（[PAD][UNK][CLS][SEP][MASK]），decode 时剔除。
const SPECIAL_TOKEN_IDS: &[i64] = &[0, 1, 2, 3, 4];

struct Engine {
    encoder: Session,
    decoder: Session,
    vocab: Vec<String>,
}

fn emit(value: &serde_json::Value) -> Result<(), String> {
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    serde_json::to_writer(&mut lock, value).map_err(|error| error.to_string())?;
    lock.write_all(b"\n").map_err(|error| error.to_string())?;
    lock.flush().map_err(|error| error.to_string())
}

/// 入口：读行循环（crate main 在 cfg(windows) 下调用）。
pub fn run() {
    let stdin = std::io::stdin();
    let mut engine: Option<Engine> = None;
    let mut line = String::new();
    loop {
        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => break, // 上游关闭或读错误：退出
            Ok(_) => {}
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let request = match serde_json::from_str::<serde_json::Value>(trimmed) {
            Ok(request) => request,
            Err(error) => {
                let _ = emit(&serde_json::json!({ "error": format!("bad request: {error}") }));
                continue;
            }
        };
        let models_dir = request
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();

        let result = (|| -> Result<String, String> {
            if engine.is_none() {
                engine = Some(load_engine(&models_dir)?);
            }
            if request.get("warmup").and_then(|v| v.as_bool()).unwrap_or(false) {
                return Ok(String::new());
            }
            let image_path = request
                .get("image")
                .and_then(|value| value.as_str())
                .ok_or("missing image")?;
            recognize(engine.as_mut().expect("engine just loaded"), image_path)
        })();
        match result {
            Ok(text) if request.get("warmup").and_then(|v| v.as_bool()).unwrap_or(false) => {
                let _ = emit(&serde_json::json!({ "ready": true }));
            }
            Ok(text) => {
                let _ = emit(&serde_json::json!({ "text": text }));
            }
            Err(error) => {
                let _ = emit(&serde_json::json!({ "error": error }));
            }
        }
    }
}

fn load_engine(models_dir: &str) -> Result<Engine, String> {
    if models_dir.is_empty() {
        return Err("missing models dir".to_string());
    }
    let dir = std::path::Path::new(models_dir);
    let build_session = |file: &str, name: &str| -> Result<Session, String> {
        Session::builder()
            .map_err(|error| format!("init {name}: {error}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|error| format!("configure {name}: {error}"))?
            .commit_from_file(dir.join(file))
            .map_err(|error| format!("load {name}: {error}"))
    };
    let encoder = build_session(ENCODER_FILE, "encoder")?;
    let decoder = build_session(DECODER_FILE, "decoder")?;
    let vocab = load_vocab(&dir.join(VOCAB_FILE))?;
    if vocab.len() <= SPECIAL_TOKEN_IDS.len() {
        return Err("vocab incomplete".to_string());
    }
    Ok(Engine {
        encoder,
        decoder,
        vocab,
    })
}

/// vocab.txt 每行一个 token，行号即 id（字符级词表，无需 WordPiece 还原）。
fn load_vocab(path: &std::path::Path) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    Ok(content.lines().map(|line| line.trim().to_string()).collect())
}

/// 读图 → RGB → 224² 双线性 → (x-0.5)/0.5 → NCHW f32。
fn preprocess(image_path: &str) -> Result<Vec<f32>, String> {
    let image = image::open(image_path)
        .map_err(|error| format!("open image: {error}"))?
        .to_rgb8();
    let resized =
        image::imageops::resize(&image, IMAGE_SIZE, IMAGE_SIZE, image::imageops::FilterType::Triangle);
    let mut pixel_values = Vec::with_capacity((IMAGE_SIZE * IMAGE_SIZE * 3) as usize);
    for channel in 0..3 {
        for y in 0..IMAGE_SIZE {
            for x in 0..IMAGE_SIZE {
                let value = f32::from(resized.get_pixel(x, y)[channel]);
                pixel_values.push((value / 255.0 - 0.5) / 0.5);
            }
        }
    }
    Ok(pixel_values)
}

fn recognize(engine: &mut Engine, image_path: &str) -> Result<String, String> {
    let pixel_values = preprocess(image_path)?;
    let encoder_outputs = engine
        .encoder
        .run(ort::inputs![
            "pixel_values" => Tensor::from_array((
                [1usize, 3, IMAGE_SIZE as usize, IMAGE_SIZE as usize],
                pixel_values,
            ))
            .map_err(|error| error.to_string())?,
        ])
        .map_err(|error| format!("encoder run: {error}"))?;
    let (hidden_shape, hidden) = encoder_outputs["last_hidden_state"]
        .try_extract_tensor::<f32>()
        .map_err(|error| format!("encoder output: {error}"))?;
    let encoder_hidden_states: Vec<f32> = hidden.to_vec();
    let hidden_len = shape_dim(&hidden_shape, 1)?;
    let hidden_dim = shape_dim(&hidden_shape, 2)?;

    let mut tokens: Vec<i64> = vec![DECODER_START_TOKEN_ID];
    for _ in 0..(MAX_LENGTH - 1) {
        let seq_len = tokens.len();
        let decoder_outputs = engine
            .decoder
            .run(ort::inputs![
                "input_ids" => Tensor::from_array(([1usize, seq_len], tokens.clone()))
                    .map_err(|error| error.to_string())?,
                "encoder_hidden_states" => Tensor::from_array((
                    [1usize, hidden_len, hidden_dim],
                    encoder_hidden_states.clone(),
                ))
                .map_err(|error| error.to_string())?,
            ])
            .map_err(|error| format!("decoder run: {error}"))?;
        let (logits_shape, logits) = decoder_outputs["logits"]
            .try_extract_tensor::<f32>()
            .map_err(|error| format!("decoder output: {error}"))?;
        let vocab_len = shape_dim(&logits_shape, logits_shape.len() - 1)?;
        if logits.len() < vocab_len {
            return Err("logits too short".to_string());
        }
        let last_row = &logits[logits.len() - vocab_len..];
        let next_token = last_row
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index as i64)
            .ok_or("empty logits")?;
        if next_token == EOS_TOKEN_ID {
            break;
        }
        tokens.push(next_token);
    }
    Ok(decode_tokens(&tokens, &engine.vocab))
}

fn shape_dim(shape: &[i64], index: usize) -> Result<usize, String> {
    shape
        .get(index)
        .copied()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| format!("bad shape dim {index}"))
}

fn decode_tokens(tokens: &[i64], vocab: &[String]) -> String {
    tokens
        .iter()
        .filter(|token| !SPECIAL_TOKEN_IDS.contains(token))
        .filter_map(|token| vocab.get(*token as usize))
        .cloned()
        .collect::<String>()
        .replace(' ', "")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_tokens_skips_specials_and_spaces() {
        let mut vocab: Vec<String> = (0..8).map(|i| format!("tok{i}")).collect();
        vocab[2] = "[CLS]".to_string();
        vocab[3] = "[SEP]".to_string();
        vocab[5] = "  ".to_string();
        vocab[6] = "こ".to_string();
        vocab[7] = "ん".to_string();
        assert_eq!(decode_tokens(&[2, 6, 5, 7, 3], &vocab), "こん");
    }

    #[test]
    fn preprocess_shapes_are_nchw() {
        let path = std::env::temp_dir().join("mocr-engine-preprocess-test.png");
        image::RgbImage::from_pixel(64, 64, image::Rgb([255, 128, 0]))
            .save(&path)
            .unwrap();
        let values = preprocess(path.to_str().unwrap()).unwrap();
        assert_eq!(values.len(), 224 * 224 * 3);
        assert!((values[0] - 1.0).abs() < 1e-6, "R=255 → +1.0");
        assert!((values[224 * 224] - (128.0 / 255.0 - 0.5) / 0.5).abs() < 1e-4);
        assert!((values[2 * 224 * 224] + 1.0).abs() < 1e-6, "B=0 → -1.0");
        let _ = std::fs::remove_file(&path);
    }

    /// 本地端到端（需先跑 export/verify 脚本产出 ocr-spike/mocr-onnx 与
    /// verify-input.png）：直接调引擎函数，输出必须与 Python 参考一致。
    #[test]
    #[ignore = "requires local onnx export under ocr-spike/mocr-onnx"]
    fn local_end_to_end_matches_reference() {
        let model_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../ocr-spike/mocr-onnx");
        let image = concat!(env!("CARGO_MANIFEST_DIR"), "/../ocr-spike/mocr-onnx/verify-input.png");
        assert!(
            std::path::Path::new(model_dir).join(ENCODER_FILE).is_file(),
            "run scripts/ocr-models/export-mocr-onnx.py first"
        );
        let mut engine = load_engine(model_dir).unwrap();
        let text = recognize(&mut engine, image).unwrap();
        assert_eq!(text, "そういえば、昨日の漫画を");
    }
}
} // mod windows_impl
