use anyhow::{anyhow, Result};
use home::home_dir;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub log_file: PathBuf,
    pub db_file: PathBuf,
    pub cache_dir: PathBuf,
    pub scan_dirs: Vec<PathBuf>,
    pub exclude_patterns: Vec<String>,
    pub auto_scan: bool,
    pub scan_interval: u64,
    pub max_log_size: u64,
    pub log_level: String,
    pub dependency_depth: usize,
    pub protect_core: bool,
    pub backup_before_remove: bool,
    pub parallel_scans: usize,
}

impl Default for Config {
    fn default() -> Self {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"));
        let config_dir = home.join(".config/pkgtrace");

        Self {
            log_file: config_dir.join("pkgtrace.log"),
            db_file: config_dir.join("packages.db.json"),
            cache_dir: config_dir.join("cache"),
            scan_dirs: vec![
                home.join("bin"),
                home.join(".local/bin"),
                home.join("opt"),
                PathBuf::from("/data/data/com.termux/files/usr/bin"),
                PathBuf::from("/data/data/com.termux/files/usr/local/bin"),
                PathBuf::from("/data/data/com.termux/files/usr/lib"),
            ],
            exclude_patterns: vec![
                r".*\.so$".to_string(),
                r".*\.a$".to_string(),
                r".*\.o$".to_string(),
                r".*\.pyc$".to_string(),
                r".*\.pyo$".to_string(),
                r".*\.elc$".to_string(),
                r".*\.class$".to_string(),
                r".*\.jar$".to_string(),
                r"^lib.*\.dylib$".to_string(),
            ],
            auto_scan: true,
            scan_interval: 86400,
            max_log_size: 10 * 1024 * 1024,
            log_level: "info".to_string(),
            dependency_depth: 20,
            protect_core: true,
            backup_before_remove: true,
            parallel_scans: 4,
        }
    }
}

impl Config {
    pub fn load_or_create() -> Result<Self> {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"));
        let config_dir = home.join(".config/pkgtrace");
        let config_file = config_dir.join("config.toml");

        if config_file.exists() {
            let content = std::fs::read_to_string(&config_file)?;
            let config: Config = toml::from_str(&content)?;
            config.ensure_directories()?;
            Ok(config)
        } else {
            std::fs::create_dir_all(&config_dir)?;
            let config = Config::default();
            let content = toml::to_string_pretty(&config)?;
            std::fs::write(&config_file, content)?;
            config.ensure_directories()?;
            Ok(config)
        }
    }

    pub fn save(&self) -> Result<()> {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"));
        let config_dir = home.join(".config/pkgtrace");

        std::fs::create_dir_all(&config_dir)
            .map_err(|e| anyhow!("Failed to create config directory: {}", e))?;

        let config_file = config_dir.join("config.toml");
        let content = toml::to_string_pretty(self)
            .map_err(|e| anyhow!("Failed to serialize config: {}", e))?;

        std::fs::write(&config_file, content)
            .map_err(|e| anyhow!("Failed to write config: {}", e))?;

        self.ensure_directories()?;
        Ok(())
    }

    fn ensure_directories(&self) -> Result<()> {
        if let Some(parent) = self.log_file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("Failed to create log directory: {}", e))?;
        }

        if let Some(parent) = self.db_file.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("Failed to create db directory: {}", e))?;
        }

        std::fs::create_dir_all(&self.cache_dir)
            .map_err(|e| anyhow!("Failed to create cache directory: {}", e))?;

        Ok(())
    }

    pub fn get_log_level_filter(&self) -> String {
        self.log_level.to_lowercase()
    }

    pub fn should_rotate_log(&self) -> bool {
        if let Ok(metadata) = std::fs::metadata(&self.log_file) {
            metadata.len() > self.max_log_size
        } else {
            false
        }
    }

    pub fn get_scan_dirs(&self) -> Vec<PathBuf> {
        self.scan_dirs.clone()
    }

    pub fn get_exclude_patterns(&self) -> Vec<String> {
        self.exclude_patterns.clone()
    }

    pub fn is_excluded(&self, path: &PathBuf) -> bool {
        if let Some(path_str) = path.to_str() {
            for pattern in &self.exclude_patterns {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if re.is_match(path_str) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn with_custom_dir(mut self, dir: PathBuf) -> Self {
        let config_dir = dir.join(".config/pkgtrace");
        self.log_file = config_dir.join("pkgtrace.log");
        self.db_file = config_dir.join("packages.db.json");
        self.cache_dir = config_dir.join("cache");
        self
    }

    pub fn validate(&self) -> Result<()> {
        if self.dependency_depth == 0 {
            return Err(anyhow!("dependency_depth must be > 0"));
        }

        if self.parallel_scans == 0 {
            return Err(anyhow!("parallel_scans must be > 0"));
        }

        if self.scan_interval < 60 {
            return Err(anyhow!("scan_interval must be at least 60 seconds"));
        }

        if self.max_log_size < 1024 * 1024 {
            return Err(anyhow!("max_log_size must be at least 1MB"));
        }

        Ok(())
    }
}

pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: Config::default(),
        }
    }

    pub fn log_file(mut self, path: PathBuf) -> Self {
        self.config.log_file = path;
        self
    }

    pub fn db_file(mut self, path: PathBuf) -> Self {
        self.config.db_file = path;
        self
    }

    pub fn cache_dir(mut self, path: PathBuf) -> Self {
        self.config.cache_dir = path;
        self
    }

    pub fn scan_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.config.scan_dirs = dirs;
        self
    }

    pub fn exclude_patterns(mut self, patterns: Vec<String>) -> Self {
        self.config.exclude_patterns = patterns;
        self
    }

    pub fn auto_scan(mut self, enabled: bool) -> Self {
        self.config.auto_scan = enabled;
        self
    }

    pub fn scan_interval(mut self, seconds: u64) -> Self {
        self.config.scan_interval = seconds;
        self
    }

    pub fn max_log_size(mut self, bytes: u64) -> Self {
        self.config.max_log_size = bytes;
        self
    }

    pub fn log_level(mut self, level: &str) -> Self {
        self.config.log_level = level.to_string();
        self
    }

    pub fn dependency_depth(mut self, depth: usize) -> Self {
        self.config.dependency_depth = depth;
        self
    }

    pub fn protect_core(mut self, protect: bool) -> Self {
        self.config.protect_core = protect;
        self
    }

    pub fn backup_before_remove(mut self, backup: bool) -> Self {
        self.config.backup_before_remove = backup;
        self
    }

    pub fn parallel_scans(mut self, count: usize) -> Self {
        self.config.parallel_scans = count;
        self
    }

    pub fn build(self) -> Result<Config> {
        self.config.validate()?;
        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.log_file.to_str().unwrap().contains("pkgtrace.log"));
        assert!(config
            .db_file
            .to_str()
            .unwrap()
            .contains("packages.db.json"));
        assert!(config.cache_dir.to_str().unwrap().contains("cache"));
        assert!(!config.scan_dirs.is_empty());
        assert!(!config.exclude_patterns.is_empty());
        assert!(config.auto_scan);
        assert_eq!(config.scan_interval, 86400);
        assert!(config.max_log_size > 0);
        assert!(!config.log_level.is_empty());
        assert!(config.dependency_depth > 0);
        assert!(config.protect_core);
        assert!(config.backup_before_remove);
        assert!(config.parallel_scans > 0);
    }

    #[test]
    fn test_config_builder() -> Result<()> {
        let dir = tempdir()?;
        let config = ConfigBuilder::new()
            .log_file(dir.path().join("test.log"))
            .db_file(dir.path().join("test.db.json"))
            .cache_dir(dir.path().join("cache"))
            .auto_scan(false)
            .scan_interval(3600)
            .log_level("debug")
            .dependency_depth(10)
            .build()?;

        assert_eq!(config.log_file, dir.path().join("test.log"));
        assert_eq!(config.db_file, dir.path().join("test.db.json"));
        assert_eq!(config.cache_dir, dir.path().join("cache"));
        assert!(!config.auto_scan);
        assert_eq!(config.scan_interval, 3600);
        assert_eq!(config.log_level, "debug");
        assert_eq!(config.dependency_depth, 10);

        Ok(())
    }

    #[test]
    fn test_validation() -> Result<()> {
        let config = ConfigBuilder::new().dependency_depth(0).build();
        assert!(config.is_err());

        let config = ConfigBuilder::new().parallel_scans(0).build();
        assert!(config.is_err());

        let config = ConfigBuilder::new().scan_interval(30).build();
        assert!(config.is_err());

        let config = ConfigBuilder::new().max_log_size(1024).build();
        assert!(config.is_err());

        Ok(())
    }

    #[test]
    fn test_is_excluded() -> Result<()> {
        let config = Config::default();
        let path = PathBuf::from("test.so");
        assert!(config.is_excluded(&path));

        let path = PathBuf::from("test.py");
        assert!(!config.is_excluded(&path));

        Ok(())
    }
}
