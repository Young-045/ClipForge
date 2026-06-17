use std::path::PathBuf;

/// Check whether the current executable is registered for autostart.
#[tauri::command]
pub fn get_autostart() -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    is_registered(&exe)
}

/// Enable or disable autostart-on-boot.
#[tauri::command]
pub fn set_autostart(enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;

    if enable {
        register(&exe)?;
    } else {
        unregister()?;
    }

    Ok(())
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

#[cfg(target_os = "windows")]
fn unregister() -> Result<(), String> {
    // /f = force (no prompt), also suppresses error if value doesn't exist
    let _ = std::process::Command::new("reg")
        .args([
            "delete",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "ClipForge",
            "/f",
        ])
        .status();
    Ok(())
}

#[cfg(target_os = "windows")]
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
        _ => Ok(false), // Not found or error → false
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
fn unregister() -> Result<(), String> {
    let plist = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join("Library/LaunchAgents/com.lefto.clipforge.plist");
    if plist.exists() {
        std::fs::remove_file(plist).map_err(|e| e.to_string())?;
    }
    Ok(())
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
fn unregister() -> Result<(), String> {
    let desktop = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join(".config/autostart/clipforge.desktop");
    if desktop.exists() {
        std::fs::remove_file(desktop).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn is_registered(_exe: &PathBuf) -> Result<bool, String> {
    let desktop = dirs_next::home_dir()
        .ok_or("Cannot find home dir")?
        .join(".config/autostart/clipforge.desktop");
    Ok(desktop.exists())
}
