use super::AppState;
use crate::db::{ClipboardItem, Database};
use arboard::Clipboard;
use base64::Engine;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

// ═══════════════════════════════════════════════════════════════════
// Watcher structs
// ═══════════════════════════════════════════════════════════════════

pub struct ClipboardWatcher {
    clipboard: Clipboard,
    last_text_hash: String,
    last_image_hash: String,
}

pub struct ClipboardImage {
    pub width: usize,
    pub height: usize,
    pub rgba_bytes: Vec<u8>,
}

pub struct ClipboardData {
    pub text: String,
    pub html: String,
    pub hash: String,
}

impl ClipboardWatcher {
    pub fn new() -> Self {
        let mut clipboard = Clipboard::new().expect("Failed to access clipboard");

        let last_text = clipboard.get_text().unwrap_or_default().trim().to_string();
        let last_text_hash = if last_text.is_empty() {
            String::new()
        } else {
            Database::hash_content(&last_text)
        };

        Self {
            clipboard,
            last_text_hash,
            last_image_hash: String::new(),
        }
    }

    pub fn get_new_data(&mut self) -> Option<ClipboardData> {
        let text = self.clipboard.get_text().ok()?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        let h = Database::hash_content(&text);
        if h == self.last_text_hash {
            return None;
        }

        self.last_text_hash = h.clone();

        // Try to get HTML content — not all platforms support it.
        // arboard 3.x does not expose get_html(); this is a placeholder
        // for when multi-platform HTML clipboard read becomes available.
        let html = String::new();

        Some(ClipboardData {
            text,
            html,
            hash: h,
        })
    }

    pub fn get_new_image(&mut self) -> Option<ClipboardImage> {
        match self.clipboard.get_image() {
            Ok(image_data) => {
                let bytes = image_data.bytes.to_vec();
                let width = image_data.width;
                let height = image_data.height;

                let hash = Database::hash_bytes(&bytes);
                if hash == self.last_image_hash {
                    return None;
                }
                self.last_image_hash = hash;

                Some(ClipboardImage {
                    width,
                    height,
                    rgba_bytes: bytes,
                })
            }
            Err(_) => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// Background watcher thread
// ═══════════════════════════════════════════════════════════════════

pub fn start_watcher(
    app_handle: AppHandle,
    database: Arc<Mutex<Database>>,
    recently_copied: Arc<Mutex<HashSet<String>>>,
) {
    std::thread::spawn(move || {
        println!("Clipboard watcher started.");

        let mut clipboard_watcher = ClipboardWatcher::new();

        loop {
            let mut update = false;

            if let Some(image) = clipboard_watcher.get_new_image() {
                println!("New clipboard image: {}x{}", image.width, image.height);
                {
                    let database = database.lock().expect("Failed to lock database");
                    database.save_clipboard_image(&image.rgba_bytes, image.width, image.height);
                }
                update = true;
            } else if let Some(data) = clipboard_watcher.get_new_data() {
                // Skip if we just wrote this content to the clipboard ourselves
                let self_copied = {
                    let mut set = recently_copied.lock().expect("recently_copied lock");
                    set.remove(&data.hash)
                };

                if !self_copied {
                    println!("New clipboard text: {}", data.text);
                    {
                        let database = database.lock().expect("Failed to lock database");
                        database.save_clipboard_data(&data.text, &data.html);
                    }
                    update = true;
                } else {
                    println!("Skipped duplicate (self-copied): {}", data.text);
                }
            }

            if update {
                let _ = app_handle.emit("clipboard-update", ());
            }

            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

// ═══════════════════════════════════════════════════════════════════
// Keyboard paste simulation
// ═══════════════════════════════════════════════════════════════════

/// Send Ctrl+V (macOS: Cmd+V) to the currently focused window.
/// Returns Ok(()) on success, Err if enigo couldn't be created.
fn simulate_paste() -> Result<(), String> {
    use enigo::{Direction, Key, Keyboard};

    let mut enigo = enigo::Enigo::new(&enigo::Settings::default())
        .map_err(|e| format!("Failed to create enigo: {}", e))?;

    #[cfg(target_os = "macos")]
    {
        let _ = enigo.key(Key::Meta, Direction::Press);
        let _ = enigo.key(Key::Unicode('v'), Direction::Click);
        let _ = enigo.key(Key::Meta, Direction::Release);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = enigo.key(Key::Control, Direction::Press);
        let _ = enigo.key(Key::Unicode('v'), Direction::Click);
        let _ = enigo.key(Key::Control, Direction::Release);
    }

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════
// Tauri commands
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn list_recent_items(state: State<AppState>, offset: i64, limit: i64) -> Vec<ClipboardItem> {
    let database = state.database.lock().expect("Failed to lock database");
    database.list_recent_items(offset, limit)
}

#[tauri::command]
pub fn copy_to_clipboard(state: State<AppState>, content: String) -> Result<(), String> {
    let hash = Database::hash_content(&content);

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(&content).map_err(|e| e.to_string())?;

    let mut set = state.recently_copied.lock().expect("recently_copied lock");
    set.insert(hash);

    Ok(())
}

/// Copy text to clipboard, hide window, then simulate Ctrl+V.
#[tauri::command]
pub fn copy_and_paste(
    state: State<AppState>,
    app: AppHandle,
    content: String,
) -> Result<(), String> {
    let hash = Database::hash_content(&content);

    // 1. Write to OS clipboard
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(&content).map_err(|e| e.to_string())?;

    // 2. Mark as self-copied
    {
        let mut set = state.recently_copied.lock().expect("recently_copied lock");
        set.insert(hash);
    }

    // 3. Hide our window
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    // 4. Small delay for OS focus switch, then paste
    std::thread::sleep(std::time::Duration::from_millis(80));

    simulate_paste().ok();

    Ok(())
}

/// Copy an image from history back to clipboard, hide window, then
/// simulate Ctrl+V so it lands in the previously focused application.
#[tauri::command]
pub fn copy_and_paste_image(
    state: State<AppState>,
    app: AppHandle,
    image_path: String,
) -> Result<(), String> {
    // 1. Read image from disk
    let base_dir = {
        let database = state.database.lock().expect("database lock");
        database.images_base_dir()
    };
    let filename = std::path::Path::new(&image_path)
        .file_name()
        .ok_or_else(|| "Invalid image path".to_string())?;
    let full_path = base_dir.join(filename);
    let bytes = std::fs::read(&full_path).map_err(|e| format!("Failed to read image: {}", e))?;

    // 2. Decode PNG → RGBA pixels
    let img = image::ImageReader::new(std::io::Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|e| format!("Cannot guess image format: {}", e))?
        .decode()
        .map_err(|e| format!("Cannot decode image: {}", e))?;
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();

    // 3. Write to OS clipboard as image
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    let img_data = arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: rgba.into_raw().into(),
    };
    clipboard.set_image(img_data).map_err(|e| e.to_string())?;

    // 4. Mark hash so watcher skips it
    let hash = Database::hash_bytes(&bytes);
    {
        let mut set = state.recently_copied.lock().expect("recently_copied lock");
        set.insert(hash);
    }

    // 5. Hide window
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }

    // 6. Paste
    std::thread::sleep(std::time::Duration::from_millis(80));
    simulate_paste().ok();

    Ok(())
}

#[tauri::command]
pub fn search_items(state: State<AppState>, keyword: String) -> Vec<ClipboardItem> {
    let database = state.database.lock().expect("Failed to lock database");
    database.search_items(&keyword)
}

#[tauri::command]
pub fn delete_item(state: State<AppState>, id: i64) -> Result<(), String> {
    let database = state.database.lock().expect("Failed to lock database");
    database.delete_item(id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn clear_all_items(state: State<AppState>) -> Result<(), String> {
    let database = state.database.lock().expect("Failed to lock database");
    database.clear_all().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_image_base64(state: State<AppState>, image_path: String) -> Result<String, String> {
    let database = state.database.lock().expect("Failed to lock database");
    let base_dir = database.images_base_dir();

    let filename = std::path::Path::new(&image_path)
        .file_name()
        .ok_or_else(|| "Invalid image path".to_string())?;

    let full_path = base_dir.join(filename);
    let bytes = std::fs::read(&full_path).map_err(|e| format!("Failed to read image: {}", e))?;

    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{}", b64))
}

#[tauri::command]
pub fn hide_window(window: tauri::Window) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

/// Called by the frontend whenever the user switches between "main" and
/// "settings" pages. The Rust side uses this to skip auto-hide and
/// shortcut-toggle while the user is on the settings page.
#[tauri::command]
pub fn set_current_page(state: State<AppState>, page: String) {
    if let Ok(mut current) = state.current_page.lock() {
        *current = page;
    }
}

// ═══════════════════════════════════════════════════════════════════
// Config commands
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn get_db_path(state: State<AppState>) -> Result<String, String> {
    let database = state.database.lock().expect("database lock");
    Ok(database.db_path().to_string_lossy().to_string())
}

#[tauri::command]
pub fn set_db_path(path: String) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.db_path = if path.trim().is_empty() {
        None
    } else {
        Some(path.trim().to_string())
    };
    cfg.save()?;
    Ok(())
}

#[tauri::command]
pub fn get_config_value(state: State<AppState>, key: String) -> Result<Option<String>, String> {
    let database = state.database.lock().expect("database lock");
    Ok(database.get_config(&key))
}

#[tauri::command]
pub fn set_config_value(state: State<AppState>, key: String, value: String) -> Result<(), String> {
    let database = state.database.lock().expect("database lock");
    database.set_config(&key, &value).map_err(|e| e.to_string())
}

// ═══════════════════════════════════════════════════════════════════
// Type / group filtered listing commands
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn list_items_by_type(
    state: State<AppState>,
    content_type: String,
    offset: i64,
    limit: i64,
) -> Vec<ClipboardItem> {
    let database = state.database.lock().expect("database lock");
    database.list_items_by_type(&content_type, offset, limit)
}

#[tauri::command]
pub fn list_items_by_group(
    state: State<AppState>,
    group_id: i64,
    offset: i64,
    limit: i64,
) -> Vec<ClipboardItem> {
    let database = state.database.lock().expect("database lock");
    database.list_items_by_group(group_id, offset, limit)
}

// ═══════════════════════════════════════════════════════════════════
// Custom groups commands
// ═══════════════════════════════════════════════════════════════════

#[tauri::command]
pub fn list_groups(state: State<AppState>) -> Result<Vec<crate::db::CustomGroup>, String> {
    let database = state.database.lock().expect("database lock");
    Ok(database.list_groups())
}

#[tauri::command]
pub fn create_group(
    state: State<AppState>,
    name: String,
    color: Option<String>,
) -> Result<crate::db::CustomGroup, String> {
    let database = state.database.lock().expect("database lock");
    database.create_group(&name, color.as_deref())
}

#[tauri::command]
pub fn rename_group(state: State<AppState>, group_id: i64, name: String) -> Result<(), String> {
    let database = state.database.lock().expect("database lock");
    database.rename_group(group_id, &name)
}

#[tauri::command]
pub fn delete_group(state: State<AppState>, group_id: i64) -> Result<(), String> {
    let database = state.database.lock().expect("database lock");
    database.delete_group(group_id)
}

#[tauri::command]
pub fn update_group_color(
    state: State<AppState>,
    group_id: i64,
    color: Option<String>,
) -> Result<(), String> {
    let database = state.database.lock().expect("database lock");
    database.update_group_color(group_id, color.as_deref())
}

#[tauri::command]
pub fn set_item_group(
    state: State<AppState>,
    item_id: i64,
    group_id: Option<i64>,
) -> Result<(), String> {
    let database = state.database.lock().expect("database lock");
    database.set_item_group(item_id, group_id)
}
