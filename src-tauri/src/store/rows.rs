//! SQLite 行 → 领域模型的映射函数。列序与各查询的 SELECT 顺序一一对应，
//! 搬家自 lib.rs（原 crate root 杂项）。

use crate::models::{
    AutomationAction, AutomationRunDetail, AutomationRunSummary, Category, CategoryItem, ClipItem,
};

// collect_rows / map_clip / map_category / map_category_item / map_automation /
// map_automation_run_summary / map_automation_run_detail —— 从 lib.rs 逐字搬入，
// 签名与实现不变。

pub(crate) fn collect_rows<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>>,
) -> Result<Vec<T>, String> {
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}

pub(crate) fn map_clip(row: &rusqlite::Row<'_>) -> rusqlite::Result<ClipItem> {
    Ok(ClipItem {
        id: row.get(0)?,
        clip_type: row.get(1)?,
        content_hash: row.get(2)?,
        display_name: row.get(3)?,
        preview_text: row.get(4)?,
        text: row.get(5)?,
        source_app: row.get(6)?,
        last_captured_at: row.get(7)?,
        favorite_count: row.get(8)?,
        is_pinned: row.get(9)?,
    })
}

pub(crate) fn map_category(row: &rusqlite::Row<'_>) -> rusqlite::Result<Category> {
    Ok(Category {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

pub(crate) fn map_category_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<CategoryItem> {
    Ok(CategoryItem {
        id: row.get(0)?,
        category_id: row.get(1)?,
        clip_snapshot_id: row.get(2)?,
        clip_type: row.get(3)?,
        content_hash: row.get(4)?,
        display_name: row.get(5)?,
        preview_text: row.get(6)?,
        text: row.get(7)?,
        sort_order: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        sync_state: row.get(11)?,
        is_pinned: row.get(12)?,
    })
}

pub(crate) fn map_automation(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationAction> {
    Ok(AutomationAction {
        id: row.get(0)?,
        name: row.get(1)?,
        command: row.get(2)?,
        cwd: row.get(3)?,
        run_mode: row.get(4)?,
        confirm_before_run: row.get(5)?,
        close_panel_on_success: row.get(6)?,
        sort_order: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        last_run: None,
    })
}

pub(crate) fn map_automation_run_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRunSummary> {
    Ok(AutomationRunSummary {
        id: row.get(0)?,
        status: row.get(1)?,
        exit_code: row.get(2)?,
        started_at: row.get(3)?,
        finished_at: row.get(4)?,
        duration_ms: row.get(5)?,
    })
}

pub(crate) fn map_automation_run_detail(row: &rusqlite::Row<'_>) -> rusqlite::Result<AutomationRunDetail> {
    Ok(AutomationRunDetail {
        id: row.get(0)?,
        automation_id: row.get(1)?,
        status: row.get(2)?,
        exit_code: row.get(3)?,
        stdout: row.get(4)?,
        stderr: row.get(5)?,
        stdout_truncated: row.get(6)?,
        stderr_truncated: row.get(7)?,
        started_at: row.get(8)?,
        finished_at: row.get(9)?,
        duration_ms: row.get(10)?,
    })
}
