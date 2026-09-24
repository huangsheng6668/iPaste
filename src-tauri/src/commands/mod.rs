// commands/ — 向 UI 暴露模块函数的薄 Tauri 命令层，按域拆分子模块。
// 子模块以 pub(crate) 暴露，lib.rs 的 generate_handler! 用显式全路径注册
// （crate 内不做 glob 再导出）。
pub(crate) mod automations;
pub(crate) mod categories;
pub(crate) mod clips;
pub(crate) mod cloud;
pub(crate) mod ocr;
pub(crate) mod screenshot;
pub(crate) mod settings;
pub(crate) mod window_cmds;

use crate::error::AppError;
use crate::models::{AppSettings, AppSnapshot, AppState};
use crate::shortcut::emit_settings_changed;

/// settings 更新命令统一收尾：落库 → 广播设置变更。多处样板收敛于此。
/// store 侧设置方法返回 `Result<AppSettings, String>`，这里以泛型错误吸收
/// （`String` 与 `AppError` 均可 Into<AppError>）。
pub(crate) fn apply_settings_update<E>(
    app: &tauri::AppHandle,
    result: Result<AppSettings, E>,
) -> Result<AppSettings, AppError>
where
    E: Into<AppError>,
{
    let settings = result.map_err(Into::into)?;
    emit_settings_changed(app, &settings);
    Ok(settings)
}

/// get_snapshot 与 sync_cloud_now 共用的 AppSnapshot 组装
/// （原两处逐行重复约 22 行）。prune_expired 统一在读取前执行。
pub(crate) fn build_app_snapshot(
    state: tauri::State<'_, AppState>,
) -> Result<AppSnapshot, AppError> {
    state.store.prune_expired()?;
    let (clip_page, categories, category_items) = state.store.snapshot()?;
    let settings = state.store.settings()?;
    let is_listening = *state
        .is_listening
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?;
    let is_append_copy_enabled = state
        .append_copy_state
        .lock()
        .map_err(|error| AppError::internal(error.to_string()))?
        .is_enabled;
    Ok(AppSnapshot {
        clips: clip_page.clips,
        has_more_clips: clip_page.has_more,
        clip_total_count: clip_page.all_count,
        categories,
        category_items,
        shortcut: settings.shortcut.clone(),
        is_listening,
        is_append_copy_enabled,
        settings,
    })
}
