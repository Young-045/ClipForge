mod autostart;
mod clipboard;
mod config;
mod cursor;
mod db;
mod logger;
mod shortcut;
mod tray;

use db::{Database, LogDatabase};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tauri::Manager;

/// Shared application state injected via Tauri manage().
pub struct AppState {
    pub database: Arc<Mutex<Database>>,
    pub log_database: Arc<Mutex<LogDatabase>>,
    pub shortcut: Arc<Mutex<Option<String>>>,
    /// Content hashes that were written to the clipboard by our own
    /// copy_to_clipboard / copy_and_paste commands. The watcher thread
    /// checks this set and skips re-saving content we ourselves put there.
    pub recently_copied: Arc<Mutex<HashSet<String>>>,
    /// Current frontend page: "main" or "settings". Used to skip
    /// auto-hide / shortcut-toggle when the user is on the settings page.
    pub current_page: Arc<Mutex<String>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load external config (for db_path, before DB init)
    let app_config = config::AppConfig::load();
    let db_path = app_config.effective_db_path();
    let log_db_path = db_path.parent().map(|dir| dir.join("logs.db"));

    let database = Arc::new(Mutex::new(Database::new(Some(db_path))));
    let log_database = Arc::new(Mutex::new(LogDatabase::new(log_db_path)));

    // Route Rust log records into the dedicated log database.
    if let Err(e) = logger::init_logger(Arc::clone(&log_database), Arc::clone(&database)) {
        eprintln!("Failed to initialize database logger: {}", e);
    }
    log::info!("ClipForge application starting");

    let shortcut: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let recently_copied: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
    let current_page: Arc<Mutex<String>> = Arc::new(Mutex::new("main".to_string()));

    // Clones for capture by setup closure
    let db_for_setup = Arc::clone(&database);
    let log_db_for_setup = Arc::clone(&log_database);
    let recently_copied_for_watcher = Arc::clone(&recently_copied);
    let current_page_for_events = Arc::clone(&current_page);

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // A second instance was launched: bring the existing window to the foreground.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState {
            database,
            log_database,
            shortcut,
            recently_copied,
            current_page,
        })
        .setup(move |app| {
            // Log the actual log database path once the app handle is available
            if let Ok(log_db) = log_db_for_setup.lock() {
                log::info!("Log database path: {}", log_db.db_path().display());
            }

            // 1. Global shortcut — init reads DB, registers, stores to state
            shortcut::init(app.handle());

            // 2. System tray + window events
            let last_tray_click = tray::setup(app)?;
            tray::setup_window_events(app.handle(), last_tray_click, current_page_for_events);

            // 3. Background clipboard watcher
            clipboard::start_watcher(
                app.handle().clone(),
                Arc::clone(&db_for_setup),
                recently_copied_for_watcher,
            );

            // 4. Sync autostart preference (DB → OS registry/plist/desktop-file)
            if let Ok(db) = db_for_setup.lock() {
                autostart::sync_on_startup(&db);
            }

            log::info!("ClipForge setup completed");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            clipboard::list_recent_items,
            clipboard::list_items_by_type,
            clipboard::list_items_by_group,
            clipboard::copy_to_clipboard,
            clipboard::copy_and_paste,
            clipboard::copy_and_paste_image,
            clipboard::search_items,
            clipboard::delete_item,
            clipboard::clear_all_items,
            clipboard::get_image_base64,
            clipboard::hide_window,
            clipboard::set_current_page,
            clipboard::get_db_path,
            clipboard::set_db_path,
            clipboard::get_config_value,
            clipboard::set_config_value,
            clipboard::list_groups,
            clipboard::create_group,
            clipboard::delete_group,
            clipboard::rename_group,
            clipboard::update_group_color,
            clipboard::set_item_group,
            clipboard::list_logs,
            shortcut::get_shortcut,
            shortcut::set_shortcut,
            autostart::get_autostart,
            autostart::set_autostart,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
