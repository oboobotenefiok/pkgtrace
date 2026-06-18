use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde_json;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, Write, BufRead, Seek, SeekFrom},
    path::PathBuf,
    sync::Arc,
};

use crate::config::Config;
use crate::models::{PackageEvent, LogAction, PackageSource};

#[derive(Clone)]
pub struct Logger {
    config: Arc<Config>,
    log_file: PathBuf,
    // Cache for fast event lookups - using parking_lot::RwLock
    event_cache: Arc<RwLock<Option<Vec<PackageEvent>>>>,
    last_read_position: Arc<RwLock<u64>>,
}

impl Logger {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let logger = Self {
            config: config.clone(),
            log_file: config.log_file.clone(),
            event_cache: Arc::new(RwLock::new(None)),
            last_read_position: Arc::new(RwLock::new(0)),
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

    // ============ EVENT LOGGING ============

    pub fn log_event(&self, event: &PackageEvent) -> Result<()> {
        self.rotate_if_needed()?;

        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_file)?;

        let mut writer = BufWriter::new(log_file);
        let json = serde_json::to_string(event)?;
        writeln!(writer, "{}", json)?;
        writer.flush()?;

        // Invalidate cache when new event is written
        let mut cache = self.event_cache.write();
        *cache = None;
        let mut pos = self.last_read_position.write();
        *pos = 0;

        Ok(())
    }

    pub fn log_action(&self, action: LogAction, package: &str, details: Option<serde_json::Value>) -> Result<()> {
        let event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: package.to_string(),
            action: action.to_string(),
            source: PackageSource::Unknown,
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

    // ============ EFFICIENT EVENT LOADING ============

    /// Load all events from log file with caching
    pub fn load_all_events(&self) -> Result<Vec<PackageEvent>> {
        // Check cache first
        {
            let cache = self.event_cache.read();
            if let Some(events) = &*cache {
                return Ok(events.clone());
            }
        }

        // Load from disk
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
                }
            }
        }

        // Update cache
        let mut cache = self.event_cache.write();
        *cache = Some(events.clone());

        Ok(events)
    }

    /// Load only new events since last read (incremental)
    pub fn load_new_events(&self) -> Result<Vec<PackageEvent>> {
        if !self.log_file.exists() {
            return Ok(Vec::new());
        }

        use std::io::Read;

        let file = OpenOptions::new()
            .read(true)
            .open(&self.log_file)?;
        
        let mut reader = BufReader::new(file);
        let mut last_pos = self.last_read_position.write();
        
        // Seek to last read position
        reader.seek(SeekFrom::Start(*last_pos))?;
        
        let mut new_events = Vec::new();
        let mut bytes_read = 0;

        for line in reader.by_ref().lines() {
            if let Ok(line) = line {
                bytes_read += line.len() as u64 + 1; // +1 for newline
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    new_events.push(event);
                }
            }
        }

        // Update position
        *last_pos += bytes_read;

        // Update cache with new events
        if !new_events.is_empty() {
            let mut cache = self.event_cache.write();
            if let Some(existing) = cache.as_mut() {
                existing.extend(new_events.clone());
            } else {
                *cache = Some(new_events.clone());
            }
        }

        Ok(new_events)
    }

    /// Get events for a specific package
    pub fn get_events_for_package(&self, package: &str) -> Result<Vec<PackageEvent>> {
        let events = self.load_all_events()?;
        Ok(events
            .into_iter()
            .filter(|e| e.package == package)
            .collect())
    }

    /// Get last use time for a package
    pub fn get_last_use(&self, package: &str) -> Result<Option<i64>> {
        let events = self.load_all_events()?;
        let last = events
            .iter()
            .filter(|e| e.package == package && (e.action == "USE" || e.action == "INSTALL"))
            .max_by_key(|e| e.timestamp)
            .map(|e| e.timestamp);
        Ok(last)
    }

    /// Get usage count for a package
    pub fn get_usage_count(&self, package: &str) -> Result<usize> {
        let events = self.load_all_events()?;
        let count = events
            .iter()
            .filter(|e| e.package == package && (e.action == "USE" || e.action == "INSTALL"))
            .count();
        Ok(count)
    }

    /// Get all used packages
    pub fn get_used_packages(&self) -> Result<HashSet<String>> {
        let events = self.load_all_events()?;
        let mut used = HashSet::new();
        for event in events {
            if event.action == "INSTALL" || event.action == "USE" {
                used.insert(event.package);
            }
        }
        Ok(used)
    }

    // ============ LOG QUERYING ============

    pub fn read_logs(&self, limit: usize) -> Result<Vec<PackageEvent>> {
        let events = self.load_all_events()?;
        let start = if events.len() > limit {
            events.len() - limit
        } else {
            0
        };
        Ok(events[start..].to_vec())
    }

    pub fn read_logs_since(&self, timestamp: i64) -> Result<Vec<PackageEvent>> {
        let events = self.load_all_events()?;
        Ok(events
            .into_iter()
            .filter(|e| e.timestamp >= timestamp)
            .collect())
    }

    pub fn get_recent_activity(&self, hours: u64) -> Result<Vec<PackageEvent>> {
        let threshold = Utc::now().timestamp() - (hours * 3600) as i64;
        self.read_logs_since(threshold)
    }

    pub fn get_package_activity(&self, package: &str) -> Result<Vec<PackageEvent>> {
        self.get_events_for_package(package)
    }

    // ============ LOG STATISTICS ============

    pub fn get_log_stats(&self) -> Result<LogStats> {
        if !self.log_file.exists() {
            return Ok(LogStats::default());
        }

        let metadata = std::fs::metadata(&self.log_file)?;
        let file_size = metadata.len();

        let events = self.load_all_events()?;
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

    // ============ LOG MAINTENANCE ============

    fn rotate_if_needed(&self) -> Result<()> {
        if !self.log_file.exists() {
            return Ok(());
        }

        let metadata = std::fs::metadata(&self.log_file)?;
        if metadata.len() > self.config.max_log_size {
            let timestamp = Utc::now().timestamp();
            let rotated_path = self.log_file.parent()
                .unwrap_or(&PathBuf::from("."))
                .join(format!("pkgtrace.{}.log", timestamp));

            // Rename current log
            std::fs::rename(&self.log_file, &rotated_path)?;

            // Create new log file
            File::create(&self.log_file)?;

            // Keep last 1000 entries in new log
            if let Ok(entries) = self.read_logs(1000) {
                for entry in entries {
                    self.log_event(&entry)?;
                }
            }

            // Clear cache after rotation
            let mut cache = self.event_cache.write();
            *cache = None;
            let mut pos = self.last_read_position.write();
            *pos = 0;

            self.info(&format!("Log rotated: {}", rotated_path.display()))?;
        }

        Ok(())
    }

    pub fn clear_logs(&self) -> Result<()> {
        if self.log_file.exists() {
            std::fs::remove_file(&self.log_file)?;
            File::create(&self.log_file)?;
            
            // Clear cache
            let mut cache = self.event_cache.write();
            *cache = None;
            let mut pos = self.last_read_position.write();
            *pos = 0;
            
            self.info("Logs cleared")?;
        }
        Ok(())
    }

    pub fn export_logs(&self, output_path: &PathBuf) -> Result<()> {
        if !self.log_file.exists() {
            return Err(anyhow!("Log file does not exist"));
        }

        std::fs::copy(&self.log_file, output_path)?;
        Ok(())
    }

    pub fn get_log_path(&self) -> &PathBuf {
        &self.log_file
    }

    /// Invalidate the event cache (useful when log file changes externally)
    pub fn invalidate_cache(&self) {
        let mut cache = self.event_cache.write();
        *cache = None;
        let mut pos = self.last_read_position.write();
        *pos = 0;
    }

    /// Force reload of all events from disk
    pub fn reload(&self) -> Result<()> {
        self.invalidate_cache();
        self.load_all_events()?;
        Ok(())
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
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;
        assert!(logger.log_file.exists());

        Ok(())
    }

    #[test]
    fn test_log_event() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;

        let event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: "test".to_string(),
            action: "INSTALL".to_string(),
            source: PackageSource::Pkg,
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
    fn test_load_all_events() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;

        // Log multiple events
        for i in 0..5 {
            let event = PackageEvent {
                timestamp: Utc::now().timestamp(),
                package: format!("test-{}", i),
                action: "INSTALL".to_string(),
                source: PackageSource::Pkg,
                details: None,
                pid: None,
                user: None,
            };
            logger.log_event(&event)?;
        }

        let events = logger.load_all_events()?;
        assert_eq!(events.len(), 5);

        // Second load should use cache
        let events2 = logger.load_all_events()?;
        assert_eq!(events2.len(), 5);

        Ok(())
    }

    #[test]
    fn test_get_events_for_package() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;

        let event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: "test-pkg".to_string(),
            action: "USE".to_string(),
            source: PackageSource::Pkg,
            details: None,
            pid: None,
            user: None,
        };
        logger.log_event(&event)?;

        let events = logger.get_events_for_package("test-pkg")?;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, "USE");

        Ok(())
    }

    #[test]
    fn test_get_used_packages() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;

        // Log install event
        let install = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: "installed-pkg".to_string(),
            action: "INSTALL".to_string(),
            source: PackageSource::Pkg,
            details: None,
            pid: None,
            user: None,
        };
        logger.log_event(&install)?;

        // Log use event
        let use_event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: "used-pkg".to_string(),
            action: "USE".to_string(),
            source: PackageSource::Pkg,
            details: None,
            pid: None,
            user: None,
        };
        logger.log_event(&use_event)?;

        let used = logger.get_used_packages()?;
        assert!(used.contains("installed-pkg"));
        assert!(used.contains("used-pkg"));
        assert!(!used.contains("not-used-pkg"));

        Ok(())
    }

    #[test]
    fn test_log_stats() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;

        logger.log_install("pkg1", "pkg")?;
        logger.log_install("pkg2", "pkg")?;
        logger.log_remove("pkg1", "pkg")?;
        logger.log_use("pkg2")?;
        logger.info("test info")?;
        logger.warning("test warning")?;

        let stats = logger.get_log_stats()?;
        assert!(stats.total_events >= 6);
        assert!(stats.installs >= 2);
        assert!(stats.removes >= 1);
        assert!(stats.uses >= 1);
        assert!(stats.warnings >= 1);

        Ok(())
    }

    #[test]
    fn test_log_rotation() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        config.max_log_size = 1024; // Small size to force rotation

        let logger = Logger::new(Arc::new(config))?;

        // Write many events to trigger rotation
        for i in 0..100 {
            let event = PackageEvent {
                timestamp: Utc::now().timestamp(),
                package: format!("test-{}", i),
                action: "INSTALL".to_string(),
                source: PackageSource::Pkg,
                details: None,
                pid: None,
                user: None,
            };
            logger.log_event(&event)?;
        }

        // Check if rotation happened
        let log_dir = dir.path();
        let rotated_files: Vec<_> = std::fs::read_dir(log_dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("pkgtrace."))
            .collect();

        assert!(!rotated_files.is_empty());

        Ok(())
    }

    #[test]
    fn test_cache_invalidation() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default();
        config.log_file = dir.path().join("test.log");
        
        let logger = Logger::new(Arc::new(config))?;

        // Load events (populates cache)
        let _ = logger.load_all_events()?;
        
        // Add new event (should invalidate cache)
        let event = PackageEvent {
            timestamp: Utc::now().timestamp(),
            package: "new-pkg".to_string(),
            action: "INSTALL".to_string(),
            source: PackageSource::Pkg,
            details: None,
            pid: None,
            user: None,
        };
        logger.log_event(&event)?;

        // Load again (should reload from disk)
        let events = logger.load_all_events()?;
        assert!(events.iter().any(|e| e.package == "new-pkg"));

        Ok(())
    }
}