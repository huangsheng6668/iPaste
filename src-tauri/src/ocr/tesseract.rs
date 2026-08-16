use std::path::PathBuf;
use std::process::Command;

use super::installer::{ocr_asset_dir, ocr_engine_dir};
use crate::models::{ImageOcrResult, ImageOcrWord};

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

    let output = Command::new(&tesseract)
        .arg(&image_path)
        .arg("stdout")
        .arg("-l")
        .arg("chi_sim+eng")
        .arg("--tessdata-dir")
        .arg(&tessdata_dir)
        .arg("-c")
        .arg("tessedit_create_tsv=1")
        .output()
        .map_err(|error| format!("无法启动 Tesseract：{error}"))?;

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
