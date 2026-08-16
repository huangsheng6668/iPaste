use std::{
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter,
};

use crate::events::{AppendCopyChanged, EVENT_APPEND_COPY_CHANGED, EVENT_LISTENING_CHANGED, ListeningChanged};
use crate::models::{AppendCopyState, AppState, MainWindowActivation};
use crate::window::show_settings_window;
use crate::{util::{localized_text, new_id}, window::show_main_window, DEFAULT_LANGUAGE};
pub(crate) fn update_pause_capture_menu_label(state: &AppState, is_listening: bool) {
    let language = state
        .store
        .settings()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| DEFAULT_LANGUAGE.to_string());
    let label = localized_text(
        &language,
        if is_listening {
            "pause_capture"
        } else {
            "resume_capture"
        },
    );
    let _ = state.pause_capture_menu_item.set_text(label);
}

pub(crate) fn set_append_copy_enabled_inner(
    app: &tauri::AppHandle,
    state: &AppState,
    enabled: bool,
) -> Result<bool, String> {
    let (is_enabled, timer_session_id) = {
        let mut append_copy = state
            .append_copy_state
            .lock()
            .map_err(|error| error.to_string())?;
        let mut timer_session_id = None;

        if append_copy.is_enabled != enabled {
            append_copy.is_enabled = enabled;
            append_copy.clip_id = None;
            append_copy.text.clear();
            append_copy.session_id = enabled.then(new_id);
            timer_session_id = append_copy.session_id.clone();
        }

        (append_copy.is_enabled, timer_session_id)
    };

    update_append_copy_menu_label(state, is_enabled);
    let _ = app.emit(
        EVENT_APPEND_COPY_CHANGED,
        AppendCopyChanged { is_enabled },
    );
    if let Some(session_id) = timer_session_id {
        let settings = state.store.settings()?;
        let timeout = Duration::from_secs(settings.append_copy_timeout_minutes.max(1) as u64 * 60);
        spawn_append_copy_timeout(
            app.clone(),
            state.append_copy_state.clone(),
            state.append_copy_menu_item.clone(),
            session_id,
            timeout,
            settings.language,
        );
    }
    Ok(is_enabled)
}

fn update_append_copy_menu_label(state: &AppState, is_enabled: bool) {
    let language = state
        .store
        .settings()
        .map(|settings| settings.language)
        .unwrap_or_else(|_| DEFAULT_LANGUAGE.to_string());
    let label = localized_text(
        &language,
        if is_enabled {
            "disable_append_copy"
        } else {
            "enable_append_copy"
        },
    );
    let _ = state.append_copy_menu_item.set_text(label);
}

fn spawn_append_copy_timeout(
    app: tauri::AppHandle,
    append_copy_state: Arc<Mutex<AppendCopyState>>,
    append_copy_menu_item: MenuItem<tauri::Wry>,
    session_id: String,
    timeout: Duration,
    language: String,
) {
    thread::spawn(move || {
        thread::sleep(timeout);
        let should_emit = append_copy_state
            .lock()
            .map(|mut append_copy| {
                if !append_copy.is_enabled
                    || append_copy.session_id.as_deref() != Some(session_id.as_str())
                {
                    return false;
                }

                append_copy.is_enabled = false;
                append_copy.clip_id = None;
                append_copy.session_id = None;
                append_copy.text.clear();
                true
            })
            .unwrap_or(false);

        if !should_emit {
            return;
        }

        let _ = append_copy_menu_item.set_text(localized_text(&language, "enable_append_copy"));
        let _ = app.emit(
            EVENT_APPEND_COPY_CHANGED,
            AppendCopyChanged { is_enabled: false },
        );
    });
}

pub(crate) fn build_tray(
    app: &tauri::AppHandle,
    show: MenuItem<tauri::Wry>,
    append_copy: MenuItem<tauri::Wry>,
    pause: MenuItem<tauri::Wry>,
    settings: MenuItem<tauri::Wry>,
    quit: MenuItem<tauri::Wry>,
    language: &str,
) -> tauri::Result<()> {
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&show, &append_copy, &settings, &pause, &separator, &quit],
    )?;

    let mut tray = TrayIconBuilder::with_id("ipaste")
        .tooltip(localized_text(language, "tray_tooltip"))
        .menu(&menu)
        .icon_as_template(false)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_main_window(tray.app_handle(), MainWindowActivation::Activate);
            }
        });

    if let Some(icon) = tray_icon().or_else(|| app.default_window_icon().cloned()) {
        tray = tray.icon(icon.clone());
    }

    tray.build(app)?;
    Ok(())
}

fn tray_icon() -> Option<tauri::image::Image<'static>> {
    #[cfg(target_os = "windows")]
    let bytes = include_bytes!("../icons/tray-icon-windows.png");

    #[cfg(not(target_os = "windows"))]
    let bytes = include_bytes!("../icons/tray-icon.png");

    let image = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    Some(tauri::image::Image::new_owned(
        image.into_raw(),
        width,
        height,
    ))
}

pub(crate) fn apply_tray_language(state: &AppState, language: &str) {
    let _ = state
        .show_menu_item
        .set_text(localized_text(language, "open_ipaste"));
    let _ = state
        .settings_menu_item
        .set_text(localized_text(language, "settings"));
    let _ = state
        .quit_menu_item
        .set_text(localized_text(language, "quit_ipaste"));

    let is_append_copy_enabled = state
        .append_copy_state
        .lock()
        .map(|append_copy| append_copy.is_enabled)
        .unwrap_or(false);
    let _ = state.append_copy_menu_item.set_text(localized_text(
        language,
        if is_append_copy_enabled {
            "disable_append_copy"
        } else {
            "enable_append_copy"
        },
    ));

    let is_listening = state
        .is_listening
        .lock()
        .map(|listening| *listening)
        .unwrap_or(true);
    let _ = state.pause_capture_menu_item.set_text(localized_text(
        language,
        if is_listening {
            "pause_capture"
        } else {
            "resume_capture"
        },
    ));
}

pub(crate) fn handle_show_menu(app: &tauri::AppHandle) {
    let _ = show_main_window(app, MainWindowActivation::Activate);
}

pub(crate) fn handle_settings_menu(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = show_settings_window(&app);
    });
}

pub(crate) fn handle_append_copy_menu(app: &tauri::AppHandle, state: &AppState) {
    let enabled = state
        .append_copy_state
        .lock()
        .map(|value| !value.is_enabled)
        .unwrap_or(true);
    let _ = set_append_copy_enabled_inner(app, state, enabled);
}

pub(crate) fn handle_pause_capture_menu(app: &tauri::AppHandle, state: &AppState) {
    if let Ok(mut listening) = state.is_listening.lock() {
        *listening = !*listening;
        update_pause_capture_menu_label(state, *listening);
        let _ = app.emit(
            EVENT_LISTENING_CHANGED,
            ListeningChanged {
                is_listening: *listening,
            },
        );
    }
}

