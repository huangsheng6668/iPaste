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

#[tauri::command]
pub(crate) fn test_cloud_settings(api_address: String, api_key: String) -> Result<bool, AppError> {
    let api_address = clean_api_address(api_address)?;
    let api_key = clean_api_key(api_key)?;
    test_cloud_connection(&api_address, &api_key)?;
    Ok(true)
}

#[tauri::command]
pub(crate) fn sync_cloud_now(state: tauri::State<'_, AppState>) -> Result<AppSnapshot, AppError> {
    state.store.sync_cloud()?;
    build_app_snapshot(state)
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
