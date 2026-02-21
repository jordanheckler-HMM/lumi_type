#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result};
use core::{
    permissions::{self, PermissionStatus},
    state::{EngineCommand, EngineEvent, TrayState},
    EngineHandle, EngineSettings,
};
use cpal::traits::{DeviceTrait, HostTrait};
use directories::ProjectDirs;
use parking_lot::RwLock;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

struct AppState {
    engine: EngineHandle,
    settings_path: PathBuf,
    push_to_talk_hotkey: Arc<RwLock<String>>,
}

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> EngineSettings {
    state.engine.settings()
}

#[tauri::command]
async fn update_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    next: EngineSettings,
) -> Result<(), String> {
    save_settings(&state.settings_path, &next).map_err(|err| err.to_string())?;
    {
        *state.push_to_talk_hotkey.write() = next.push_to_talk_hotkey.clone();
    }

    register_shortcuts(&app, &state.engine, &next.push_to_talk_hotkey)
        .map_err(|err| err.to_string())?;
    state.engine.apply_settings(next).await;
    Ok(())
}

#[tauri::command]
fn list_input_devices() -> Result<Vec<String>, String> {
    let devices = cpal::default_host()
        .input_devices()
        .map_err(|err| err.to_string())?
        .filter_map(|device| device.name().ok())
        .collect::<Vec<_>>();
    Ok(devices)
}

#[tauri::command]
fn request_permissions(state: tauri::State<'_, AppState>) -> Result<PermissionStatus, String> {
    let status = permissions::request_permissions();
    state
        .engine
        .send_blocking(EngineCommand::PermissionsChecked(status));
    Ok(status)
}

#[tauri::command]
fn open_settings_window(app: tauri::AppHandle) -> Result<(), String> {
    show_settings_window(&app).map_err(|err| err.to_string())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("failed to start LumiType: {error}");
    }
}

fn run() -> Result<()> {
    let settings_path = settings_path()?;
    let settings = load_settings(&settings_path)?;

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(move |app| {
            let app_handle = app.handle().clone();
            setup_tray(&app_handle)?;

            let model_root = detect_model_root(&app_handle);
            let engine = core::spawn_engine(settings.clone(), model_root)
                .context("failed to start core engine")?;

            let hotkey = Arc::new(RwLock::new(settings.push_to_talk_hotkey.clone()));
            let state = AppState {
                engine: engine.clone(),
                settings_path: settings_path.clone(),
                push_to_talk_hotkey: hotkey,
            };
            app.manage(state);

            register_shortcuts(&app_handle, &engine, &settings.push_to_talk_hotkey)
                .context("failed to register keyboard shortcuts")?;
            position_overlay_window(&app_handle).ok();

            let status = permissions::check_permissions();
            engine.send_blocking(EngineCommand::PermissionsChecked(status));

            wire_engine_events(app_handle, engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            list_input_devices,
            request_permissions,
            open_settings_window
        ])
        .run(tauri::generate_context!())
        .context("tauri app exited with error")
}

fn setup_tray(app: &tauri::AppHandle) -> Result<()> {
    let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&settings_item, &separator, &quit_item])?;

    let (rgba, width, height) = tray_icon_rgba(TrayState::Listening);
    let icon = Image::new_owned(rgba, width, height);

    TrayIconBuilder::with_id("lumitype-tray")
        .menu(&menu)
        .icon(icon)
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "settings" => {
                    let _ = show_settings_window(app);
                }
                "quit" => {
                    std::process::exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = show_settings_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn show_settings_window(app: &tauri::AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("settings") {
        window.show()?;
        window.set_focus()?;
    }
    Ok(())
}

fn position_overlay_window(app: &tauri::AppHandle) -> Result<()> {
    let Some(window) = app.get_webview_window("overlay") else {
        return Ok(());
    };

    let monitor = window
        .current_monitor()?
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return Ok(());
    };

    let size = monitor.size();
    let scale = monitor.scale_factor();

    let width = 420.0;
    let x = ((size.width as f64 / scale) - width) / 2.0;
    let y = 32.0;

    window.set_position(tauri::Position::Logical(tauri::LogicalPosition { x, y }))?;
    Ok(())
}

fn register_shortcuts(app: &tauri::AppHandle, engine: &EngineHandle, ptt_hotkey: &str) -> Result<()> {
    let shortcuts = app.global_shortcut();
    shortcuts.unregister_all()?;

    let ptt = normalize_shortcut(ptt_hotkey);
    let ptt_engine = engine.clone();
    shortcuts.on_shortcut(ptt.as_str(), move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            ptt_engine.send_blocking(EngineCommand::PushToTalkTriggered);
        } else if event.state == ShortcutState::Released {
            ptt_engine.send_blocking(EngineCommand::SilenceTimeout);
        }
    })?;

    let cancel_engine = engine.clone();
    shortcuts.on_shortcut("Escape", move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            cancel_engine.send_blocking(EngineCommand::CancelDictation);
        }
    })?;

    let undo_engine = engine.clone();
    shortcuts.on_shortcut("Command+Alt+Z", move |_app, _shortcut, event| {
        if event.state == ShortcutState::Pressed {
            undo_engine.send_blocking(EngineCommand::UndoLastDictation);
        }
    })?;

    Ok(())
}

fn normalize_shortcut(raw: &str) -> String {
    raw.replace("Cmd", "Command")
        .replace("Option", "Alt")
        .replace("Esc", "Escape")
}

fn wire_engine_events(app: tauri::AppHandle, engine: EngineHandle) {
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        while let Ok(event) = rx.recv().await {
            match event {
                EngineEvent::StateChanged(state) => {
                    let _ = app.emit("engine-state", state);
                }
                EngineEvent::TrayStateChanged(state) => {
                    let _ = set_tray_icon(&app, state);
                }
                EngineEvent::OverlayVisibility(visible) => {
                    if let Some(window) = app.get_webview_window("overlay") {
                        if visible {
                            let _ = position_overlay_window(&app);
                            let _ = window.show();
                            let _ = window.emit("overlay-show", ());
                        } else {
                            let _ = window.emit("overlay-hide", ());
                            let cloned = window.clone();
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(Duration::from_millis(220)).await;
                                let _ = cloned.hide();
                            });
                        }
                    }
                }
                EngineEvent::OverlayReset => {
                    if let Some(window) = app.get_webview_window("overlay") {
                        let _ = window.emit("overlay-reset", ());
                    }
                }
                EngineEvent::OverlayTextDelta(delta) => {
                    if let Some(window) = app.get_webview_window("overlay") {
                        let _ = window.emit("overlay-text", delta);
                    }
                }
                EngineEvent::OverlayWave(level) => {
                    if let Some(window) = app.get_webview_window("overlay") {
                        let _ = window.emit("overlay-wave", level);
                    }
                }
                EngineEvent::PermissionsRequired(status) => {
                    let _ = app.emit("permissions-required", status);
                    let _ = show_settings_window(&app);
                }
                EngineEvent::Error(message) => {
                    let _ = app.emit("engine-error", message);
                }
            }
        }
    });
}

fn set_tray_icon(app: &tauri::AppHandle, state: TrayState) -> Result<()> {
    let Some(tray) = app.tray_by_id("lumitype-tray") else {
        return Ok(());
    };

    let (rgba, width, height) = tray_icon_rgba(state);
    tray.set_icon(Some(Image::new_owned(rgba, width, height)))?;
    Ok(())
}

fn tray_icon_rgba(state: TrayState) -> (Vec<u8>, u32, u32) {
    let width = 18u32;
    let height = 18u32;
    let mut rgba = vec![0u8; (width * height * 4) as usize];

    let center = (width as f32 - 1.0) / 2.0;
    let radius = 6.5f32;

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let distance = (dx * dx + dy * dy).sqrt();
            let idx = ((y * width + x) * 4) as usize;

            let alpha = match state {
                TrayState::Idle => {
                    let edge = (distance - radius).abs();
                    if edge <= 0.9 {
                        220u8
                    } else {
                        0u8
                    }
                }
                TrayState::Listening => {
                    if distance <= radius && distance >= radius - 2.1 {
                        180u8
                    } else if distance < radius - 2.1 {
                        45u8
                    } else {
                        0u8
                    }
                }
                TrayState::Dictating => {
                    if distance <= radius {
                        255u8
                    } else {
                        0u8
                    }
                }
            };

            rgba[idx] = 255;
            rgba[idx + 1] = 255;
            rgba[idx + 2] = 255;
            rgba[idx + 3] = alpha;
        }
    }

    (rgba, width, height)
}

fn settings_path() -> Result<PathBuf> {
    let dirs =
        ProjectDirs::from("com", "LumiType", "LumiType").context("unable to resolve config directory")?;
    let path = dirs.config_dir().join("settings.json");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create settings directory")?;
    }
    Ok(path)
}

fn load_settings(path: &Path) -> Result<EngineSettings> {
    if !path.exists() {
        let default = EngineSettings::default();
        save_settings(path, &default)?;
        return Ok(default);
    }

    let content = fs::read_to_string(path).context("failed to read settings file")?;
    let settings =
        serde_json::from_str::<EngineSettings>(&content).context("failed to parse settings file")?;
    Ok(settings)
}

fn save_settings(path: &Path, settings: &EngineSettings) -> Result<()> {
    let content = serde_json::to_string_pretty(settings).context("failed to encode settings")?;
    fs::write(path, content).context("failed to persist settings")?;
    Ok(())
}

fn detect_model_root(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(path) = std::env::var("LUMI_MODEL_DIR") {
        return PathBuf::from(path);
    }

    if let Ok(resource_dir) = app.path().resource_dir() {
        let bundled = resource_dir.join("models");
        if bundled.exists() {
            return bundled;
        }
    }

    let local = PathBuf::from("src-tauri/models");
    if local.exists() {
        return local;
    }

    PathBuf::from("models")
}
