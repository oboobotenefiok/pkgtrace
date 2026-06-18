use anyhow::{Result, anyhow};
use chrono::{DateTime, Local, Utc};
use serde_json;
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write, BufRead};
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::models::{PackageEvent, LogEntry, LogAction};

#[derive(Clone)]
pub struct Logger {
    config: Arc<Config>,
    log_file: PathBuf,
}

impl Logger {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let logger = Self {
            config: config.clone(),
            log_file: config.log_file.clone(),
        };

        logger.ensure_log_file()?;
        Ok(logger)
    }

    fn ensure_log_file(&self) -> Result<()> {
        if let Some(parent) = self.log_file.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        if !self.log_file.exists() {
            File::create(&self.log_file)?;
        }

        Ok(())
    }

    pub fn log_event(&self, event: &PackageEvent) -> Result<()> {
        self.rotate_if_needed()?;

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)?;

        let mut writer = BufWriter::new(log_file);
        let json = serde_json::to_string(event)?;
        writeln!(writer, "{}", json)?;

        Ok(())
    }

    pub fn log_action(&self, action: LogAction, package: &str, details: Option<serde_json::Value>) -> Result<()> {
        let event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: package.to_string(),
            action: action.to_string(),
            source: crate::models::PackageSource::Unknown,
            details: details.map(|d| d.to_string()),
            pid: Some(std::process::id()),
            user: Some(std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())),
        };

        self.log_event(&event)
    }

    pub fn info(&self, message: &str) -> Result<()> {
        self.log_action(LogAction::Info, "pkgtrace", Some(serde_json::json!({ "message": message })))
    }

    pub fn warning(&self, message: &str) -> Result<()> {
        self.log_action(LogAction::Warning, "pkgtrace", Some(serde_json::json!({ "message": message })))
    }

    pub fn error(&self, message: &str) -> Result<()> {
        self.log_action(LogAction::Error, "pkgtrace", Some(serde_json::json!({ "message": message })))
    }

    pub fn log_install(&self, package: &str, source: &str) -> Result<()> {
        self.log_action(
            LogAction::Install,
            package,
            Some(serde_json::json!({ "source": source }))
        )
    }

    pub fn log_remove(&self, package: &str, source: &str) -> Result<()> {
        self.log_action(
            LogAction::Remove,
            package,
            Some(serde_json::json!({ "source": source }))
        )
    }

    pub fn log_use(&self, package: &str) -> Result<()> {
        self.log_action(LogAction::Use, package, None)
    }

    pub fn log_scan(&self, count: usize) -> Result<()> {
        self.log_action(
            LogAction::Scan,
            "pkgtrace",
            Some(serde_json::json!({ "packages_found": count }))
        )
    }

    fn rotate_if_needed(&self) -> Result<()> {
        if !self.log_file.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(&self.log_file)?;
        if metadata.len() > self.config.max_log_size {
            let backup_path = self.log_file.with_extension("log.old");
            
            let timestamp = Utc::now().timestamp();
            let rotated_path = self.log_file.parent()
                .unwrap_or(&PathBuf::from("."))
                .join(format!("pkgtrace.{}.log", timestamp));

            if backup_path.exists() {
                std::fs::remove_file(&backup_path)?;
            }

            std::fs::rename(&self.log_file, &rotated_path)?;

            if let Ok(entries) = self.read_logs(1000) {
                if !entries.is_empty() {
                    File::create(&self.log_file)?;
                    for entry in entries {
                        self.log_event(&entry)?;
                    }
                }
            }

            self.info(&format!("Log rotated: {}", rotated_path.display()))?;
        }

        Ok(())
    }

    pub fn read_logs(&self, limit: usize) -> Result<Vec<PackageEvent>> {
        if !self.log_file.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.log_file)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    events.push(event);
                    if events.len() >= limit {
                        break;
                    }
                }
            }
        }

        Ok(events)
    }

    pub fn read_logs_since(&self, timestamp: i64) -> Result<Vec<PackageEvent>> {
        if !self.log_file.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.log_file)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    if event.timestamp >= timestamp {
                        events.push(event);
                    }
                }
            }
        }

        Ok(events)
    }

    pub fn get_log_stats(&self) -> Result<LogStats> {
        if !self.log_file.exists() {
            return Ok(LogStats::default());
        }

        let metadata = std::fs::metadata(&self.log_file)?;
        let file_size = metadata.len();

        let events = self.read_logs(usize::MAX)?;
        let total_events = events.len();

        let mut installs = 0;
        let mut removes = 0;
        let mut uses = 0;
        let mut errors = 0;
        let mut warnings = 0;

        for event in events {
            match event.action.as_str() {
                "INSTALL" => installs += 1,
                "REMOVE" => removes += 1,
                "USE" => uses += 1,
                "ERROR" => errors += 1,
                "WARNING" => warnings += 1,
                _ => {}
            }
        }

        Ok(LogStats {
            file_size,
            total_events,
            installs,
            removes,
            uses,
            errors,
            warnings,
        })
    }

    pub fn clear_logs(&self) -> Result<()> {
        if self.log_file.exists() {
            std::fs::remove_file(&self.log_file)?;
            File::create(&self.log_file)?;
            self.info("Logs cleared")?;
        }
        Ok(())
    }

    pub fn get_log_path(&self) -> &PathBuf {
        &self.log_file
    }

    pub fn export_logs(&self, output_path: &PathBuf) -> Result<()> {
        if !self.log_file.exists() {
            return Err(anyhow!("Log file does not exist"));
        }

        std::fs::copy(&self.log_file, output_path)?;
        Ok(())
    }

    pub fn get_recent_activity(&self, hours: u64) -> Result<Vec<PackageEvent>> {
        let threshold = Utc::now().timestamp() - (hours * 3600) as i64;
        self.read_logs_since(threshold)
    }

    pub fn get_package_activity(&self, package: &str) -> Result<Vec<PackageEvent>> {
        if !self.log_file.exists() {
            return Ok(Vec::new());
        }

        let file = File::open(&self.log_file)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    if event.package == package {
                        events.push(event);
                    }
                }
            }
        }

        Ok(events)
    }
}

#[derive(Debug, Default)]
pub struct LogStats {
    pub file_size: u64,
    pub total_events: usize,
    pub installs: usize,
    pub removes: usize,
    pub uses: usize,
    pub errors: usize,
    pub warnings: usize,
}

impl LogStats {
    pub fn summary(&self) -> String {
        format!(
            "Log size: {}, Events: {}, Installs: {}, Removes: {}, Uses: {}, Errors: {}, Warnings: {}",
            crate::utils::format_size(self.file_size),
            self.total_events,
            self.installs,
            self.removes,
            self.uses,
            self.errors,
            self.warnings
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_logger_creation() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let logger = Logger::new(Arc::new(config))?;
        assert!(logger.log_file.exists());

        Ok(())
    }

    #[test]
    fn test_log_event() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let logger = Logger::new(Arc::new(config))?;

        let event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: "test".to_string(),
            action: "INSTALL".to_string(),
            source: crate::models::PackageSource::Pkg,
            details: None,
            pid: None,
            user: None,
        };

        logger.log_event(&event)?;

        let logs = logger.read_logs(10)?;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].package, "test");
        assert_eq!(logs[0].action, "INSTALL");

        Ok(())
    }

    #[test]
    fn test_log_actions() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let logger = Logger::new(Arc::new(config))?;

        logger.info("Test info")?;
        logger.warning("Test warning")?;
        logger.error("Test error")?;
        logger.log_install("test-pkg", "pkg")?;
        logger.log_remove("test-pkg", "pkg")?;
        logger.log_use("test-pkg")?;

        let stats = logger.get_log_stats()?;
        assert!(stats.total_events >= 6);

        Ok(())
    }

    #[test]
    fn test_log_rotation() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());
        config.max_log_size = 1024;

        let logger = Logger::new(Arc::new(config))?;

        for i in 0..100 {
            let event = PackageEvent {
                timestamp: Utc::now().timestamp(),
                package: format!("test-{}", i),
                action: "INSTALL".to_string(),
                source: crate::models::PackageSource::Pkg,
                details: None,
                pid: None,
                user: None,
            };
            logger.log_event(&event)?;
        }

        let log_dir = dir.path().join(".config/pkgtrace");
        let rotated_files: Vec<_> = std::fs::read_dir(&log_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("pkgtrace."))
            .collect();

        assert!(!rotated_files.is_empty());

        Ok(())
    }

    #[test]
    fn test_get_recent_activity() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let logger = Logger::new(Arc::new(config))?;

        let event = PackageEvent {
            timestamp: Utc::now().timestamp() - 3600,
            package: "test".to_string(),
            action: "INSTALL".to_string(),
            source: crate::models::PackageSource::Pkg,
            details: None,
            pid: None,
            user: None,
        };
        logger.log_event(&event)?;

        let recent = logger.get_recent_activity(2)?;
        assert_eq!(recent.len(), 1);

        let older = logger.get_recent_activity(0)?;
        assert_eq!(older.len(), 0);

        Ok(())
    }
}