//! External JSON configuration file (AppData/Local/ClipForge/clipforge.json).
//! Persists settings that must be known before the SQLite database is opened,
//! such as the database file path.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Default filename under the OS-local app data folder.
const CONFIG_FILENAME: &str = "clipforge.json";
const APP_DIR_NAME: &str = "ClipForge";

/// Default data directory under the OS-local app data folder.
/// Windows: `C:\Users\<user>\AppData\Local\ClipForge\db`
/// macOS:   `~/Library/Application Support/ClipForge/db`
/// Linux:   `~/.local/share/ClipForge/db`
pub fn app_data_dir() -> PathBuf {
    dirs_next::data_local_dir()
        .expect("Failed to get local app data directory")
        .join(APP_DIR_NAME)
        .join("db")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    /// Absolute path to the SQLite database file.
    /// `None` / missing → use default `<exe_dir>/db/clipforge.db`.
    #[serde(default)]
    pub db_path: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self { db_path: None }
    }
}

impl AppConfig {
    /// Load config from disk, falling back to defaults if the file
    /// doesn't exist or is malformed.
    pub fn load() -> Self {
        let path = Self::path();
        match fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist current config to disk.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path();
        let json =
            serde_json::to_string_pretty(self).map_err(|e| format!("序列化配置失败: {}", e))?;
        fs::write(&path, json).map_err(|e| format!("写入配置文件失败: {}", e))?;
        Ok(())
    }

    /// Absolute path to the JSON config file.
    /// Windows: `C:\Users\<user>\AppData\Local\ClipForge\clipforge.json`
    /// macOS:   `~/Library/Application Support/ClipForge/clipforge.json`
    /// Linux:   `~/.local/share/ClipForge/clipforge.json`
    pub fn path() -> PathBuf {
        dirs_next::data_local_dir()
            .expect("Failed to get local app data directory")
            .join(APP_DIR_NAME)
            .join(CONFIG_FILENAME)
    }

    /// Resolve the effective database path.
    /// If `db_path` is set and non-empty, use it. Otherwise default to
    /// `<app_data_dir>/clipboard.db`.
    pub fn effective_db_path(&self) -> PathBuf {
        if let Some(ref custom) = self.db_path {
            let p = PathBuf::from(custom);
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        Self::default_db_path()
    }

    /// Default database path: `<app_data_dir>/clipboard.db`.
    pub fn default_db_path() -> PathBuf {
        app_data_dir().join("clipboard.db")
    }
}
