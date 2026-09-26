//! 设置项清单（Task 35/36）：键名、清洗器与默认值的**唯一权威来源**。
//!
//! 读取（`settings_with_conn`）与写入（各 `update_*`）都从这里取规格：
//! - 写入：`spec.clean(value)` 不通过即拒绝；通过则落库；
//! - 读取：清洗失败一律回落 `spec.default`（防御历史脏数据）。
//!
//! 新增一个字符串型设置项只需在此登记一行，再在 `AppSettings` 与命令层补字段。
//! 数值型设置项（retention_days / append_copy_timeout_minutes）与云设置不在本表——
//! 前者走 i64 清洗且带额外副作用（过期清理），后者由 keyring 参与、结构不同。

use crate::util::{
    clean_language, clean_ocr_engine, clean_ocr_mode, clean_panel_layout,
    clean_panel_open_behavior, clean_shortcut,
};
use crate::{
    DEFAULT_LANGUAGE, DEFAULT_OCR_ENGINE, DEFAULT_OCR_MODE, DEFAULT_PANEL_LAYOUT,
    DEFAULT_PANEL_OPEN_BEHAVIOR, DEFAULT_SHORTCUT,
};

/// 字符串型设置项规格。
pub(crate) struct StringSettingSpec {
    pub key: &'static str,
    /// 清洗器：Err 表示值非法（写入拒绝、读取回落默认）。
    pub clean: fn(String) -> Result<String, String>,
    pub default: &'static str,
}

/// 快捷键：面板唤出键。
pub(crate) const SHORTCUT: StringSettingSpec = StringSettingSpec {
    key: "shortcut",
    clean: clean_shortcut,
    default: DEFAULT_SHORTCUT,
};

/// 快捷键：截图 OCR。
pub(crate) const OCR_SHORTCUT: StringSettingSpec = StringSettingSpec {
    key: "ocr_shortcut",
    clean: clean_shortcut,
    default: crate::DEFAULT_OCR_SHORTCUT,
};

pub(crate) const PANEL_OPEN_BEHAVIOR: StringSettingSpec = StringSettingSpec {
    key: "panel_open_behavior",
    clean: clean_panel_open_behavior,
    default: DEFAULT_PANEL_OPEN_BEHAVIOR,
};

pub(crate) const PANEL_LAYOUT: StringSettingSpec = StringSettingSpec {
    key: "panel_layout",
    clean: clean_panel_layout,
    default: DEFAULT_PANEL_LAYOUT,
};

pub(crate) const OCR_MODE: StringSettingSpec = StringSettingSpec {
    key: "ocr_mode",
    clean: clean_ocr_mode,
    default: DEFAULT_OCR_MODE,
};

pub(crate) const OCR_ENGINE: StringSettingSpec = StringSettingSpec {
    key: "ocr_engine",
    clean: clean_ocr_engine,
    default: DEFAULT_OCR_ENGINE,
};

pub(crate) const LANGUAGE: StringSettingSpec = StringSettingSpec {
    key: "language",
    clean: clean_language,
    default: DEFAULT_LANGUAGE,
};

/// 全部字符串型设置项（读取路径按此表组装）。
pub(crate) const STRING_SETTINGS: &[&StringSettingSpec] = &[
    &SHORTCUT,
    &OCR_SHORTCUT,
    &PANEL_OPEN_BEHAVIOR,
    &PANEL_LAYOUT,
    &OCR_MODE,
    &OCR_ENGINE,
    &LANGUAGE,
];
