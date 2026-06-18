use anyhow::{Result, anyhow};
use chrono::{DateTime, Local, Utc, TimeZone};
use serde_json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::models::{Package, CacheEntry, DependencyCache, UsageCache};



#[derive(Clone)]
pub struct CacheManager {
    config: Arc<Config>,
    cache_file: PathBuf,
    metadata_file: PathBuf,
    dep_cache_file: PathBuf,
    usage_cache_file: PathBuf,
}

impl CacheManager {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let manager = Self {
            config: config.clone(),
            cache_file: config.db_file.clone(),
            metadata_file: config.cache_dir.join("metadata.json"),
            dep_cache_file: config.cache_dir.join("dependency_cache.json"),
            usage_cache_file: config.cache_dir.join("usage_cache.json"),
        };

        manager.ensure_cache_dir()?;
        Ok(manager)
    }

    fn ensure_cache_dir(&self) -> Result<()> {
        if let Some(parent) = self.cache_file.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        if let Some(parent) = self.metadata_file.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        if let Some(parent) = self.dep_cache_file.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        if let Some(parent) = self.usage_cache_file.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
            }
        }

        Ok(())
    }

    // ============ PACKAGE CACHE ============

    pub fn load(&self) -> Result<HashMap<String, Package>> {
        if !self.cache_file.exists() {
            return Ok(HashMap::new());
        }

        let file = File::open(&self.cache_file)?;
        let reader = BufReader::new(file);
        let entries: Vec<CacheEntry> = serde_json::from_reader(reader)?;

        let mut packages = HashMap::new();
        for entry in entries {
            packages.insert(entry.package.name.clone(), entry.package);
        }

        Ok(packages)
    }

    pub fn save(&self, packages: &[Package]) -> Result<()> {
        let entries: Vec<CacheEntry> = packages
            .iter()
            .map(|pkg| CacheEntry {
                package: pkg.clone(),
                last_updated: Utc::now().timestamp(),
                scan_duration: 0,
            })
            .collect();

        let file = File::create(&self.cache_file)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &entries)?;

        self.save_metadata(packages.len())?;
        Ok(())
    }

    pub fn save_metadata(&self, package_count: usize) -> Result<()> {
        let metadata = serde_json::json!({
            "last_updated": Utc::now().timestamp(),
            "package_count": package_count,
            "version": env!("CARGO_PKG_VERSION"),
            "config_hash": self.config_hash(),
        });

        let file = File::create(&self.metadata_file)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, &metadata)?;

        Ok(())
    }

    pub fn get_all(&self) -> Vec<Package> {
        if let Ok(packages) = self.load() {
            packages.into_values().collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_package(&self, name: &str) -> Result<Option<Package>> {
        let packages = self.load()?;
        Ok(packages.get(name).cloned())
    }

    pub fn add_package(&self, package: Package) -> Result<()> {
        let mut packages = self.load()?;
        packages.insert(package.name.clone(), package);
        self.save(&packages.into_values().collect::<Vec<_>>())?;
        Ok(())
    }

    pub fn remove_package(&self, name: &str) -> Result<()> {
        let mut packages = self.load()?;
        packages.remove(name);
        self.save(&packages.into_values().collect::<Vec<_>>())?;
        Ok(())
    }

    pub fn update_package(&self, package: Package) -> Result<()> {
        let mut packages = self.load()?;
        packages.insert(package.name.clone(), package);
        self.save(&packages.into_values().collect::<Vec<_>>())?;
        Ok(())
    }

    pub fn is_fresh(&self) -> Result<bool> {
        if !self.metadata_file.exists() {
            return Ok(false);
        }

        let file = File::open(&self.metadata_file)?;
        let reader = BufReader::new(file);
        let metadata: serde_json::Value = serde_json::from_reader(reader)?;

        if let Some(timestamp) = metadata.get("last_updated").and_then(|v| v.as_i64()) {
            let age = Utc::now().timestamp() - timestamp;
            let interval = self.config.scan_interval as i64;
            Ok(age < interval)
        } else {
            Ok(false)
        }
    }

    pub fn get_scan_age(&self) -> Result<Option<u64>> {
        if !self.metadata_file.exists() {
            return Ok(None);
        }

        let file = File::open(&self.metadata_file)?;
        let reader = BufReader::new(file);
        let metadata: serde_json::Value = serde_json::from_reader(reader)?;

        if let Some(timestamp) = metadata.get("last_updated").and_then(|v| v.as_i64()) {
            let age = Utc::now().timestamp() - timestamp;
            Ok(Some(age as u64))
        } else {
            Ok(None)
        }
    }

    pub fn get_cache_size(&self) -> Result<u64> {
        if !self.cache_file.exists() {
            return Ok(0);
        }

        let metadata = std::fs::metadata(&self.cache_file)?;
        Ok(metadata.len())
    }

    pub fn get_cache_stats(&self) -> Result<CacheStats> {
        let size = self.get_cache_size()?;
        let package_count = if self.cache_file.exists() {
            self.load()?.len()
        } else {
            0
        };

        let age = self.get_scan_age()?;

        Ok(CacheStats {
            size,
            package_count,
            age,
            is_fresh: self.is_fresh()?,
        })
    }

    // ============ DEPENDENCY CACHE ============

    pub fn save_dependency_cache(&self, dep_cache: &DependencyCache) -> Result<()> {
        let file = File::create(&self.dep_cache_file)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, dep_cache)?;
        Ok(())
    }

    pub fn load_dependency_cache(&self) -> Result<Option<DependencyCache>> {
        if !self.dep_cache_file.exists() {
            return Ok(None);
        }

        let file = File::open(&self.dep_cache_file)?;
        let reader = BufReader::new(file);
        let cache: DependencyCache = serde_json::from_reader(reader)?;
        Ok(Some(cache))
    }

    pub fn dependency_cache_fresh(&self, current_package_count: usize) -> Result<bool> {
        if let Some(cache) = self.load_dependency_cache()? {
            // Check if cache is from this session and package count matches
            let age = Utc::now().timestamp() - cache.built_at;
            if age < 3600 && cache.package_count == current_package_count {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ============ USAGE CACHE ============

    pub fn save_usage_cache(&self, usage_cache: &UsageCache) -> Result<()> {
        let file = File::create(&self.usage_cache_file)?;
        let writer = BufWriter::new(file);
        serde_json::to_writer_pretty(writer, usage_cache)?;
        Ok(())
    }

    pub fn load_usage_cache(&self) -> Result<Option<UsageCache>> {
        if !self.usage_cache_file.exists() {
            return Ok(None);
        }

        let file = File::open(&self.usage_cache_file)?;
        let reader = BufReader::new(file);
        let cache: UsageCache = serde_json::from_reader(reader)?;
        Ok(Some(cache))
    }

    pub fn usage_cache_fresh(&self) -> Result<bool> {
        if let Some(cache) = self.load_usage_cache()? {
            // Check if cache is from this session (last hour)
            let age = Utc::now().timestamp() - cache.loaded_at;
            if age < 3600 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    // ============ CACHE MAINTENANCE ============

    pub fn clear(&self) -> Result<()> {
        if self.cache_file.exists() {
            std::fs::remove_file(&self.cache_file)?;
        }
        if self.metadata_file.exists() {
            std::fs::remove_file(&self.metadata_file)?;
        }
        if self.dep_cache_file.exists() {
            std::fs::remove_file(&self.dep_cache_file)?;
        }
        if self.usage_cache_file.exists() {
            std::fs::remove_file(&self.usage_cache_file)?;
        }
        Ok(())
    }

    pub fn clear_dependency_cache(&self) -> Result<()> {
        if self.dep_cache_file.exists() {
            std::fs::remove_file(&self.dep_cache_file)?;
        }
        Ok(())
    }

    pub fn clear_usage_cache(&self) -> Result<()> {
        if self.usage_cache_file.exists() {
            std::fs::remove_file(&self.usage_cache_file)?;
        }
        Ok(())
    }

    pub fn backup(&self) -> Result<PathBuf> {
        let backup_dir = self.config.cache_dir.join("backups");
        if !backup_dir.exists() {
            std::fs::create_dir_all(&backup_dir)?;
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let backup_file = backup_dir.join(format!("cache_{}.json", timestamp));

        if self.cache_file.exists() {
            std::fs::copy(&self.cache_file, &backup_file)?;
            Ok(backup_file)
        } else {
            Err(anyhow!("Cache file does not exist"))
        }
    }

    pub fn backup_all(&self) -> Result<Vec<PathBuf>> {
        let backup_dir = self.config.cache_dir.join("backups");
        if !backup_dir.exists() {
            std::fs::create_dir_all(&backup_dir)?;
        }

        let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let mut backups = Vec::new();

        // Backup package cache
        if self.cache_file.exists() {
            let backup_file = backup_dir.join(format!("packages_{}.json", timestamp));
            std::fs::copy(&self.cache_file, &backup_file)?;
            backups.push(backup_file);
        }

        // Backup dependency cache
        if self.dep_cache_file.exists() {
            let backup_file = backup_dir.join(format!("deps_{}.json", timestamp));
            std::fs::copy(&self.dep_cache_file, &backup_file)?;
            backups.push(backup_file);
        }

        // Backup usage cache
        if self.usage_cache_file.exists() {
            let backup_file = backup_dir.join(format!("usage_{}.json", timestamp));
            std::fs::copy(&self.usage_cache_file, &backup_file)?;
            backups.push(backup_file);
        }

        if backups.is_empty() {
            Err(anyhow!("No cache files to backup"))
        } else {
            Ok(backups)
        }
    }

    pub fn restore(&self, backup_file: &PathBuf) -> Result<()> {
        if !backup_file.exists() {
            return Err(anyhow!("Backup file does not exist"));
        }

        std::fs::copy(backup_file, &self.cache_file)?;
        self.load()?;
        Ok(())
    }

    pub fn restore_all(&self, backup_dir: &PathBuf) -> Result<()> {
        if !backup_dir.exists() {
            return Err(anyhow!("Backup directory does not exist"));
        }

        // Find the latest backups
        let mut package_backup = None;
        let mut dep_backup = None;
        let mut usage_backup = None;

        for entry in std::fs::read_dir(backup_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            
            if name.starts_with("packages_") && name.ends_with(".json") {
                package_backup = Some(entry.path());
            } else if name.starts_with("deps_") && name.ends_with(".json") {
                dep_backup = Some(entry.path());
            } else if name.starts_with("usage_") && name.ends_with(".json") {
                usage_backup = Some(entry.path());
            }
        }

        if let Some(path) = package_backup {
            std::fs::copy(&path, &self.cache_file)?;
        }

        if let Some(path) = dep_backup {
            std::fs::copy(&path, &self.dep_cache_file)?;
        }

        if let Some(path) = usage_backup {
            std::fs::copy(&path, &self.usage_cache_file)?;
        }

        self.load()?;
        Ok(())
    }

    pub fn merge(&self, other_cache: &PathBuf) -> Result<()> {
        if !other_cache.exists() {
            return Err(anyhow!("Other cache file does not exist"));
        }

        let current = self.load()?;
        let other: HashMap<String, Package> = {
            let file = File::open(other_cache)?;
            let reader = BufReader::new(file);
            let entries: Vec<CacheEntry> = serde_json::from_reader(reader)?;
            entries.into_iter().map(|e| (e.package.name.clone(), e.package)).collect()
        };

        let mut merged = current;
        for (name, pkg) in other {
            if let Some(existing) = merged.get(&name) {
                // Keep the one with the more recent installed date
                if pkg.installed_date.unwrap_or(0) > existing.installed_date.unwrap_or(0) {
                    merged.insert(name, pkg);
                }
            } else {
                merged.insert(name, pkg);
            }
        }

        self.save(&merged.into_values().collect::<Vec<_>>())?;
        Ok(())
    }

    pub fn validate(&self) -> Result<Vec<String>> {
        let packages = self.load()?;
        let mut issues = Vec::new();

        for (name, pkg) in packages {
            if !pkg.install_path.exists() {
                issues.push(format!("Package '{}' path does not exist: {}", name, pkg.install_path.display()));
            }

            if let Some(size) = pkg.size {
                let actual_size = crate::utils::get_path_size(&pkg.install_path).unwrap_or(0);
                if actual_size > 0 && size != actual_size {
                    issues.push(format!(
                        "Package '{}' size mismatch: cached {}, actual {}",
                        name,
                        crate::utils::format_size(size),
                        crate::utils::format_size(actual_size)
                    ));
                }
            }
        }

        Ok(issues)
    }

    pub fn compact(&self) -> Result<()> {
        let packages = self.load()?;
        self.save(&packages.into_values().collect::<Vec<_>>())?;
        
        // Also compact dependency and usage caches if they exist
        if let Some(dep_cache) = self.load_dependency_cache()? {
            self.save_dependency_cache(&dep_cache)?;
        }
        
        if let Some(usage_cache) = self.load_usage_cache()? {
            self.save_usage_cache(&usage_cache)?;
        }
        
        Ok(())
    }

    fn config_hash(&self) -> String {
        use sha2::{Sha256, Digest};
        let config_str = format!(
            "{:?}{:?}{:?}{:?}",
            self.config.scan_dirs,
            self.config.exclude_patterns,
            self.config.scan_interval,
            self.config.dependency_depth
        );
        let mut hasher = Sha256::new();
        hasher.update(config_str.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    // ============ CACHE STATS ============

    pub fn get_dependency_cache_stats(&self) -> Result<Option<DependencyCacheStats>> {
        if let Some(cache) = self.load_dependency_cache()? {
            let age = Utc::now().timestamp() - cache.built_at;
            Ok(Some(DependencyCacheStats {
                package_count: cache.package_count,
                dependency_count: cache.direct_deps.len(),
                reverse_dep_count: cache.reverse_deps.len(),
                max_depth: cache.max_depth,
                age_seconds: age,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_usage_cache_stats(&self) -> Result<Option<UsageCacheStats>> {
        if let Some(cache) = self.load_usage_cache()? {
            let age = Utc::now().timestamp() - cache.loaded_at;
            Ok(Some(UsageCacheStats {
                event_count: cache.event_count,
                package_count: cache.package_index.len(),
                last_event_timestamp: cache.last_event_timestamp,
                age_seconds: age,
            }))
        } else {
            Ok(None)
        }
    }
}

#[derive(Debug)]
pub struct CacheStats {
    pub size: u64,
    pub package_count: usize,
    pub age: Option<u64>,
    pub is_fresh: bool,
}

impl CacheStats {
    pub fn summary(&self) -> String {
        let age_str = self.age.map(|a| {
            let hours = a / 3600;
            let minutes = (a % 3600) / 60;
            format!("{}h {}m", hours, minutes)
        }).unwrap_or_else(|| "unknown".to_string());

        format!(
            "Cache size: {}, Packages: {}, Age: {}, Fresh: {}",
            crate::utils::format_size(self.size),
            self.package_count,
            age_str,
            if self.is_fresh { "yes" } else { "no" }
        )
    }
}

#[derive(Debug)]
pub struct DependencyCacheStats {
    pub package_count: usize,
    pub dependency_count: usize,
    pub reverse_dep_count: usize,
    pub max_depth: usize,
    pub age_seconds: i64,
}

impl DependencyCacheStats {
    pub fn summary(&self) -> String {
        format!(
            "Packages: {}, Dependencies: {}, Reverse deps: {}, Max depth: {}, Age: {}s",
            self.package_count,
            self.dependency_count,
            self.reverse_dep_count,
            self.max_depth,
            self.age_seconds
        )
    }
}

#[derive(Debug)]
pub struct UsageCacheStats {
    pub event_count: usize,
    pub package_count: usize,
    pub last_event_timestamp: i64,
    pub age_seconds: i64,
}

impl UsageCacheStats {
    pub fn summary(&self) -> String {
        let last_event = DateTime::<Local>::from(Utc.timestamp_opt(self.last_event_timestamp, 0).unwrap());
        format!(
            "Events: {}, Packages: {}, Last event: {}, Age: {}s",
            self.event_count,
            self.package_count,
            last_event.format("%Y-%m-%d %H:%M:%S"),
            self.age_seconds
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use crate::models::{PackageSource, Package};

    #[test]
    fn test_cache_manager_creation() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;
        assert!(manager.cache_file.to_str().unwrap().contains("packages.db.json"));
        assert!(manager.metadata_file.to_str().unwrap().contains("metadata.json"));
        assert!(manager.dep_cache_file.to_str().unwrap().contains("dependency_cache.json"));
        assert!(manager.usage_cache_file.to_str().unwrap().contains("usage_cache.json"));

        Ok(())
    }

    #[test]
    fn test_save_and_load() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let packages = vec![
            Package {
                name: "test1".to_string(),
                version: Some("1.0.0".to_string()),
                source: PackageSource::Pkg,
                install_path: PathBuf::from("/usr/bin/test1"),
                size: Some(1024),
                dependencies: None,
                installed_date: None,
                last_used: None,
                usage_count: None,
                checksum: None,
            },
            Package {
                name: "test2".to_string(),
                version: Some("2.0.0".to_string()),
                source: PackageSource::Cargo,
                install_path: PathBuf::from("/usr/bin/test2"),
                size: Some(2048),
                dependencies: None,
                installed_date: None,
                last_used: None,
                usage_count: None,
                checksum: None,
            },
        ];

        manager.save(&packages)?;

        let loaded = manager.load()?;
        assert_eq!(loaded.len(), 2);
        assert!(loaded.contains_key("test1"));
        assert!(loaded.contains_key("test2"));

        let all = manager.get_all();
        assert_eq!(all.len(), 2);

        Ok(())
    }

    #[test]
    fn test_dependency_cache_persistence() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let mut dep_cache = DependencyCache::new();
        dep_cache.direct_deps.insert("pkg1".to_string(), vec!["dep1".to_string(), "dep2".to_string()]);
        dep_cache.direct_deps.insert("pkg2".to_string(), vec!["dep1".to_string()]);
        dep_cache.reverse_deps.insert("dep1".to_string(), vec!["pkg1".to_string(), "pkg2".to_string()]);
        dep_cache.package_count = 2;
        dep_cache.built_at = Utc::now().timestamp();

        manager.save_dependency_cache(&dep_cache)?;

        let loaded = manager.load_dependency_cache()?;
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.package_count, 2);
        assert!(loaded.direct_deps.contains_key("pkg1"));
        assert!(loaded.reverse_deps.contains_key("dep1"));

        Ok(())
    }

    #[test]
    fn test_usage_cache_persistence() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let mut usage_cache = UsageCache::new();
        usage_cache.events = vec![
            PackageEvent {
                timestamp: Utc::now().timestamp(),
                package: "pkg1".to_string(),
                action: "INSTALL".to_string(),
                source: PackageSource::Pkg,
                details: None,
                pid: None,
                user: None,
            },
        ];
        usage_cache.package_index.insert("pkg1".to_string(), vec![0]);
        usage_cache.event_count = 1;
        usage_cache.loaded_at = Utc::now().timestamp();

        manager.save_usage_cache(&usage_cache)?;

        let loaded = manager.load_usage_cache()?;
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.event_count, 1);
        assert!(loaded.package_index.contains_key("pkg1"));

        Ok(())
    }

    #[test]
    fn test_backup_and_restore() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let packages = vec![Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: PackageSource::Pkg,
            install_path: PathBuf::from("/usr/bin/test"),
            size: Some(1024),
            dependencies: None,
            installed_date: None,
            last_used: None,
            usage_count: None,
            checksum: None,
        }];
        manager.save(&packages)?;

        let backup = manager.backup()?;
        assert!(backup.exists());

        // Clear and restore
        manager.clear()?;
        assert!(!manager.cache_file.exists());

        manager.restore(&backup)?;
        assert!(manager.cache_file.exists());

        let loaded = manager.load()?;
        assert_eq!(loaded.len(), 1);

        Ok(())
    }

    #[test]
    fn test_backup_all() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        // Create some cache data
        let packages = vec![Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: PackageSource::Pkg,
            install_path: PathBuf::from("/usr/bin/test"),
            size: Some(1024),
            dependencies: None,
            installed_date: None,
            last_used: None,
            usage_count: None,
            checksum: None,
        }];
        manager.save(&packages)?;

        let dep_cache = DependencyCache::new();
        manager.save_dependency_cache(&dep_cache)?;

        let usage_cache = UsageCache::new();
        manager.save_usage_cache(&usage_cache)?;

        let backups = manager.backup_all()?;
        assert!(!backups.is_empty());
        
        // Should have at least package backup
        assert!(backups.iter().any(|p| p.to_string_lossy().contains("packages_")));

        Ok(())
    }

    #[test]
    fn test_cache_stats() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let packages = vec![Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: PackageSource::Pkg,
            install_path: PathBuf::from("/usr/bin/test"),
            size: Some(1024),
            dependencies: None,
            installed_date: None,
            last_used: None,
            usage_count: None,
            checksum: None,
        }];
        manager.save(&packages)?;

        let stats = manager.get_cache_stats()?;
        assert_eq!(stats.package_count, 1);
        assert!(stats.size > 0);

        Ok(())
    }

    #[test]
    fn test_validate() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let pkg = Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: PackageSource::Pkg,
            install_path: PathBuf::from("/nonexistent/path"),
            size: Some(1024),
            dependencies: None,
            installed_date: None,
            last_used: None,
            usage_count: None,
            checksum: None,
        };
        manager.add_package(pkg)?;

        let issues = manager.validate()?;
        assert!(!issues.is_empty());
        assert!(issues[0].contains("path does not exist"));

        Ok(())
    }

    #[test]
    fn test_compact() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let packages = vec![
            Package {
                name: "test1".to_string(),
                version: Some("1.0.0".to_string()),
                source: PackageSource::Pkg,
                install_path: PathBuf::from("/usr/bin/test1"),
                size: Some(1024),
                dependencies: None,
                installed_date: None,
                last_used: None,
                usage_count: None,
                checksum: None,
            },
            Package {
                name: "test2".to_string(),
                version: Some("2.0.0".to_string()),
                source: PackageSource::Cargo,
                install_path: PathBuf::from("/usr/bin/test2"),
                size: Some(2048),
                dependencies: None,
                installed_date: None,
                last_used: None,
                usage_count: None,
                checksum: None,
            },
        ];
        manager.save(&packages)?;

        // Compact should work without errors
        manager.compact()?;

        let loaded = manager.load()?;
        assert_eq!(loaded.len(), 2);

        Ok(())
    }

    #[test]
    fn test_freshness() -> Result<()> {
        let dir = tempdir()?;
        let mut config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());
        config.scan_interval = 3600; // 1 hour

        let manager = CacheManager::new(Arc::new(config))?;

        // No cache yet
        assert!(!manager.is_fresh()?);

        // Save some data
        let packages = vec![Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: PackageSource::Pkg,
            install_path: PathBuf::from("/usr/bin/test"),
            size: Some(1024),
            dependencies: None,
            installed_date: None,
            last_used: None,
            usage_count: None,
            checksum: None,
        }];
        manager.save(&packages)?;

        // Should be fresh now
        assert!(manager.is_fresh()?);

        Ok(())
    }
}