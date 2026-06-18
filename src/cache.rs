use anyhow::{Result, anyhow};
use serde_json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::sync::Arc;
use chrono::{Utc, DateTime};

use crate::config::Config;
use crate::models::{Package, CacheEntry};

#[derive(Clone)]
pub struct CacheManager {
    config: Arc<Config>,
    cache_file: PathBuf,
    metadata_file: PathBuf,
}

impl CacheManager {
    pub fn new(config: Arc<Config>) -> Result<Self> {
        let manager = Self {
            config: config.clone(),
            cache_file: config.db_file.clone(),
            metadata_file: config.cache_dir.join("metadata.json"),
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

        Ok(())
    }

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

    pub fn clear(&self) -> Result<()> {
        if self.cache_file.exists() {
            std::fs::remove_file(&self.cache_file)?;
        }
        if self.metadata_file.exists() {
            std::fs::remove_file(&self.metadata_file)?;
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

    pub fn restore(&self, backup_file: &PathBuf) -> Result<()> {
        if !backup_file.exists() {
            return Err(anyhow!("Backup file does not exist"));
        }

        std::fs::copy(backup_file, &self.cache_file)?;
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

    pub fn compact(&self) -> Result<()> {
        let packages = self.load()?;
        self.save(&packages.into_values().collect::<Vec<_>>())?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_cache_manager_creation() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;
        assert!(manager.cache_file.to_str().unwrap().contains("packages.db.json"));
        assert!(manager.metadata_file.to_str().unwrap().contains("metadata.json"));

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
                source: crate::models::PackageSource::Pkg,
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
                source: crate::models::PackageSource::Cargo,
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
    fn test_add_and_remove() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let pkg = Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: crate::models::PackageSource::Pkg,
            install_path: PathBuf::from("/usr/bin/test"),
            size: Some(1024),
            dependencies: None,
            installed_date: None,
            last_used: None,
            usage_count: None,
            checksum: None,
        };

        manager.add_package(pkg.clone())?;
        let loaded = manager.load()?;
        assert_eq!(loaded.len(), 1);

        manager.remove_package("test")?;
        let loaded = manager.load()?;
        assert_eq!(loaded.len(), 0);

        Ok(())
    }

    #[test]
    fn test_is_fresh() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        // No metadata file yet
        assert!(!manager.is_fresh()?);

        // Save some data
        let packages = vec![Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: crate::models::PackageSource::Pkg,
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

    #[test]
    fn test_backup_and_restore() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let packages = vec![Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: crate::models::PackageSource::Pkg,
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
    fn test_validate() -> Result<()> {
        let dir = tempdir()?;
        let config = crate::config::Config::default()
            .with_custom_dir(dir.path().to_path_buf());

        let manager = CacheManager::new(Arc::new(config))?;

        let pkg = Package {
            name: "test".to_string(),
            version: Some("1.0.0".to_string()),
            source: crate::models::PackageSource::Pkg,
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
}