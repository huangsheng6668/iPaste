use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use super::installer::{ocr_asset_dir, ocr_engine_dir};
use crate::models::{ImageOcrResult, ImageOcrWord};

/// Tesseract 单次识别超时：挂死的引擎不应永久占用 spawn_blocking 线程。
const TESSERACT_TIMEOUT: Duration = Duration::from_secs(120);

/// 等待子进程退出；超过 `timeout` 则 kill 并返回错误。
/// 轮询 `try_wait` 而非阻塞 `wait`，才能在超时时主动终止挂死进程。
fn wait_child_with_timeout(child: &mut std::process::Child, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if child.try_wait().map_err(|error| error.to_string())?.is_some() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Tesseract 识别超时".to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn recognize_image_text_inner(
    app: &tauri::AppHandle,
    image_path: String,
) -> Result<ImageOcrResult, String> {
    let image_path = PathBuf::from(image_path);
    if !image_path.exists() {
        return Err("图片文件不存在".to_string());
    }

    let tesseract = find_tesseract_executable(app)?;
    let tessdata_dir = ocr_asset_dir(app)?;
    if !tessdata_dir.join("eng.traineddata").exists()
        || !tessdata_dir.join("chi_sim.traineddata").exists()
    {
        return Err("请先在偏好设置中下载图片 OCR 资源".to_string());
    }

    let mut child = Command::new(&tesseract)
        .arg(&image_path)
        .arg("stdout")
        .arg("-l")
        .arg("chi_sim+eng")
        .arg("--tessdata-dir")
        .arg(&tessdata_dir)
        .arg("-c")
        .arg("tessedit_create_tsv=1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("无法启动 Tesseract：{error}"))?;
    wait_child_with_timeout(&mut child, TESSERACT_TIMEOUT)?;
    // 子进程已退出（或被超时 kill 前的最后一次等待），此处仅回收并读取管道
    let output = child
        .wait_with_output()
        .map_err(|error| format!("读取 Tesseract 输出失败：{error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Tesseract 识别失败".to_string()
        } else {
            stderr
        });
    }

    let tsv = String::from_utf8_lossy(&output.stdout);
    let words = parse_tesseract_tsv(&tsv);
    let text = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(ImageOcrResult {
        text,
        engine: tesseract.to_string_lossy().to_string(),
        language: "chi_sim+eng".to_string(),
        words,
    })
}

#[cfg(not(target_os = "macos"))]
fn find_tesseract_executable(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let app_data_tesseract = ocr_engine_dir(app)?.join("tesseract.exe");
    if app_data_tesseract.exists() {
        return Ok(app_data_tesseract);
    }

    Err("未找到 Tesseract 引擎。请先在偏好设置中下载图片 OCR 资源。".to_string())
}

#[cfg(not(target_os = "macos"))]
fn parse_tesseract_tsv(tsv: &str) -> Vec<ImageOcrWord> {
    tsv.lines()
        .skip(1)
        .filter_map(parse_tesseract_tsv_line)
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn parse_tesseract_tsv_line(line: &str) -> Option<ImageOcrWord> {
    let columns = line.split('\t').collect::<Vec<_>>();
    if columns.len() < 12 || columns.first()? != &"5" {
        return None;
    }

    let text = columns[11].trim();
    let confidence = columns[10].parse::<f64>().ok()?;
    if text.is_empty() || confidence < 0.0 {
        return None;
    }

    Some(ImageOcrWord {
        text: text.to_string(),
        left: parse_tsv_number(columns[6])?,
        top: parse_tsv_number(columns[7])?,
        width: parse_tsv_number(columns[8])?,
        height: parse_tsv_number(columns[9])?,
        confidence,
        block_index: columns[2].parse::<i64>().ok()?,
        paragraph_index: columns[3].parse::<i64>().ok()?,
        line_index: columns[4].parse::<i64>().ok()?,
        word_index: columns[5].parse::<i64>().ok()?,
    })
}

#[cfg(not(target_os = "macos"))]
fn parse_tsv_number(value: &str) -> Option<f64> {
    value.parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 约 2 秒后自然退出的子进程（Windows 用 powershell，Unix 用 sleep）。
    fn short_lived_child() -> std::process::Command {
        #[cfg(windows)]
        {
            let mut cmd = std::process::Command::new("powershell");
            cmd.args(["-NoProfile", "-Command", "Start-Sleep", "-Seconds", "2"]);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = std::process::Command::new("sleep");
            cmd.arg("2");
            cmd
        }
    }

    #[test]
    fn wait_child_with_timeout_kills_process_exceeding_timeout() {
        let mut child = short_lived_child().spawn().unwrap();
        let started = Instant::now();
        let result = wait_child_with_timeout(&mut child, Duration::from_millis(300));
        assert!(result.is_err(), "超时应返回错误而非等待进程自然退出");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "超时后应立即 kill（实际耗时 {:?}）",
            started.elapsed()
        );
    }

    #[test]
    fn wait_child_with_timeout_returns_when_process_exits() {
        #[cfg(windows)]
        let mut child = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "exit", "0"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let mut child = std::process::Command::new("true").spawn().unwrap();

        wait_child_with_timeout(&mut child, Duration::from_secs(30)).unwrap();
    }
}
