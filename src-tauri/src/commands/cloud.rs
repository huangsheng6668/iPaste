// commands/cloud.rs — 自托管云同步设置/测试/触发
use std::thread;

use crate::cloud::test_cloud_connection;
use crate::error::AppError;
use crate::models::{AppSettings, AppSnapshot, AppState};
use crate::util::{clean_api_address, clean_api_key};

use super::{apply_settings_update, build_app_snapshot};

#[tauri::command]
pub(crate) fn update_cloud_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    api_address: String,
    api_key: String,
) -> Result<AppSettings, AppError> {
    apply_settings_update(
        &app,
        state.store.update_cloud_settings(api_address, api_key),
    )
}

#[tauri::command]
pub(crate) fn disable_cloud_sync(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<AppSettings, AppError> {
    apply_settings_update(&app, state.store.disable_cloud_sync())
}

// 云同步/连接测试都是阻塞式网络调用（reqwest::blocking）：同步命令跑在主线程，
// 整次 HTTP 往返会把窗口冻住——改 spawn_blocking。
#[tauri::command]
pub(crate) async fn test_cloud_settings(
    api_address: String,
    api_key: String,
) -> Result<bool, AppError> {
    let api_address = clean_api_address(api_address)?;
    // 掩码哨兵 = 输入框未改动：用存储的真实 Key 测试。
    let api_key = if crate::util::is_masked_secret(&api_key) {
        match crate::store::secrets::get_api_key() {
            Ok(Some(stored)) if !stored.trim().is_empty() => stored,
            _ => return Err(AppError::internal("云同步 API Key 未配置")),
        }
    } else {
        clean_api_key(api_key)?
    };
    tokio::task::spawn_blocking(move || {
        test_cloud_connection(&api_address, &api_key)?;
        Ok(true)
    })
    .await
    .map_err(|error| AppError::internal(format!("连接测试任务失败：{error}")))?
}

#[tauri::command]
pub(crate) async fn sync_cloud_now(
    state: tauri::State<'_, AppState>,
) -> Result<AppSnapshot, AppError> {
    let store = state.store.clone();
    let is_listening = state.capture.is_listening.clone();
    let append_copy_state = state.capture.append_copy_state.clone();
    tokio::task::spawn_blocking(move || {
        store.sync_cloud()?;
        build_app_snapshot(&store, &is_listening, &append_copy_state)
    })
    .await
    .map_err(|error| AppError::internal(format!("云同步任务失败：{error}")))?
}

#[tauri::command]
pub(crate) fn sync_cloud_in_background(state: tauri::State<'_, AppState>) -> Result<(), AppError> {
    let store = state.store.clone();
    thread::spawn(move || {
        if let Err(error) = store.sync_cloud() {
            eprintln!("background cloud sync failed: {error}");
        }
    });
    Ok(())
}
