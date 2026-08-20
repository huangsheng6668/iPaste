use tauri::Emitter;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::events::{EVENT_SETTINGS_CHANGED, SettingsChanged};
use crate::models::{AppSettings, AppState};

pub(crate) fn shortcut_matches(shortcut: &Shortcut, shortcut_spec: &str) -> bool {
    shortcut_spec
        .parse::<Shortcut>()
        .map(|expected| shortcut.id() == expected.id())
        .unwrap_or(false)
}

fn shortcut_matches_spec(candidate: &str, other: &str) -> bool {
    candidate
        .parse::<Shortcut>()
        .ok()
        .zip(other.parse::<Shortcut>().ok())
        .map(|(a, b)| a.id() == b.id())
        .unwrap_or(candidate == other)
}

/// 两个全局快捷键不允许同值：handler 只会命中先匹配的分支，另一个会静默失效。
pub(crate) fn ensure_shortcut_not_conflicting(candidate: &str, other: &str) -> Result<(), String> {
    if shortcut_matches_spec(candidate, other) {
        return Err("快捷键与另一个全局快捷键冲突，请换一个组合".to_string());
    }
    Ok(())
}

pub(crate) fn register_app_shortcut(app: &tauri::AppHandle, shortcut: &str) -> Result<(), String> {
    if app.global_shortcut().is_registered(shortcut) {
        return Ok(());
    }

    app.global_shortcut()
        .register(shortcut)
        .map_err(|error| shortcut_registration_error(shortcut, error))
}

fn unregister_app_shortcut(app: &tauri::AppHandle, shortcut: &str) -> Result<(), String> {
    if !app.global_shortcut().is_registered(shortcut) {
        return Ok(());
    }

    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|error| error.to_string())
}

pub(crate) fn set_app_shortcut_enabled_inner(
    app: &tauri::AppHandle,
    state: &AppState,
    enabled: bool,
) -> Result<bool, String> {
    // 与 update_registered_app_shortcut 同理：注册/反注册不能持锁
    let (panel_shortcut, ocr_shortcut) = {
        let panel = state
            .active_shortcut
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        let ocr = state
            .active_ocr_shortcut
            .lock()
            .map_err(|error| error.to_string())?
            .clone();
        (panel, ocr)
    };

    if enabled {
        register_app_shortcut(app, &panel_shortcut)?;
        register_app_shortcut(app, &ocr_shortcut)?;
    } else {
        unregister_app_shortcut(app, panel_shortcut.as_str())?;
        unregister_app_shortcut(app, ocr_shortcut.as_str())?;
    }

    *state
        .is_app_shortcut_enabled
        .lock()
        .map_err(|error| error.to_string())? = enabled;
    Ok(enabled)
}

pub(crate) fn update_registered_app_shortcut(
    app: &tauri::AppHandle,
    state: &AppState,
    shortcut: &str,
) -> Result<(), String> {
    // 注册/反注册不能在 active_shortcut 锁内进行：快捷键事件 handler 在主线程
    // 锁同一把锁，而插件注册接口可能同步等待主线程，持锁等待会互相死锁。
    let previous = state
        .active_shortcut
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    if previous == shortcut {
        if is_app_shortcut_enabled(state)? && !app.global_shortcut().is_registered(shortcut) {
            register_app_shortcut(app, shortcut)?;
        }
        state
            .show_menu_item
            .set_accelerator(Some(shortcut))
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let was_enabled = is_app_shortcut_enabled(state)?;
    unregister_app_shortcut(app, previous.as_str())?;

    if was_enabled {
        if let Err(error) = register_app_shortcut(app, shortcut) {
            let _ = register_app_shortcut(app, &previous);
            return Err(error);
        }
    }

    if let Err(error) = state.show_menu_item.set_accelerator(Some(shortcut)) {
        let _ = app.global_shortcut().unregister(shortcut);
        if was_enabled {
            let _ = register_app_shortcut(app, &previous);
        }
        return Err(error.to_string());
    }

    *state
        .active_shortcut
        .lock()
        .map_err(|error| error.to_string())? = shortcut.to_string();
    Ok(())
}

pub(crate) fn update_registered_ocr_shortcut(
    app: &tauri::AppHandle,
    state: &AppState,
    shortcut: &str,
) -> Result<(), String> {
    // 注册/反注册不能在 active_ocr_shortcut 锁内进行（同面板键的死锁约束）
    let previous = state
        .active_ocr_shortcut
        .lock()
        .map_err(|error| error.to_string())?
        .clone();

    if previous == shortcut {
        if is_app_shortcut_enabled(state)? && !app.global_shortcut().is_registered(shortcut) {
            register_app_shortcut(app, shortcut)?;
        }
        state
            .ocr_menu_item
            .set_accelerator(Some(shortcut))
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let was_enabled = is_app_shortcut_enabled(state)?;
    unregister_app_shortcut(app, previous.as_str())?;

    if was_enabled {
        if let Err(error) = register_app_shortcut(app, shortcut) {
            let _ = register_app_shortcut(app, &previous);
            return Err(error);
        }
    }

    if let Err(error) = state.ocr_menu_item.set_accelerator(Some(shortcut)) {
        let _ = app.global_shortcut().unregister(shortcut);
        if was_enabled {
            let _ = register_app_shortcut(app, &previous);
        }
        return Err(error.to_string());
    }

    *state
        .active_ocr_shortcut
        .lock()
        .map_err(|error| error.to_string())? = shortcut.to_string();
    Ok(())
}

fn is_app_shortcut_enabled(state: &AppState) -> Result<bool, String> {
    state
        .is_app_shortcut_enabled
        .lock()
        .map(|value| *value)
        .map_err(|error| error.to_string())
}

fn shortcut_registration_error(shortcut: &str, error: impl ToString) -> String {
    format!(
        "无法注册快捷键 {shortcut}：{}。请换一个未被系统或其他应用占用的组合。",
        error.to_string()
    )
}

pub(crate) fn emit_settings_changed(app: &tauri::AppHandle, settings: &AppSettings) {
    let _ = app.emit(
        EVENT_SETTINGS_CHANGED,
        SettingsChanged {
            settings: settings.clone(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::ensure_shortcut_not_conflicting;

    #[test]
    fn conflicting_shortcuts_are_rejected() {
        // CommandOrControl 在 Windows 解析为 Ctrl：与 Ctrl+Shift+V 同 id
        assert!(ensure_shortcut_not_conflicting("CommandOrControl+Shift+V", "Ctrl+Shift+V").is_err());
        assert!(ensure_shortcut_not_conflicting("Ctrl+Shift+O", "CommandOrControl+Shift+O").is_err());
        assert!(ensure_shortcut_not_conflicting("CommandOrControl+Shift+V", "CommandOrControl+Shift+O").is_ok());
        assert!(ensure_shortcut_not_conflicting("Alt+S", "Alt+D").is_ok());
    }
}
