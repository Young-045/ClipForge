use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Emitter, Manager, WindowEvent,
};

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Build the system-tray icon with right-click menu and left-click →
/// show+focus. Returns the `last_tray_click` atomic (shared with window
/// event handler to suppress spurious auto-hide).
pub fn setup(app: &App) -> tauri::Result<Arc<AtomicU64>> {
    let last_tray_click = Arc::new(AtomicU64::new(0));

    let show_item = MenuItemBuilder::with_id("show", "显示窗口").build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "退出").build(app)?;
    let menu = MenuBuilder::new(app)
        .item(&show_item)
        .item(&separator)
        .item(&quit_item)
        .build()?;

    let icon_bytes = include_bytes!("../icons/tray-32x32.png");
    let icon_img = image::load_from_memory(icon_bytes)
        .expect("Failed to decode tray icon")
        .into_rgba8();
    let (icon_w, icon_h) = icon_img.dimensions();
    let tray_icon = Image::new_owned(icon_img.into_raw(), icon_w, icon_h);

    let tray_click_flag = Arc::clone(&last_tray_click);
    TrayIconBuilder::new()
        .icon(tray_icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(move |tray_icon, event| {
            if let TrayIconEvent::Click { button, .. } = event {
                if matches!(button, tauri::tray::MouseButton::Left) {
                    tray_click_flag.store(now_ms(), Ordering::Relaxed);
                    let app = tray_icon.app_handle();
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                        let _ = app.emit("tray-show", ());
                    }
                }
            }
        })
        .build(app)?;

    Ok(last_tray_click)
}

/// Attach window event handlers to the "main" window:
/// - Close → prevent close, hide to tray.
/// - Focus loss → emit "auto-hide" after 250ms (unless tray was clicked,
///   or the user is currently on the settings page).
pub fn setup_window_events(
    app_handle: &AppHandle,
    last_tray_click: Arc<AtomicU64>,
    current_page: Arc<Mutex<String>>,
) {
    let Some(window) = app_handle.get_webview_window("main") else {
        return;
    };

    let win = window.clone();
    let app_h = app_handle.clone();
    let focus_flag = Arc::clone(&last_tray_click);

    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            let _ = win.hide();
        }
        WindowEvent::Focused(false) => {
            // If user is on settings page, never auto-hide.
            let is_settings = current_page
                .lock()
                .map(|p| p.as_str() == "settings")
                .unwrap_or(false);
            if is_settings {
                return;
            }
            let last = focus_flag.load(Ordering::Relaxed);
            if now_ms().saturating_sub(last) < 600 {
                return;
            }
            let app_h = app_h.clone();
            let flag = focus_flag.clone();
            let win_check = win.clone();
            let current_page_clone = current_page.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                // Re-check after delay — user may have switched back to main
                let still_settings = current_page_clone
                    .lock()
                    .map(|p| p.as_str() == "settings")
                    .unwrap_or(false);
                if still_settings {
                    return;
                }
                let last = flag.load(Ordering::Relaxed);
                if now_ms().saturating_sub(last) < 600 {
                    return;
                }
                if win_check.is_focused().unwrap_or(false) {
                    return;
                }
                let _ = app_h.emit("auto-hide", ());
            });
        }
        _ => {}
    });
}
