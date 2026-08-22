fn main() {
    println!("cargo:rerun-if-env-changed=IPASTE_OCR_R2_BASE_URL");
    println!("cargo:rerun-if-env-changed=IPASTE_UPDATER_R2_ENDPOINT");
    let updater_r2_endpoint = std::env::var("IPASTE_UPDATER_R2_ENDPOINT").unwrap_or_default();
    let ocr_r2_base_url = std::env::var("IPASTE_OCR_R2_BASE_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| derive_ocr_r2_base_url(&updater_r2_endpoint).unwrap_or_default());
    println!(
        "cargo:rustc-env=IPASTE_OCR_R2_BASE_URL={}",
        ocr_r2_base_url
    );
    println!(
        "cargo:rustc-env=IPASTE_UPDATER_R2_ENDPOINT={}",
        updater_r2_endpoint
    );
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-lib=framework=Vision");
    }
    ensure_mocr_sidecar_placeholder();
    tauri_build::build()
}

/// externalBin 门禁：tauri-build 对 binaries/mocr_engine-<triple> 只做存在性
/// 检查，但真实 sidecar 由 beforeBuildCommand（npm run build:mocr-engine）在
/// tauri build 前放置。为了让 cargo check/test 等直接 cargo 命令在 sidecar
/// 未构建时也能通过，这里先落一个占位文件；真实构建会将其覆盖。
/// externalBin 仅在 tauri.windows.conf.json 声明（macOS 暂不分发 sidecar），
/// 因此占位也只在 Windows 目标生成，避免 mac 构建产物携带无用文件。
fn ensure_mocr_sidecar_placeholder() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.is_empty() || !target.contains("windows") {
        return;
    }
    let ext = if target.contains("windows") { ".exe" } else { "" };
    let path = std::path::Path::new("binaries").join(format!("mocr_engine-{target}{ext}"));
    if !path.exists() {
        let _ = std::fs::create_dir_all("binaries");
        let _ = std::fs::write(&path, b"placeholder: run npm run build:mocr-engine");
    }
}

fn derive_ocr_r2_base_url(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim();
    if !endpoint.starts_with("https://") {
        return None;
    }

    let endpoint = endpoint
        .split(['?', '#'])
        .next()
        .unwrap_or(endpoint)
        .trim_end_matches('/');
    let parent_index = endpoint.rfind('/')?;
    let parent = &endpoint[..parent_index];
    if parent.len() <= "https://".len() {
        return None;
    }

    Some(format!("{parent}/ocr/"))
}
