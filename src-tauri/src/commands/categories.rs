// commands/categories.rs — 分类与分类条目 CRUD/排序
use crate::error::AppError;
use crate::models::{AppState, Category, CategoryItem, CategoryWithItem};

#[tauri::command]
pub(crate) fn list_categories(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<Category>, AppError> {
    state.store.list_categories().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn list_category_items(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<CategoryItem>, AppError> {
    state.store.list_category_items().map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn reorder_categories(
    state: tauri::State<'_, AppState>,
    category_ids: Vec<String>,
) -> Result<Vec<Category>, AppError> {
    state
        .store
        .reorder_categories(category_ids)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn reorder_category_items(
    state: tauri::State<'_, AppState>,
    category_id: String,
    item_ids: Vec<String>,
) -> Result<Vec<CategoryItem>, AppError> {
    state
        .store
        .reorder_category_items(category_id, item_ids)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn create_category(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
) -> Result<Category, AppError> {
    state
        .store
        .create_category(name, color)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn create_category_with_clip(
    state: tauri::State<'_, AppState>,
    name: String,
    color: String,
    clip_id: String,
) -> Result<CategoryWithItem, AppError> {
    state
        .store
        .create_category_with_clip(name, color, clip_id)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn update_category(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    color: String,
) -> Result<Category, AppError> {
    state
        .store
        .update_category(id, name, color)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn delete_category(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.store.delete_category(id).map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn add_clip_to_category(
    state: tauri::State<'_, AppState>,
    clip_id: String,
    category_id: String,
) -> Result<CategoryItem, AppError> {
    state
        .store
        .add_clip_to_category(clip_id, category_id)
        .map_err(AppError::from)
}

#[tauri::command]
pub(crate) fn remove_category_item(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.store.remove_category_item(id).map_err(AppError::from)
}
