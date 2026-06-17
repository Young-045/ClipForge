use super::cursor;
use super::AppState;
use tauri::{AppHandle, Manager, PhysicalPosition, State, WebviewWindow};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const DEFAULT_SHORTCUT: &str = "CmdOrCtrl+Shift+V";
pub const SHORTCUT_KEY: &str = "shortcut";

// ── Normalize ───────────────────────────────────────────────────────

/// Normalize a user-facing shortcut (e.g. "Control+Shift+V") to plugin format.
pub fn normalize(raw: &str) -> String {
    raw.trim()
        .replace("Control+", "CmdOrCtrl+")
        .replace("CommandOrControl+", "CmdOrCtrl+")
        .replace("CmdOrControl+", "CmdOrCtrl+")
}

// ── Registration ────────────────────────────────────────────────────

/// Try to place the window near the global cursor without covering the
/// input field the user is typing in.  Falls back to centering.
fn position_near_cursor(window: &WebviewWindow) -> Result<(), String> {
    let (cx, cy) = cursor::get_cursor_position().ok_or("无法获取光标位置".to_string())?;

    let ws = window
        .outer_size()
        .map_err(|e| format!("获取窗口尺寸失败: {}", e))?;
    let ww = ws.width as i32;
    let wh = ws.height as i32;

    // Find the monitor that contains the cursor
    let monitors = window
        .available_monitors()
        .map_err(|e| format!("获取显示器列表失败: {}", e))?;
    let monitor = monitors
        .iter()
        .find(|m| {
            let pos = m.position();
            let size = m.size();
            cx >= pos.x
                && cx < pos.x + size.width as i32
                && cy >= pos.y
                && cy < pos.y + size.height as i32
        })
        .ok_or("光标不在任何显示器上".to_string())?;

    let mpos = monitor.position();
    let msize = monitor.size();
    let mon_right = mpos.x + msize.width as i32;
    let mon_bottom = mpos.y + msize.height as i32;

    const GAP: i32 = 28;

    // Default: below cursor, horizontally centred on cursor
    let mut wx = cx - ww / 2;
    let mut wy = cy + GAP;

    // If window would spill below monitor, try above the cursor
    if wy + wh > mon_bottom {
        wy = cy - wh - GAP;
    }
    // Clamp vertical
    if wy < mpos.y {
        wy = mpos.y + 8;
    }
    // Clamp horizontal
    if wx < mpos.x {
        wx = mpos.x + 4;
    }
    if wx + ww > mon_right {
        wx = mon_right - ww - 4;
    }

    window
        .set_position(PhysicalPosition::new(wx, wy))
        .map_err(|e| format!("设置窗口位置失败: {}", e))?;

    Ok(())
}

fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // When on settings page, never hide; just focus if already visible.
        let is_settings = app
            .state::<AppState>()
            .current_page
            .lock()
            .map(|p| p.as_str() == "settings")
            .unwrap_or(false);

        match window.is_visible() {
            Ok(true) => {
                if is_settings {
                    // On settings page: never hide, always bring to front.
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                } else {
                    let _ = window.hide();
                }
            }
            Ok(false) => {
                if cursor::is_input_focused() {
                    if position_near_cursor(&window).is_err() {
                        let _ = window.center();
                    }
                } else {
                    let _ = window.center();
                }
                let _ = window.show();
                let _ = window.set_focus();
            }
            Err(_) => {
                // On error, force show + focus anyway.
                let _ = window.unminimize();
                let _ = window.center();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }
    }
}

/// Register accelerator with the global shortcut plugin.
/// The callback toggles the main window on Pressed.
pub fn register(app: &AppHandle, raw: &str) -> Result<(), String> {
    let normalized = normalize(raw);
    if normalized.is_empty() {
        return Err("Empty shortcut".into());
    }

    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(normalized.as_str(), move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_window(&app_handle);
            }
        })
        .map_err(|e| format!("{}", e))?;

    println!("Shortcut registered (Rust): {}", normalized);
    Ok(())
}

/// Unregister a previously registered accelerator.
pub fn unregister(app: &AppHandle, raw: &str) {
    let normalized = normalize(raw);
    if let Err(e) = app.global_shortcut().unregister(normalized.as_str()) {
        eprintln!("Failed to unregister shortcut '{}': {}", normalized, e);
    }
}

// ── Initialization (called once from setup) ─────────────────────────

/// Load persisted shortcut from database (via AppState managed by Tauri),
/// register it, and store it in-memory. Called once during setup.
pub fn init(app_handle: &AppHandle) {
    let state = app_handle.state::<AppState>();

    let initial = {
        let db = state.database.lock().expect("database lock");
        db.get_config(SHORTCUT_KEY)
            .unwrap_or_else(|| DEFAULT_SHORTCUT.to_string())
    };

    if let Err(e) = register(app_handle, &initial) {
        eprintln!("Failed to register initial shortcut: {}", e);
    }

    {
        let mut s = state.shortcut.lock().expect("shortcut lock");
        *s = Some(initial);
    }
}

// ── Tauri commands ──────────────────────────────────────────────────

#[tauri::command]
pub fn get_shortcut(state: State<AppState>) -> String {
    let shortcut = state.shortcut.lock().expect("shortcut lock");
    shortcut
        .clone()
        .unwrap_or_else(|| DEFAULT_SHORTCUT.to_string())
}

#[tauri::command]
pub fn set_shortcut(
    app: AppHandle,
    state: State<AppState>,
    shortcut: String,
) -> Result<(), String> {
    let trimmed = shortcut.trim().to_string();
    if trimmed.is_empty() {
        return Err("快捷键不能为空".into());
    }

    let new_key = normalize(&trimmed);
    if new_key.is_empty() {
        return Err("快捷键格式无效".into());
    }

    // Read current shortcut (before any changes)
    let old: Option<String> = {
        let current = state.shortcut.lock().expect("shortcut lock");
        current.clone()
    };

    // If new shortcut normalizes to the same string as old → just update
    // the stored representation, no OS re-registration needed.
    let same = old
        .as_ref()
        .map(|o| normalize(o) == new_key)
        .unwrap_or(false);

    if !same {
        // ── Register NEW shortcut FIRST ────────────────────────────
        // If another program already owns this hotkey the OS will
        // reject our registration — and we'll return an error BEFORE
        // touching the old shortcut.
        register(&app, &trimmed).map_err(|e| format!("快捷键已被其他程序占用：{}", e))?;

        // New registration succeeded → safe to drop the old one.
        if let Some(ref old_shortcut) = old {
            unregister(&app, old_shortcut);
        }
    }

    // Persist to SQLite
    {
        let database = state.database.lock().expect("database lock");
        database
            .set_config(SHORTCUT_KEY, &trimmed)
            .map_err(|e| format!("保存配置失败: {}", e))?;
    }

    // Update in-memory state
    {
        let mut current = state.shortcut.lock().expect("shortcut lock");
        *current = Some(trimmed);
    }

    Ok(())
}
