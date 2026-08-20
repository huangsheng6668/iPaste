//! Manga-OCR (mocr) 专用日漫推理桥接器。
//! 当切换到「日语 · 漫画」模式时，优先通过本地 Python 环境与 Manga-OCR (ViT + RoBERTa)
//! 模型执行高精度漫画识别；若环境不可用则安全回退到 PaddleOCR Manga Profile。

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use crate::models::{ImageOcrResult, ImageOcrWord};
use crate::ocr::tokens::split_line_tokens;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const EMBEDDED_MOCR_SCRIPT: &str = r#"
import os, sys, json, torch
from PIL import Image

try:
    from transformers import AutoTokenizer, VisionEncoderDecoderModel, ViTImageProcessor
except ImportError:
    print(json.dumps({"error": "transformers not installed"}))
    sys.exit(1)

image_path = sys.argv[1]
custom_model_path = sys.argv[2] if len(sys.argv) > 2 and sys.argv[2] != "" else None

candidate_paths = [
    custom_model_path,
    r"E:\github_project\manga-translator-ui\models\ocr\manga_ocr",
    os.path.expanduser("~/.cache/manga_ocr"),
    "kha-white/manga-ocr-base"
]

model_path = None
for p in candidate_paths:
    if p and os.path.exists(p):
        model_path = p
        break

if not model_path:
    model_path = "kha-white/manga-ocr-base"

try:
    processor = ViTImageProcessor.from_pretrained(model_path)
    tokenizer = AutoTokenizer.from_pretrained(model_path)
    model = VisionEncoderDecoderModel.from_pretrained(model_path)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)
    model.eval()

    img = Image.open(image_path).convert("RGB")
    pixel_values = processor(img, return_tensors="pt").pixel_values.to(device)
    with torch.no_grad():
        generated_ids = model.generate(pixel_values, max_length=300)
    text = tokenizer.batch_decode(generated_ids, skip_special_tokens=True)[0]
    text = text.replace(" ", "")
    sys.stdout.buffer.write(json.dumps({"text": text, "device": device}, ensure_ascii=False).encode("utf-8"))
except Exception as e:
    sys.stdout.buffer.write(json.dumps({"error": str(e)}, ensure_ascii=False).encode("utf-8"))
    sys.exit(1)
"#;

use tauri::Manager;

/// 尝试使用 Manga-OCR 识别图片
pub(crate) fn recognize_image_text_mocr(
    app: Option<&tauri::AppHandle>,
    image_path: &str,
) -> Result<ImageOcrResult, String> {
    let image = Path::new(image_path);
    if !image.exists() {
        return Err("图片文件不存在".to_string());
    }

    let python_bin = find_python_executable(app)
        .ok_or_else(|| "未找到支持 Manga-OCR 的 Python 环境或独立引擎".to_string())?;

    let model_path = find_mocr_model_path(app).unwrap_or_default();

    let is_standalone = python_bin
        .file_name()
        .map(|n| {
            let s = n.to_string_lossy().to_lowercase();
            s.starts_with("mocr") || s.starts_with("manga")
        })
        .unwrap_or(false);

    let mut cmd = Command::new(&python_bin);
    if is_standalone {
        cmd.arg(image_path);
        if !model_path.is_empty() {
            cmd.arg(&model_path);
        }
    } else {
        cmd.arg("-c")
            .arg(EMBEDDED_MOCR_SCRIPT)
            .arg(image_path)
            .arg(&model_path);
    }

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let output = cmd
        .output()
        .map_err(|e| format!("执行 Manga-OCR 进程失败：{e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("Manga-OCR 执行出错：{stderr} {stdout}"));
    }

    let stdout_str = String::from_utf8(output.stdout)
        .map_err(|e| format!("解析 Manga-OCR 输出 UTF-8 失败：{e}"))?;

    #[derive(serde::Deserialize)]
    struct MocrOutput {
        text: Option<String>,
        error: Option<String>,
    }

    let parsed: MocrOutput = serde_json::from_str(&stdout_str)
        .map_err(|e| format!("解析 Manga-OCR JSON 输出失败：{e} ({stdout_str})"))?;

    if let Some(err) = parsed.error {
        return Err(format!("Manga-OCR 推理报错：{err}"));
    }

    let recognized_text = parsed.text.unwrap_or_default().trim().to_string();
    if recognized_text.is_empty() {
        return Err("Manga-OCR 未识别到有效文本".to_string());
    }

    // 构造词级数据
    let tokens = split_line_tokens(&recognized_text);
    let words = tokens
        .into_iter()
        .enumerate()
        .map(|(i, t)| ImageOcrWord {
            text: t.text,
            left: 0.0,
            top: 0.0,
            width: 0.0,
            height: 0.0,
            confidence: 99.0,
            block_index: 0,
            paragraph_index: 0,
            line_index: 0,
            word_index: i as i64,
        })
        .collect();

    Ok(ImageOcrResult {
        text: recognized_text,
        engine: "manga-ocr".to_string(),
        language: "ja".to_string(),
        words,
    })
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 查找可用的 Python 可执行文件或便携式独立 mocr 运行时
fn find_python_executable(app_handle: Option<&tauri::AppHandle>) -> Option<PathBuf> {
    // 0. 最高优先级：App 托管的便携式运行环境 / sidecar (app_data_dir/ocr/mocr/)
    if let Some(app) = app_handle {
        if let Ok(app_dir) = app.path().app_data_dir() {
            let mocr_dir = app_dir.join("ocr").join("mocr");
            #[cfg(windows)]
            {
                let candidates = [
                    mocr_dir.join("mocr.exe"),
                    mocr_dir.join("mocr_engine.exe"),
                    mocr_dir.join("python.exe"),
                    mocr_dir.join("Scripts").join("python.exe"),
                ];
                for c in candidates {
                    if c.exists() {
                        return Some(c);
                    }
                }
            }
            #[cfg(not(windows))]
            {
                let candidates = [
                    mocr_dir.join("mocr"),
                    mocr_dir.join("bin").join("python"),
                    mocr_dir.join("python"),
                ];
                for c in candidates {
                    if c.exists() {
                        return Some(c);
                    }
                }
            }
        }

        // 尝试 App 资源打包目录 (resources/mocr/)
        if let Ok(res_dir) = app.path().resource_dir() {
            let mocr_res = res_dir.join("resources").join("mocr");
            #[cfg(windows)]
            {
                let c = mocr_res.join("mocr.exe");
                if c.exists() {
                    return Some(c);
                }
            }
            #[cfg(not(windows))]
            {
                let c = mocr_res.join("mocr");
                if c.exists() {
                    return Some(c);
                }
            }
        }
    }

    // 1. 自定义环境变量
    if let Ok(p) = std::env::var("MOCR_PYTHON_PATH") {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    let mut candidates = Vec::new();

    // 2. 本地开发环境发现
    if let Some(home) = home_dir() {
        #[cfg(windows)]
        {
            candidates.push(home.join(r".conda\envs\manga-env\python.exe"));
            candidates.push(home.join(r".conda\envs\manga_ocr\python.exe"));
            candidates.push(home.join(r".conda\envs\mocr\python.exe"));
            candidates.push(home.join(r"miniconda3\envs\manga-env\python.exe"));
            candidates.push(home.join(r"miniconda3\envs\manga_ocr\python.exe"));
            candidates.push(home.join(r"anaconda3\envs\manga-env\python.exe"));
        }
        #[cfg(not(windows))]
        {
            candidates.push(home.join(".conda/envs/manga-env/bin/python"));
            candidates.push(home.join(".conda/envs/manga_ocr/bin/python"));
            candidates.push(home.join("miniconda3/envs/manga-env/bin/python"));
        }
    }

    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(
            r"E:\github_project\manga-translator-ui\venv\Scripts\python.exe",
        ));
        candidates.push(PathBuf::from(
            r"E:\miniconda3\envs\manga-env\python.exe",
        ));
    }

    for p in candidates {
        if p.exists() {
            return Some(p);
        }
    }

    // 3. 尝试系统 PATH 中的 python
    for cmd in ["python", "python3", "py"] {
        if let Ok(out) = Command::new(cmd).arg("--version").output() {
            if out.status.success() {
                return Some(PathBuf::from(cmd));
            }
        }
    }

    None
}

/// 查找本地 Manga-OCR 权重目录
fn find_mocr_model_path(app_handle: Option<&tauri::AppHandle>) -> Option<String> {
    // 0. 最高优先级：App 托管的便携式模型目录 (app_data_dir/ocr/mocr/)
    if let Some(app) = app_handle {
        if let Ok(app_dir) = app.path().app_data_dir() {
            let mocr_model_dir = app_dir.join("ocr").join("mocr").join("models");
            if mocr_model_dir.exists() {
                return Some(mocr_model_dir.to_string_lossy().to_string());
            }
            let mocr_dir = app_dir.join("ocr").join("mocr");
            if mocr_dir.join("config.json").exists() {
                return Some(mocr_dir.to_string_lossy().to_string());
            }
        }
    }

    // 1. 自定义环境变量
    if let Ok(p) = std::env::var("MOCR_MODEL_PATH") {
        if Path::new(&p).exists() {
            return Some(p);
        }
    }

    // 2. 本地开发目录
    let candidates = [
        PathBuf::from(r"E:\github_project\manga-translator-ui\models\ocr\manga_ocr"),
        home_dir()
            .map(|h| h.join(".cache/manga_ocr"))
            .unwrap_or_default(),
    ];

    for p in candidates {
        if p.exists() {
            return Some(p.to_string_lossy().to_string());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_python_or_none() {
        let _ = find_python_executable(None);
        let _ = find_mocr_model_path(None);
    }

    #[test]
    fn test_rejects_missing_image() {
        let res = recognize_image_text_mocr(None, "non_existent_file.png");
        assert!(res.is_err());
    }
}
