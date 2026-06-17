//! External JSON configuration file (exe_dir/clipforge.json).
//! Persists settings that must be known before the SQLite database is opened,
//! such as the database file path.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Default filename next to the executable.
const CONFIG_FILENAME: &str = "clipforge.json";

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
    pub fn path() -> PathBuf {
        let exe_path = std::env::current_exe().expect("Failed to get current exe path");
        exe_path.parent().unwrap().join(CONFIG_FILENAME)
    }

    /// Resolve the effective database path.
    /// If `db_path` is set and non-empty, use it. Otherwise default to
    /// `<exe_dir>/db/clipforge.db`.
    pub fn effective_db_path(&self) -> PathBuf {
        if let Some(ref custom) = self.db_path {
            let p = PathBuf::from(custom);
            if !p.as_os_str().is_empty() {
                return p;
            }
        }
        Self::default_db_path()
    }

    /// Default database path: `<exe_dir>/db/clipforge.db`.
    pub fn default_db_path() -> PathBuf {
        let exe_path = std::env::current_exe().expect("Failed to get current exe path");
        let exe_dir = exe_path.parent().expect("Failed to get exe directory");
        exe_dir.join("db").join("clipforge.db")
    }
}
