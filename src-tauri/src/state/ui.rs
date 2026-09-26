//! 托盘菜单句柄：AppState 上原本 6 个平铺的 MenuItem 归到一处，
//! 托盘文案更新与语言切换统一经此取句柄。

use tauri::menu::MenuItem;
use tauri::Wry;

/// 托盘菜单项句柄（Task 27 聚合）。
pub struct UiHandles {
    pub show_menu_item: MenuItem<Wry>,
    pub append_copy_menu_item: MenuItem<Wry>,
    pub pause_capture_menu_item: MenuItem<Wry>,
    pub settings_menu_item: MenuItem<Wry>,
    pub quit_menu_item: MenuItem<Wry>,
    pub ocr_menu_item: MenuItem<Wry>,
}
