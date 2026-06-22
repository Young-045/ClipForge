use crate::db::{Database, LogDatabase, LogLevel};
use log::{Metadata, Record};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

struct LogRecord {
    level: LogLevel,
    source: String,
    message: String,
}

/// Logger implementation that routes every log record through a channel
/// to a background thread, which writes into a **dedicated** SQLite log
/// database (`<data_dir>/logs.db`, `app_logs` table).
///
/// Using a separate DB file keeps the log table from bloating the main
/// application database, and the channel avoids blocking callers / deadlocks.
pub struct DbLogger {
    sender: mpsc::Sender<LogRecord>,
}

impl DbLogger {
    pub fn new(
        log_database: Arc<Mutex<LogDatabase>>,
        app_database: Arc<Mutex<Database>>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel::<LogRecord>();

        thread::spawn(move || {
            while let Ok(record) = receiver.recv() {
                if let Ok(log_db) = log_database.lock() {
                    log_db.write_log(record.level, &record.source, &record.message);

                    // Cleanup old logs based on the config stored in the main DB.
                    let max_log_days = app_database
                        .lock()
                        .map(|db| db.get_config_i64("max_log_days", 30))
                        .unwrap_or(30);
                    log_db.cleanup_old_logs(max_log_days);
                }
                // If the lock is poisoned or the logger is shutting down,
                // silently drop the record.
            }
        });

        Self { sender }
    }
}

impl log::Log for DbLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        // Filtering is handled by the global max_level setting.
        true
    }

    fn log(&self, record: &Record) {
        let level = match record.level() {
            log::Level::Debug => LogLevel::Debug,
            log::Level::Info => LogLevel::Info,
            log::Level::Warn => LogLevel::Warn,
            log::Level::Error => LogLevel::Error,
            log::Level::Trace => LogLevel::Debug,
        };

        let source = format!("{}:{}", record.file().unwrap_or("?"), record.line().unwrap_or(0));
        let message = format!("{}", record.args());

        if let Err(_) = self.sender.send(LogRecord {
            level,
            source,
            message,
        }) {
            // Channel closed (logger shutting down) — nothing to do.
        }
    }

    fn flush(&self) {}
}

/// Initialize the global logger. Must be called after the databases are
/// created but before any logging should be captured in `logs.db`.
pub fn init_logger(
    log_database: Arc<Mutex<LogDatabase>>,
    app_database: Arc<Mutex<Database>>,
) -> Result<(), log::SetLoggerError> {
    let logger = Box::new(DbLogger::new(log_database, app_database));
    log::set_boxed_logger(logger)?;
    log::set_max_level(log::LevelFilter::Info);
    Ok(())
}
