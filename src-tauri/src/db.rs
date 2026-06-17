use chrono::Local;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::env;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

#[derive(Debug, Serialize, Clone)]
pub struct ClipboardItem {
    pub id: i64,
    pub content: String,
    pub content_type: String,
    pub image_path: String,
    pub html_content: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::Database;

    #[test]
    fn detects_short_code_snippets() {
        assert_eq!(
            Database::detect_content_type("fn main() { println!(\"hi\"); }"),
            "code"
        );
        assert_eq!(
            Database::detect_content_type("const total = items.reduce((a, b) => a + b, 0);"),
            "code"
        );
    }

    #[test]
    fn detects_structured_code_and_queries() {
        assert_eq!(
            Database::detect_content_type("{\"name\":\"ClipForge\",\"enabled\":true}"),
            "code"
        );
        assert_eq!(
            Database::detect_content_type("SELECT id, name FROM users WHERE active = 1;"),
            "code"
        );
    }

    #[test]
    fn detects_shell_commands_as_code() {
        assert_eq!(
            Database::detect_content_type("cargo test --package clipforge"),
            "code"
        );
        assert_eq!(
            Database::detect_content_type("npm install @tauri-apps/api"),
            "code"
        );
    }

    #[test]
    fn keeps_plain_text_as_text() {
        assert_eq!(
            Database::detect_content_type("请在会议后把这个方案发给我确认一下"),
            "text"
        );
        assert_eq!(
            Database::detect_content_type("This is a normal sentence with (parentheses)."),
            "text"
        );
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct CustomGroup {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub color: Option<String>,
}

pub struct Database {
    conn: Connection,
    images_dir: PathBuf,
    db_path: PathBuf,
}

impl Database {
    pub fn new(db_path: Option<PathBuf>) -> Self {
        let path = db_path.unwrap_or_else(Self::default_db_path);
        let db_dir = path.parent().unwrap().to_path_buf();
        let images_dir = db_dir.join("images");

        fs::create_dir_all(&images_dir).expect("Failed to create images directory");

        println!("Database path: {}", path.display());

        // Ensure parent directory exists (important for custom paths)
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("Failed to create db parent directory");
        }

        let conn = Connection::open(&path).expect("Failed to open database");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("Failed to enable foreign keys");

        Self::migrate_schema(&conn);

        Self {
            conn,
            images_dir,
            db_path: path,
        }
    }

    fn default_db_path() -> PathBuf {
        let exe_path = env::current_exe().expect("Failed to get current exe path");
        let exe_dir = exe_path.parent().expect("Failed to get exe directory");
        let db_dir = exe_dir.join("db");
        fs::create_dir_all(&db_dir).expect("Failed to create db directory");
        db_dir.join("clipforge.db")
    }

    /// Absolute path to the currently open database file.
    pub fn db_path(&self) -> PathBuf {
        self.db_path.clone()
    }

    fn migrate_schema(conn: &Connection) {
        // Use a custom migrations table instead of SQLite's built-in PRAGMA user_version.
        // Clearer, self-documenting, and survives DROP/CREATE cycles.
        conn.execute(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version     INTEGER PRIMARY KEY,
                applied_at  TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
                description TEXT    NOT NULL
            )
            ",
            [],
        )
        .expect("Failed to create schema_migrations table");

        let version: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);

        // Testing phase — keep everything in a single v1 migration.
        // When the schema changes, add a new block like: if version < 2 { … }.
        if version < 1 {
            // Tear down and rebuild clean
            conn.execute("DROP TABLE IF EXISTS clipboard_items", []).ok();
            conn.execute("DROP TABLE IF EXISTS custom_groups", []).ok();
            conn.execute("DROP TABLE IF EXISTS config", []).ok();

            conn.execute(
                "
                CREATE TABLE clipboard_items (
                    id           INTEGER PRIMARY KEY AUTOINCREMENT,
                    content      TEXT,
                    image_path   TEXT DEFAULT '',
                    html_content TEXT DEFAULT '',
                    content_hash TEXT NOT NULL,
                    created_at   TEXT NOT NULL,
                    group_id     INTEGER DEFAULT NULL REFERENCES custom_groups(id) ON DELETE SET NULL
                )
                ",
                [],
            )
            .expect("Failed to create clipboard_items");

            conn.execute(
                "
                CREATE TABLE custom_groups (
                    id         INTEGER PRIMARY KEY AUTOINCREMENT,
                    name       TEXT    NOT NULL UNIQUE,
                    created_at TEXT    NOT NULL DEFAULT (datetime('now', 'localtime')),
                    color      TEXT
                )
                ",
                [],
            )
            .expect("Failed to create custom_groups");

            conn.execute(
                "
                CREATE TABLE config (
                    key   TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                )
                ",
                [],
            )
            .expect("Failed to create config");

            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at, description) VALUES (?1, datetime('now', 'localtime'), ?2)",
                params![1, "Initial schema: clipboard_items, custom_groups, config"],
            )
            .expect("Failed to record v1 migration");
        }
    }

    /// Public — also used by clipboard.rs for watcher comparison.
    pub fn hash_content(content: &str) -> String {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    pub fn hash_bytes(bytes: &[u8]) -> String {
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        format!("{:x}", hasher.finish())
    }

    /// Classify clipboard text: "url", "email", "code", "color", or "text".
    pub fn detect_content_type(text: &str) -> &'static str {
        let trimmed = text.trim();

        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            if trimmed.len() >= 8 {
                return "url";
            }
        }

        // Simple email regex
        if let Some(at) = trimmed.find('@') {
            let before = &trimmed[..at];
            let after = &trimmed[at + 1..];
            if !before.is_empty()
                && !after.is_empty()
                && after.contains('.')
                && !before.contains(' ')
                && !after.contains(' ')
                && before.len() <= 254
                && after.len() <= 254
            {
                return "email";
            }
        }

        // Color: #RGB / #RRGGBB / #RRGGBBAA
        if trimmed.starts_with('#') {
            let hex_part = &trimmed[1..];
            let len = hex_part.len();
            if (len == 3 || len == 6 || len == 8) && hex_part.chars().all(|c| c.is_ascii_hexdigit())
            {
                return "color";
            }
        }

        if Self::looks_like_code(trimmed) {
            return "code";
        }

        "text"
    }

    fn looks_like_code(text: &str) -> bool {
        if text.len() < 2 {
            return false;
        }
        if text.starts_with("```") {
            return true;
        }

        let mut score = 0;
        let lower = text.to_ascii_lowercase();
        let structural_count = text
            .chars()
            .filter(|c| matches!(c, '{' | '}' | '[' | ']' | '(' | ')' | ';' | '=' | '<' | '>'))
            .count();

        if structural_count >= 4 {
            score += 3;
        } else if structural_count >= 2 {
            score += 2;
        } else if structural_count == 1 {
            score += 1;
        }

        if Self::looks_like_json(text) {
            score += 4;
        }
        if Self::looks_like_shell_command(&lower) {
            score += 4;
        }
        if Self::has_code_layout(text) {
            score += 2;
        }

        let keyword_count = Self::code_keyword_count(&lower);
        if keyword_count >= 2 {
            score += 3;
        } else if keyword_count == 1 {
            score += 1;
        }

        if [
            "=>", "->", "::", "==", "!=", "<=", ">=", "&&", "||", ":=", "$(",
        ]
        .iter()
        .any(|op| text.contains(op))
        {
            score += 2;
        }

        score >= 4
    }

    fn looks_like_json(text: &str) -> bool {
        let wrapped = (text.starts_with('{') && text.ends_with('}'))
            || (text.starts_with('[') && text.ends_with(']'));
        wrapped && text.contains(':')
    }

    fn looks_like_shell_command(lower: &str) -> bool {
        let Some(first) = lower.split_whitespace().next() else {
            return false;
        };
        if !lower.contains(' ') {
            return false;
        }

        matches!(
            first,
            "cargo"
                | "npm"
                | "yarn"
                | "pnpm"
                | "git"
                | "docker"
                | "kubectl"
                | "pip"
                | "python"
                | "node"
                | "rustc"
                | "go"
                | "java"
                | "mvn"
                | "gradle"
                | "cmake"
                | "make"
                | "curl"
                | "wget"
                | "ssh"
        )
    }

    fn has_code_layout(text: &str) -> bool {
        text.lines().any(|line| {
            let leading_spaces = line.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            leading_spaces >= 2
        }) || text.contains('\n') && text.contains(';')
    }

    fn code_keyword_count(lower: &str) -> usize {
        const KEYWORDS: &[&str] = &[
            "fn", "function", "const", "let", "var", "class", "struct", "enum", "impl", "trait",
            "import", "export", "return", "async", "await", "if", "else", "for", "while", "match",
            "use", "pub", "def", "lambda", "select", "from", "where", "insert", "update", "delete",
            "create", "alter", "join", "script", "html", "div", "span",
        ];

        lower
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|token| KEYWORDS.contains(token))
            .take(3)
            .count()
    }

    pub fn save_clipboard_data(&self, text: &str, html: &str) {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let hash = Self::hash_content(text);

        let result = self.conn.execute(
            "
            INSERT OR IGNORE INTO clipboard_items (content, image_path, html_content, content_hash, created_at)
            VALUES (?1, '', ?2, ?3, ?4)
            ",
            params![text, html, hash, now],
        );

        match result {
            Ok(rows) => {
                if rows > 0 {
                    println!("Saved text+html to database.");
                    self.enforce_limits();
                } else {
                    println!("Text already exists, ignored.");
                }
            }
            Err(error) => {
                println!("Failed to save data: {}", error);
            }
        }
    }

    pub fn save_clipboard_image(&self, rgba_bytes: &[u8], width: usize, height: usize) {
        let hash = Self::hash_bytes(rgba_bytes);

        let exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM clipboard_items WHERE content_hash = ?1 AND image_path != ''",
                params![hash],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if exists {
            println!("Image already exists, ignored.");
            return;
        }

        let img = image::RgbaImage::from_raw(width as u32, height as u32, rgba_bytes.to_vec());
        let img = match img {
            Some(i) => i,
            None => {
                println!("Failed to create image from raw bytes");
                return;
            }
        };

        let image_filename = format!("{}.png", hash);
        let image_path = self.images_dir.join(&image_filename);

        match img.save(&image_path) {
            Ok(_) => println!("Saved image to: {}", image_path.display()),
            Err(e) => {
                println!("Failed to save image: {}", e);
                return;
            }
        }

        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let rel_path = format!("images/{}", image_filename);

        let result = self.conn.execute(
            "
            INSERT INTO clipboard_items (content, image_path, html_content, content_hash, created_at)
            VALUES ('', ?1, '', ?2, ?3)
            ",
            params![rel_path, hash, now],
        );

        match result {
            Ok(_) => {
                println!("Image record saved to database.");
                self.enforce_limits();
            }
            Err(error) => {
                println!("Failed to save image record: {}", error);
                let _ = fs::remove_file(&image_path);
            }
        }
    }

    pub fn list_recent_items(&self, offset: i64, limit: i64) -> Vec<ClipboardItem> {
        self.list_items(None, offset, limit)
    }

    /// List items with optional group_id filter.  content_type is computed
    /// dynamically by map_item — no SQL filtering on it.
    fn list_items(
        &self,
        group_id: Option<i64>,
        offset: i64,
        limit: i64,
    ) -> Vec<ClipboardItem> {
        let mut conditions: Vec<&str> = Vec::new();
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(gid) = group_id {
            conditions.push("ci.group_id = ?");
            param_values.push(Box::new(gid));
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", conditions.join(" AND "))
        };

        let where_count = param_values.len();
        let limit_idx = where_count + 1;
        let offset_idx = where_count + 2;

        param_values.push(Box::new(limit));
        param_values.push(Box::new(offset));

        let sql = format!(
            "SELECT ci.id, ci.content, ci.image_path, \
             ci.html_content, ci.created_at, ci.group_id, cg.name \
             FROM clipboard_items ci \
             LEFT JOIN custom_groups cg ON ci.group_id = cg.id \
             {where_clause} \
             ORDER BY ci.id DESC \
             LIMIT ?{limit_idx} OFFSET ?{offset_idx}"
        );

        let mut stmt = self.conn.prepare(&sql).expect("Failed to prepare query");

        // Convert Box<dyn ToSql> into rusqlite param references
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();

        let rows = stmt
            .query_map(param_refs.as_slice(), Self::map_item)
            .expect("Failed to query clipboard items");

        rows.filter_map(|r| r.ok()).collect()
    }

    fn map_item(row: &rusqlite::Row) -> rusqlite::Result<ClipboardItem> {
        let content: String = row.get(1)?;
        let image_path: String = row.get(2)?;
        // content_type is no longer stored; compute dynamically
        let content_type = if !image_path.is_empty() {
            "image".to_string()
        } else {
            Self::detect_content_type(&content).to_string()
        };
        Ok(ClipboardItem {
            id: row.get(0)?,
            content,
            content_type,
            image_path,
            html_content: row.get(3)?,
            created_at: row.get(4)?,
            group_id: row.get(5).ok(),
            group_name: row.get(6).ok(),
        })
    }

    pub fn search_items(&self, keyword: &str) -> Vec<ClipboardItem> {
        let pattern = format!("%{}%", keyword);

        let mut statement = self
            .conn
            .prepare(
                "
            SELECT ci.id, ci.content, ci.image_path, ci.html_content, ci.created_at, ci.group_id, cg.name
            FROM clipboard_items ci
            LEFT JOIN custom_groups cg ON ci.group_id = cg.id
            WHERE ci.content LIKE ?1
            ORDER BY ci.id DESC
            LIMIT 50
            ",
            )
            .expect("Failed to prepare search query");

        let rows = statement
            .query_map(params![pattern], Self::map_item)
            .expect("Failed to search clipboard items");

        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn delete_item(&self, id: i64) -> Result<(), rusqlite::Error> {
        if let Ok((image_path,)) = self.conn.query_row::<(String,), _, _>(
            "SELECT image_path FROM clipboard_items WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?,)),
        ) {
            if !image_path.is_empty() {
                let db_dir = self.db_path.parent().unwrap();
                let full_path = db_dir.join(&image_path);
                let _ = fs::remove_file(&full_path);
            }
        }

        self.conn
            .execute("DELETE FROM clipboard_items WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn clear_all(&self) -> Result<(), rusqlite::Error> {
        let db_dir = self.db_path.parent().unwrap().to_path_buf();

        let mut stmt = self
            .conn
            .prepare("SELECT image_path FROM clipboard_items WHERE image_path != ''")
            .ok();

        if let Some(ref mut stmt) = stmt {
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).ok();
            if let Some(rows) = rows {
                for row in rows.flatten() {
                    let full_path = db_dir.join(&row);
                    let _ = fs::remove_file(&full_path);
                }
            }
        }

        self.conn.execute("DELETE FROM clipboard_items", [])?;
        Ok(())
    }

    pub fn images_base_dir(&self) -> PathBuf {
        self.images_dir.clone()
    }

    pub fn get_config(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM config WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .ok()
    }

    pub fn get_config_i64(&self, key: &str, default: i64) -> i64 {
        self.get_config(key)
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(default)
    }

    pub fn set_config(&self, key: &str, value: &str) -> Result<(), rusqlite::Error> {
        self.conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // ── Limits ────────────────────────────────────────────────────

    /// Read `max_items` and `max_retention_days` from config, then
    /// delete unprotected items that exceed the limits.
    /// Items with `group_id IS NOT NULL` (custom-groups) are always
    /// protected from automatic cleanup.
    fn enforce_limits(&self) {
        let max_items = self.get_config_i64("max_items", 0);
        let max_days = self.get_config_i64("max_retention_days", 0);

        if max_items > 0 {
            // Count total unprotected items
            let count: i64 = self
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM clipboard_items WHERE group_id IS NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let excess = count - max_items;
            if excess > 0 {
                // Delete the oldest excess items
                let _ = self.conn.execute(
                    "DELETE FROM clipboard_items WHERE id IN (\
                     SELECT id FROM clipboard_items WHERE group_id IS NULL \
                     ORDER BY id ASC LIMIT ?1\
                     )",
                    params![excess],
                );
                println!("Cleaned up {} excess items (limit: {})", excess, max_items);
            }
        }

        if max_days > 0 {
            let cutoff = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            // SQLite datetime comparison — delete items older than max_days
            let deleted = self
                .conn
                .execute(
                    "DELETE FROM clipboard_items \
                     WHERE group_id IS NULL \
                     AND datetime(created_at) < datetime(?, '-' || ? || ' days')",
                    params![cutoff, max_days],
                )
                .unwrap_or(0);
            if deleted > 0 {
                println!(
                    "Cleaned up {} expired items (retention: {} days)",
                    deleted, max_days
                );
            }
        }
    }

    // ── Type / group filtered listing ─────────────────────────────

    /// List items filtered by content_type with pagination.
    /// content_type is computed dynamically (not stored), so we fetch a batch
    /// and post-filter in Rust.
    pub fn list_items_by_type(
        &self,
        content_type: &str,
        offset: i64,
        limit: i64,
    ) -> Vec<ClipboardItem> {
        // Fetch enough items so that after filtering we can still paginate.
        let fetch_limit = 500;
        let all = self.list_items(None, 0, fetch_limit);
        all.into_iter()
            .filter(|item| item.content_type == content_type)
            .skip(offset as usize)
            .take(limit as usize)
            .collect()
    }

    /// List items belonging to a specific custom group.
    pub fn list_items_by_group(
        &self,
        group_id: i64,
        offset: i64,
        limit: i64,
    ) -> Vec<ClipboardItem> {
        self.list_items(Some(group_id), offset, limit)
    }

    // ── Custom groups CRUD ────────────────────────────────────────

    pub fn list_groups(&self) -> Vec<CustomGroup> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, created_at, color FROM custom_groups ORDER BY id ASC")
            .expect("Failed to prepare groups query");

        let rows = stmt
            .query_map([], |row| {
                Ok(CustomGroup {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    color: row.get(3).ok(),
                })
            })
            .expect("Failed to query groups");

        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn create_group(&self, name: &str, color: Option<&str>) -> Result<CustomGroup, String> {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.conn
            .execute(
                "INSERT INTO custom_groups (name, created_at, color) VALUES (?1, ?2, ?3)",
                params![name, now, color],
            )
            .map_err(|e| format!("创建分组失败: {}", e))?;

        let id = self.conn.last_insert_rowid();
        Ok(CustomGroup {
            id,
            name: name.to_string(),
            created_at: now,
            color: color.map(|c| c.to_string()),
        })
    }

    pub fn rename_group(&self, group_id: i64, new_name: &str) -> Result<(), String> {
        let name = new_name.trim();
        if name.is_empty() {
            return Err("分组名不能为空".to_string());
        }
        self.conn
            .execute(
                "UPDATE custom_groups SET name = ?1 WHERE id = ?2",
                params![name, group_id],
            )
            .map_err(|e| format!("重命名分组失败: {}", e))?;
        Ok(())
    }

    pub fn update_group_color(&self, group_id: i64, color: Option<&str>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE custom_groups SET color = ?1 WHERE id = ?2",
                params![color, group_id],
            )
            .map_err(|e| format!("更新分组颜色失败: {}", e))?;
        Ok(())
    }

    pub fn delete_group(&self, group_id: i64) -> Result<(), String> {
        // Items with this group_id will have group_id set to NULL
        // (cascading SET NULL via foreign key — already defined in schema)
        self.conn
            .execute("DELETE FROM custom_groups WHERE id = ?1", params![group_id])
            .map_err(|e| format!("删除分组失败: {}", e))?;
        Ok(())
    }

    pub fn set_item_group(&self, item_id: i64, group_id: Option<i64>) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE clipboard_items SET group_id = ?1 WHERE id = ?2",
                params![group_id, item_id],
            )
            .map_err(|e| format!("设置分组失败: {}", e))?;
        Ok(())
    }
}
