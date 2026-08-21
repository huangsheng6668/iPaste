//! Manga-OCR (mocr) 专用日漫推理桥接器。
//! 当切换到「日语 · 漫画」模式时，优先通过本地 Python 环境与 Manga-OCR (ViT + RoBERTa)
//! 模型执行高精度漫画识别；若环境不可用则安全回退到 PaddleOCR Manga Profile。
//!
//! Python 路径采用按需常驻的推理服务进程（stdin/stdout 按行交换 JSON）：
//! 冷启动需重付 import 依赖 + 权重加载（实测数秒），常驻后单次识别仅剩推理耗时；
//! 空闲超过 MOCR_IDLE_SHUTDOWN 后在下次调用时回收，避免长期占用 GPU/内存。

use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

#[cfg(windows)]
use tokio::process::Command as TokioCommand;

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin},
};

use crate::models::{ImageOcrResult, ImageOcrWord};
use crate::ocr::tokens::split_line_tokens;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// 空闲多久后在下次调用时重启（回收 GPU 显存与 Python 常驻内存）。
const MOCR_IDLE_SHUTDOWN: Duration = Duration::from_secs(5 * 60);
/// 单次请求的等待上限：需覆盖首次冷启动（依赖 import + 权重加载）。
const MOCR_RESPONSE_TIMEOUT: Duration = Duration::from_secs(180);

/// 常驻推理服务脚本：逐行读取 {"image": "...", "model": "..."} 请求，
/// 逐行回写 {"text": "...", "device": "..."} 或 {"error": "..."}。
/// 模型在首个请求时惰性加载（首响应即含加载耗时）。
const MOCR_SERVER_SCRIPT: &str = r#"
import os, sys, json

def emit(obj):
    sys.stdout.buffer.write((json.dumps(obj, ensure_ascii=False) + "\n").encode("utf-8"))
    sys.stdout.flush()

try:
    import torch
    from PIL import Image
    from transformers import AutoTokenizer, VisionEncoderDecoderModel, ViTImageProcessor
except Exception as e:
    emit({"error": "deps unavailable: %s" % e})
    sys.exit(1)

processor = tokenizer = model = device = None

def resolve_model(custom):
    candidates = [
        custom,
        r"E:\github_project\manga-translator-ui\models\ocr\manga_ocr",
        os.path.expanduser("~/.cache/manga_ocr"),
    ]
    for p in candidates:
        if p and os.path.exists(p):
            return p
    return "kha-white/manga-ocr-base"

def ensure_model(custom):
    global processor, tokenizer, model, device
    if model is not None:
        return
    path = resolve_model(custom)
    processor = ViTImageProcessor.from_pretrained(path)
    tokenizer = AutoTokenizer.from_pretrained(path)
    model = VisionEncoderDecoderModel.from_pretrained(path)
    device = "cuda" if torch.cuda.is_available() else "cpu"
    model.to(device)
    model.eval()

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    try:
        req = json.loads(line)
        ensure_model(req.get("model") or "")
        if req.get("warmup"):
            emit({"ready": True, "device": device})
            continue
        img = Image.open(req["image"]).convert("RGB")
        pixel_values = processor(img, return_tensors="pt").pixel_values.to(device)
        with torch.no_grad():
            generated = model.generate(pixel_values, max_length=300)
        text = tokenizer.batch_decode(generated, skip_special_tokens=True)[0]
        emit({"text": text.replace(" ", ""), "device": device})
    except Exception as e:
        emit({"error": str(e)})
"#;

struct MocrServer {
    child: Child,
    stdin: ChildStdin,
    /// 常驻按行读取进程响应；请求/响应在 server Mutex 内严格成对，天然同步。
    stdout: BufReader<tokio::process::ChildStdout>,
    last_used: Instant,
}

impl MocrServer {
    fn shutdown(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// tokio Mutex：guard 需跨 await 持有（请求串行化），std Mutex 会破坏 future 的 Send。
static MOCR_SERVER: OnceLock<tokio::sync::Mutex<Option<MocrServer>>> = OnceLock::new();
static PYTHON_CANDIDATE: OnceLock<Option<PathBuf>> = OnceLock::new();
static MODEL_CANDIDATE: OnceLock<Option<String>> = OnceLock::new();

use tauri::Manager;

/// 尝试使用 Manga-OCR 识别图片（async：进程 IO 全异步，CPU 推理在子进程侧）。
pub(crate) async fn recognize_image_text_mocr(
    app: Option<&tauri::AppHandle>,
    image_path: &str,
) -> Result<ImageOcrResult, String> {
    let image = Path::new(image_path);
    if !image.exists() {
        return Err("图片文件不存在".to_string());
    }

    let python_bin = cached_python_executable(app)
        .ok_or_else(|| "未找到支持 Manga-OCR 的 Python 环境或独立引擎".to_string())?;

    let model_path = cached_mocr_model_path(app).unwrap_or_default();

    let is_standalone = python_bin
        .file_name()
        .map(|n| {
            let s = n.to_string_lossy().to_lowercase();
            s.starts_with("mocr") || s.starts_with("manga")
        })
        .unwrap_or(false);

    if is_standalone {
        // 独立引擎不受本仓协议约束，保持一次性调用
        let mut args: Vec<&str> = vec![image_path];
        if !model_path.is_empty() {
            args.push(&model_path);
        }
        let output = run_once(&python_bin, &args).await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(format!("Manga-OCR 执行出错：{stderr} {stdout}"));
        }
        return parse_standalone_output(&String::from_utf8_lossy(&output.stdout));
    }

    let payload = serde_json::json!({ "image": image_path, "model": model_path });
    let stdout_line = request_via_server(app, &payload).await?;
    finish_from_server_line(&stdout_line)
}

/// 服务进程一行响应 → 结果（脚本级错误/空文本均转 Err，上层回退 Paddle manga）。
fn finish_from_server_line(stdout_line: &str) -> Result<ImageOcrResult, String> {
    let parsed: MocrOutput = serde_json::from_str(stdout_line.trim())
        .map_err(|error| format!("解析 Manga-OCR JSON 输出失败：{error} ({stdout_line})"))?;
    if let Some(err) = parsed.error {
        return Err(format!("Manga-OCR 推理报错：{err}"));
    }
    let recognized_text = parsed.text.unwrap_or_default().trim().to_string();
    if recognized_text.is_empty() {
        return Err("Manga-OCR 未识别到有效文本".to_string());
    }
    build_result(recognized_text)
}

/// 序列化后的请求体按行写入服务进程并等待一行响应。
async fn send_request(server: &mut MocrServer, payload: &serde_json::Value) -> Result<String, String> {
    let line = serde_json::to_string(payload)
        .map_err(|error| format!("构造 Manga-OCR 请求失败：{error}"))?;
    server
        .stdin
        .write_all(line.as_bytes())
        .await
        .map_err(|error| format!("写入 Manga-OCR 请求失败：{error}"))?;
    server
        .stdin
        .write_all(b"\n")
        .await
        .map_err(|error| format!("写入 Manga-OCR 请求失败：{error}"))?;
    server
        .stdin
        .flush()
        .await
        .map_err(|error| format!("刷新 Manga-OCR 请求失败：{error}"))?;

    let mut response = String::new();
    let read = tokio::time::timeout(MOCR_RESPONSE_TIMEOUT, server.stdout.read_line(&mut response))
        .await
        .map_err(|_| "Manga-OCR 响应超时".to_string())?
        .map_err(|error| format!("读取 Manga-OCR 响应失败：{error}"))?;
    if read == 0 {
        return Err("Manga-OCR 进程已退出".to_string());
    }
    Ok(response)
}

/// 预热常驻推理服务（结果窗打开时后台调用）：启动进程并触发模型加载，
/// 用户首次点击 manga profile 时通常已就绪。失败静默——不影响正常冷启动路径。
pub(crate) async fn prewarm_server(app: &tauri::AppHandle) {
    let cell = MOCR_SERVER.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cell.lock().await;
    if guard.is_some() {
        return; // 已就绪或预热中
    }
    let Some(python_bin) = cached_python_executable(Some(app)) else {
        return;
    };
    let is_standalone = python_bin
        .file_name()
        .map(|n| {
            let s = n.to_string_lossy().to_lowercase();
            s.starts_with("mocr") || s.starts_with("manga")
        })
        .unwrap_or(false);
    if is_standalone {
        return; // 独立引擎无行协议，无可预热
    }
    let Ok(mut server) = spawn_server(&python_bin).await else {
        return;
    };
    let model_path = cached_mocr_model_path(Some(app)).unwrap_or_default();
    let payload = serde_json::json!({ "warmup": true, "model": model_path });
    if send_request(&mut server, &payload).await.is_err() {
        server.shutdown();
        return;
    }
    server.last_used = Instant::now();
    *guard = Some(server);
}

/// 应用退出时回收常驻推理进程（防止孤儿进程残留占用内存/显存）。
pub(crate) async fn shutdown_server() {
    if let Some(cell) = MOCR_SERVER.get() {
        if let Some(mut server) = cell.lock().await.take() {
            server.shutdown();
            let _ = server.child.wait().await;
        }
    }
}

/// 在常驻服务上执行一次请求：确保服务存活（空闲过期则回收重建）→
/// 写请求行 → 等待一行响应。进程死亡/超时等不可恢复错误会回收服务并返回 Err
/// （上层回退 Paddle manga 管线），下次调用重新冷启动。
async fn request_via_server(
    app: Option<&tauri::AppHandle>,
    payload: &serde_json::Value,
) -> Result<String, String> {
    let cell = MOCR_SERVER.get_or_init(|| tokio::sync::Mutex::new(None));
    let mut guard = cell.lock().await;

    if let Some(server) = guard.as_mut() {
        if server.last_used.elapsed() > MOCR_IDLE_SHUTDOWN {
            server.shutdown();
            *guard = None;
        }
    }
    if guard.is_none() {
        let python_bin = cached_python_executable(app)
            .ok_or_else(|| "未找到支持 Manga-OCR 的 Python 环境或独立引擎".to_string())?;
        *guard = Some(spawn_server(&python_bin).await?);
    }

    let server = guard.as_mut().expect("server just ensured");
    let result = send_request(server, payload).await;
    server.last_used = Instant::now();
    if result.is_err() {
        // 通信失败可能意味着进程已死：回收以便下次重建
        server.shutdown();
        *guard = None;
    }
    result
}

/// 独立引擎输出兼容：优先按 JSON 解析，失败则整段视为纯文本。
fn parse_standalone_output(stdout: &str) -> Result<ImageOcrResult, String> {
    let trimmed = stdout.trim();
    let recognized_text = match serde_json::from_str::<MocrOutput>(trimmed) {
        Ok(parsed) => {
            if let Some(err) = parsed.error {
                return Err(format!("Manga-OCR 推理报错：{err}"));
            }
            parsed.text.unwrap_or_default().trim().to_string()
        }
        Err(_) => trimmed.to_string(),
    };
    if recognized_text.is_empty() {
        return Err("Manga-OCR 未识别到有效文本".to_string());
    }
    build_result(recognized_text)
}

fn build_result(recognized_text: String) -> Result<ImageOcrResult, String> {
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

#[derive(serde::Deserialize)]
struct MocrOutput {
    text: Option<String>,
    error: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// 子进程统一构造入口：可执行文件仅接受 find_python_executable 白名单
/// 候选（conda/App 托管/资源目录）中 exists() 命中的路径，argv 数组式
/// 传参、不经 shell，无命令注入面。
fn build_mocr_command(python_bin: &Path) -> TokioCommand {
    let mut cmd = TokioCommand::new(python_bin);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// 独立引擎一次性调用（argv：图片路径 + 模型路径）。
async fn run_once(python_bin: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    build_mocr_command(python_bin)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("执行 Manga-OCR 进程失败：{e}"))
}

/// 启动常驻推理服务进程（stdin/stdout 管道）。
async fn spawn_server(python_bin: &Path) -> Result<MocrServer, String> {
    let mut child = build_mocr_command(python_bin)
        .arg("-c")
        .arg(MOCR_SERVER_SCRIPT)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| format!("启动 Manga-OCR 推理进程失败：{error}"))?;
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "获取 Manga-OCR 进程 stdin 失败".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "获取 Manga-OCR 进程 stdout 失败".to_string())?;

    Ok(MocrServer {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        last_used: Instant::now(),
    })
}

/// Python 环境探测含 exists 乃至 spawn 探测，结果进程内不变，缓存一次。
fn cached_python_executable(app_handle: Option<&tauri::AppHandle>) -> Option<PathBuf> {
    PYTHON_CANDIDATE
        .get_or_init(|| find_python_executable(app_handle))
        .clone()
}

fn cached_mocr_model_path(app_handle: Option<&tauri::AppHandle>) -> Option<String> {
    MODEL_CANDIDATE
        .get_or_init(|| find_mocr_model_path(app_handle))
        .clone()
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

    // 3. 尝试系统 PATH 中的 python（纯文件查找，不 spawn 探测进程）
    for name in ["python.exe", "python3.exe", "py.exe", "python", "python3"] {
        if let Some(path) = find_in_path(name) {
            return Some(path);
        }
    }

    None
}

/// 在 PATH 各目录中查找可执行文件；PATH 未设置时返回 None。
fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
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

    #[tokio::test]
    async fn test_rejects_missing_image() {
        let res = recognize_image_text_mocr(None, "non_existent_file.png").await;
        assert!(res.is_err());
    }

    #[test]
    fn standalone_output_parses_json_and_plain_text() {
        let parsed = parse_standalone_output("{\"text\": \"こんにちは\"}\n").unwrap();
        assert_eq!(parsed.text, "こんにちは");
        assert_eq!(parsed.engine, "manga-ocr");

        let plain = parse_standalone_output("plain text output\n").unwrap();
        assert_eq!(plain.text, "plain text output");

        let errored = parse_standalone_output("{\"error\": \"boom\"}");
        assert!(errored.is_err());
    }
}
