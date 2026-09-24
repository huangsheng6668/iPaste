// commands/automations.rs — 快捷动作 CRUD 与执行
use crate::error::AppError;
use crate::models::{
    AppState, AutomationAction, AutomationInput, AutomationRunDetail, AutomationRunSummary,
};

#[tauri::command]
pub(crate) fn list_automations(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<AutomationAction>, AppError> {
    state.store.list_automations().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn create_automation(
    state: tauri::State<'_, AppState>,
    input: AutomationInput,
) -> Result<AutomationAction, AppError> {
    state.store.create_automation(input).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_automation(
    state: tauri::State<'_, AppState>,
    id: String,
    input: AutomationInput,
) -> Result<AutomationAction, AppError> {
    state
        .store
        .update_automation(&id, input)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn delete_automation(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.store.delete_automation(&id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) async fn run_automation(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<AutomationRunSummary, AppError> {
    let conn = state.store.connect()?;
    let action = state.store.get_automation_with_conn(&conn, &id)?;
    let store = state.store.clone();
    tauri::async_runtime::spawn(async move {
        crate::automation::execute_automation(app, &store, action).await
    })
    .await
    .map_err(|e| format!("任务失败: {e}"))?
    .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn get_automation_run(
    state: tauri::State<'_, AppState>,
    run_id: String,
) -> Result<AutomationRunDetail, AppError> {
    let conn = state.store.connect()?;
    state
        .store
        .get_automation_run_detail(&conn, &run_id)
        .map_err(AppError::from)
}
