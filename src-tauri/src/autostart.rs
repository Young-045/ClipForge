use crate::AppState;
use crate::db::Database;
use std::path::PathBuf;
use tauri::State;

/// Read the autostart preference from the database `config` table.
///
/// Unlike the old implementation, this no longer queries the OS registry /
/// plist / desktop-file. The preference is a pure boolean stored alongside
/// other user settings (`max_items`, `max_retention_days`, etc.) so that
/// debug and release builds share the same choice.
#[tauri::command]
pub fn get_autostart(state: State<'_, AppState>) -> Result<bool, String> {
    let db = state.database.lock().map_err(|e| e.to_string())?;
    Ok(db.get_config("autostart").as_deref() == Some("true"))
}

/// Persist the autostart preference to the database AND sync it with the
/// OS-specific auto-start mechanism for the *current* executable.
///
/// The registry / plist / desktop-file mutation is offloaded to a blocking
/// thread so the Tauri event loop stays responsive.
#[tauri::command]
pub async fn set_autostart(
    state: State<'_, AppState>,
    enable: bool,
) -> Result<(), String> {
    // 1. Persist the human preference to SQLite (fast, local lock).
    {
        let db = state.database.lock().map_err(|e| e.to_string())?;
        db.set_config("autostart", if enable { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
    }

    // 2. Sync the OS auto-start entry for the *current* executable.
    tauri::async_runtime::spawn_blocking(move || {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        if enable {
            register(&exe)?;
        } else {
            unregister();
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Called once at startup. Ensures the OS auto-start entry matches the
/// persisted preference in the database.
///
/// This is important because debug and release builds are different
/// executables. The preference lives in the shared database; the OS entry
/// must be kept in sync every time the process starts.
pub fn sync_on_startup(db: &Database) {
    let enabled = db.get_config("autostart").as_deref() == Some("true");

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(_) => return,
    };

    if enabled {
        // Best-effort: if registration fails we log but carry on.
        let _ = register(&exe).inspect_err(|e| {
            log::warn!("Startup autostart registration failed: {}", e);
        });
    } else {
        unregister();
    }
}

// ── Windows ────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn register(exe: &PathBuf) -> Result<(), String> {
    let status = std::process::Command::new("reg")
        .args([
            "add",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "ClipForge",
            "/t",
            "REG_SZ",
            "/d",
            &exe.to_string_lossy(),
            "/f",
        ])
        .status()
        .map_err(|e| format!("Failed to run reg add: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("reg add exited with {}", status))
    }
}

/// Unregister always succeeds (best-effort). If the key doesn't exist,
/// `reg delete /f` still exits 0.
#[cfg(target_os = "windows")]
fn unregister() {
    let _ = std::process::Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "ClipForge",
            "/f",
        ])
        .status();
}

#[cfg(target_os = "windows")]
#[allow(dead_code)]
fn is_registered(exe: &PathBuf) -> Result<bool, String> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "ClipForge",
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            Ok(stdout.contains(&exe.to_string_lossy().as_ref()))
        }
        _ => Ok(false),
    }
}

// ── macOS ──────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn register(exe: &PathBuf) -> Result<(), String> {
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.lefto.clipforge</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>"#,
        exe.display()
    );

    let agents_dir = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents_dir).map_err(|e| e.to_string())?;
    std::fs::write(agents_dir.join("com.lefto.clipforge.plist"), plist)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(target_os = "macos")]
fn unregister() {
    let plist = dirs_next::home_dir()
        .map(|h| h.join("Library/LaunchAgents/com.lefto.clipforge.plist"));
    if let Some(p) = plist {
        if p.exists() {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(target_os = "macos")]
fn is_registered(_exe: &PathBuf) -> Result<bool, String> {
    let plist = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join("Library/LaunchAgents/com.lefto.clipforge.plist");
    Ok(plist.exists())
}

// ── Linux ──────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn register(exe: &PathBuf) -> Result<(), String> {
    let desktop = format!(
        r#"[Desktop Entry]
Type=Application
Name=ClipForge
Exec={}
X-GNOME-Autostart-enabled=true
"#,
        exe.display()
    );

    let autostart_dir = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join(".config/autostart");
    std::fs::create_dir_all(&autostart_dir).map_err(|e| e.to_string())?;
    std::fs::write(autostart_dir.join("clipforge.desktop"), desktop).map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(target_os = "linux")]
fn unregister() {
    let desktop = dirs_next::home_dir()
        .map(|h| h.join(".config/autostart/clipforge.desktop"));
    if let Some(d) = desktop {
        if d.exists() {
            let _ = std::fs::remove_file(d);
        }
    }
}

#[cfg(target_os = "linux")]
fn is_registered(_exe: &PathBuf) -> Result<bool, String> {
    let desktop = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join(".config/autostart/clipforge.desktop");
    Ok(desktop.exists())
}
