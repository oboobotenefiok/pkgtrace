use anyhow::{Result, anyhow};
use chrono::{DateTime, Local, Utc, TimeZone};
use parking_lot::RwLock;
use serde_json;
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    io::{BufReader, BufWriter, BufRead},
    path::PathBuf,
    process::Command,
    sync::Arc,
};    

use crate::{
    config::Config,
    models::*,
    scanner::PackageScanner,
    utils,
    cache::CacheManager,
    logger::Logger,
};

#[derive(Clone)]
pub struct Tracker {
    config: Arc<Config>,
    cache: Arc<RwLock<HashMap<String, Package>>>,
    cache_manager: CacheManager,
    logger: Logger,
}

impl Tracker {
    pub fn new(config: Config) -> Result<Self> {
        let config_arc = Arc::new(config);
        let cache_manager = CacheManager::new(config_arc.clone())?;
        let logger = Logger::new(config_arc.clone())?;
        
        let tracker = Self {
            config: config_arc.clone(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_manager,
            logger,
        };
        
        tracker.load_cache()?;
        Ok(tracker)
    }

    fn load_cache(&self) -> Result<()> {
        self.cache_manager.load()?;
        let packages = self.cache_manager.get_all();
        let mut cache = self.cache.write();
        cache.clear();
        for pkg in packages {
            cache.insert(pkg.name.clone(), pkg);
        }
        Ok(())
    }

    fn save_cache(&self) -> Result<()> {
        let packages: Vec<Package> = self.cache.read().values().cloned().collect();
        self.cache_manager.save(&packages)?;
        Ok(())
    }

    pub fn log_event(&self, event: &PackageEvent) -> Result<()> {
        self.logger.log_event(event)
    }

// This function runs really and I'm looking for ways to optimize it.
// It's being called from the mod cmd commands in about five places. We'll need a cache for somethings.
    pub fn get_installed_packages_all(&self) -> Result<Vec<Package>> {
    // We call scanner and pass the tracker-config we received from the cmd - which came from main - there.
        let packages = PackageScanner::scan_all(self.config.clone())?;
        Ok(packages.into_iter().collect())
    }

    pub fn scan_all_packages(&self, force: bool) -> Result<Vec<Package>> {
        if !force && self.cache_manager.is_fresh()? {
            return self.get_installed_packages_all();
        }

        self.logger.info("Starting package scan")?;
        let packages = PackageScanner::scan_all(self.config.clone())?;
        let packages_vec: Vec<Package> = packages.into_iter().collect();

        self.cache_manager.save(&packages_vec)?;
        
        let mut cache = self.cache.write();
        cache.clear();
        for pkg in &packages_vec {
            cache.insert(pkg.name.clone(), pkg.clone());
        }
        drop(cache);

        self.logger.info(&format!("Scan complete: {} packages found", packages_vec.len()))?;
        Ok(packages_vec)
    }

    pub fn get_dependencies(&self, package: &str) -> Result<Vec<String>> {
        let output = Command::new("pkg")
            .arg("show")
            .arg(package)
            .output();

        if let Err(e) = output {
            return Err(anyhow!("Failed to run pkg show for {}: {}", package, e));
        }

        let output = output.unwrap();
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("pkg show failed for {}: {}", package, stderr));
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut deps = Vec::new();
        let mut in_deps = false;

        for line in stdout.lines() {
            if line.starts_with("Depends:") {
                in_deps = true;
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() > 1 {
                    let dep_list = parts[1].trim();
                    for dep in dep_list.split(',') {
                        let clean = dep.trim().split_whitespace().next().unwrap_or("").to_string();
                        if !clean.is_empty() {
                            deps.push(clean);
                        }
                    }
                }
            } else if in_deps && line.starts_with(' ') {
                for dep in line.trim().split(',') {
                    let clean = dep.trim().split_whitespace().next().unwrap_or("").to_string();
                    if !clean.is_empty() {
                        deps.push(clean);
                    }
                }
            } else if in_deps && !line.starts_with(' ') {
                break;
            }
        }

        Ok(deps)
    }

    pub fn get_all_dependencies(&self, packages: &[String]) -> Result<HashSet<String>> {
        let mut all_deps = HashSet::new();
        let mut to_process: Vec<String> = packages.to_vec();
        let mut processed = HashSet::new();
        let core_packages = Self::get_core_packages();

        let depth_limit = self.config.dependency_depth;
        let mut depth_map: HashMap<String, usize> = HashMap::new();

        for pkg in packages {
            depth_map.insert(pkg.clone(), 0);
        }

        while let Some(pkg) = to_process.pop() {
            if processed.contains(&pkg) {
                continue;
            }

            let current_depth = *depth_map.get(&pkg).unwrap_or(&0);
            if current_depth >= depth_limit {
                continue;
            }

            processed.insert(pkg.clone());

            if core_packages.contains(&pkg.as_str()) {
                continue;
            }

            if let Ok(deps) = self.get_dependencies(&pkg) {
                for dep in deps {
                    if !processed.contains(&dep) && !all_deps.contains(&dep) {
                        all_deps.insert(dep.clone());
                        to_process.push(dep.clone());
                        depth_map.insert(dep, current_depth + 1);
                    }
                }
            }
        }

        Ok(all_deps)
    }

    pub fn get_reverse_dependencies(&self, package: &str) -> Result<Vec<String>> {
        let all_packages = self.get_installed_packages_all()?;
        let mut reverse_deps = Vec::new();

        for pkg in all_packages {
            if let Ok(deps) = self.get_dependencies(&pkg.name) {
                if deps.contains(&package.to_string()) {
                    reverse_deps.push(pkg.name);
                }
            }
        }

        Ok(reverse_deps)
    }

    pub fn get_used_packages(&self) -> Result<HashSet<String>> {
        let mut used = HashSet::new();

        if self.config.log_file.exists() {
            let file = File::open(&self.config.log_file)?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                if let Ok(line) = line {
                    if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                        if event.action == "INSTALL" || event.action == "USE" {
                            used.insert(event.package);
                        }
                    }
                }
            }
        }

        Ok(used)
    }

    pub fn get_core_packages() -> HashSet<&'static str> {
        let mut core = HashSet::new();
        core.insert("bash");
        core.insert("coreutils");
        core.insert("findutils");
        core.insert("grep");
        core.insert("sed");
        core.insert("awk");
        core.insert("tar");
        core.insert("gzip");
        core.insert("xz-utils");
        core.insert("termux-tools");
        core.insert("termux-exec");
        core.insert("termux-keyring");
        core.insert("termux-am");
        core.insert("termux-api");
        core.insert("apk-tools");
        core.insert("apt");
        core.insert("dpkg");
        core.insert("busybox");
        core.insert("ca-certificates");
        core.insert("openssl");
        core.insert("libc++");
        core.insert("libandroid-support");
        core.insert("libc++_shared");
        core.insert("zlib");
        core.insert("ncurses");
        core.insert("readline");
        core
    }

    pub fn find_unused(&self, days_threshold: u32) -> Result<Vec<UnusedPackage>> {
        let installed = self.get_installed_packages_all()?;
        let core_packages = Self::get_core_packages();
        let current_time = Utc::now().timestamp();
        let threshold_seconds = (days_threshold as i64) * 86400;

        let mut events = Vec::new();
        if self.config.log_file.exists() {
            let file = File::open(&self.config.log_file)?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                if let Ok(line) = line {
                    if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                        events.push(event);
                    }
                }
            }
        }

        let used_packages = self.get_used_packages()?;
        let mut unused = Vec::new();

        for pkg in installed {
            if core_packages.contains(&pkg.name.as_str()) && self.config.protect_core {
                continue;
            }

            if used_packages.contains(&pkg.name) {
                continue;
            }

            let entries: Vec<_> = events
                .iter()
                .filter(|e| e.package == pkg.name)
                .collect();

            let last_used = entries
                .iter()
                .max_by_key(|e| e.timestamp)
                .map(|e| e.timestamp);

            let days_unused = if let Some(last) = last_used {
                let age = current_time - last;
                if age > threshold_seconds {
                    (age / 86400) as u32
                } else {
                    continue;
                }
            } else {
                0
            };

            unused.push(UnusedPackage {
                name: pkg.name.clone(),
                source: pkg.source,
                last_used,
                days_unused,
                size: pkg.size,
                status: PackageStatus::Unused,
                install_path: pkg.install_path,
            });
        }

        Ok(unused)
    }

    pub fn find_unused_with_deps(&self, days_threshold: u32) -> Result<Vec<UnusedPackage>> {
        let mut unused = self.find_unused(days_threshold)?;
        let used_packages = self.get_used_packages()?;
        let used_vec: Vec<String> = used_packages.into_iter().collect();
        let dependency_set = self.get_all_dependencies(&used_vec)?;

        let mut filtered = Vec::new();
        for mut pkg in unused {
            let is_dependency = dependency_set.contains(&pkg.name);
            if is_dependency {
                pkg.status = PackageStatus::Dependency;
            } else {
                if Self::get_core_packages().contains(&pkg.name.as_str()) {
                    pkg.status = PackageStatus::SystemCritical;
                }
                filtered.push(pkg);
            }
        }

        Ok(filtered)
    }

    pub fn explain_protection(&self, package: &str) -> Result<String> {
        let used_packages = self.get_used_packages()?;

        if used_packages.contains(package) {
            return Ok(format!("Package '{}' is directly used (you installed it)", package));
        }

        if Self::get_core_packages().contains(&package) && self.config.protect_core {
            return Ok(format!("Package '{}' is a core system package", package));
        }

        let used_vec: Vec<String> = used_packages.iter().cloned().collect();
        let deps = self.get_all_dependencies(&used_vec)?;

        if deps.contains(package) {
            for used_pkg in &used_packages {
                if let Ok(pkg_deps) = self.get_dependencies(used_pkg) {
                    if pkg_deps.contains(&package.to_string()) {
                        return Ok(format!("Package '{}' is a dependency of '{}'", package, used_pkg));
                    }
                }
            }
            return Ok(format!("Package '{}' is a dependency of used packages", package));
        }

        Ok(format!("Package '{}' is not protected", package))
    }

    pub fn get_package_info(&self, package: &str) -> Result<PackageInfo> {
        let installed = self.get_installed_packages_all()?;
        let pkg = installed
            .iter()
            .find(|p| p.name == package)
            .ok_or_else(|| anyhow!("Package '{}' not found", package))?
            .clone();

        let dependencies = self.get_dependencies(package).ok();
        let reverse_deps = self.get_reverse_dependencies(package).ok();

        let installed_date = self.get_install_date(package);
        let last_used_date = self.get_last_used_date(package);
        let usage_count = self.get_usage_count(package);

        Ok(PackageInfo {
            name: pkg.name,
            version: pkg.version,
            source: pkg.source,
            install_path: pkg.install_path,
            size: pkg.size,
            installed_date,
            last_used_date,
            dependencies,
            reverse_dependencies: reverse_deps,
            usage_count,
            checksum: pkg.checksum,
        })
    }

    fn get_install_date(&self, package: &str) -> Option<String> {
        if !self.config.log_file.exists() {
            return None;
        }

        let file = File::open(&self.config.log_file).ok()?;
        let reader = BufReader::new(file);

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    if event.package == package && event.action == "INSTALL" {
                        let dt = DateTime::<Local>::from(Utc.timestamp_opt(event.timestamp, 0).unwrap());
                        return Some(dt.format("%Y-%m-%d %H:%M:%S").to_string());
                    }
                }
            }
        }

        None
    }

    fn get_last_used_date(&self, package: &str) -> Option<String> {
        if !self.config.log_file.exists() {
            return None;
        }

        let file = File::open(&self.config.log_file).ok()?;
        let reader = BufReader::new(file);

        let mut last_ts = None;
        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    if event.package == package && (event.action == "USE" || event.action == "INSTALL") {
                        if last_ts.is_none() || event.timestamp > last_ts.unwrap() {
                            last_ts = Some(event.timestamp);
                        }
                    }
                }
            }
        }

        last_ts.map(|ts| {
            let dt = DateTime::<Local>::from(Utc.timestamp_opt(ts, 0).unwrap());
            dt.format("%Y-%m-%d %H:%M:%S").to_string()
        })
    }

    fn get_usage_count(&self, package: &str) -> Option<u64> {
        if !self.config.log_file.exists() {
            return None;
        }

        let file = File::open(&self.config.log_file).ok()?;
        let reader = BufReader::new(file);

        let mut count = 0;
        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    if event.package == package && (event.action == "USE" || event.action == "INSTALL") {
                        count += 1;
                    }
                }
            }
        }

        Some(count)
    }

    pub fn remove_package(&self, unused_pkg: &UnusedPackage) -> Result<()> {
        if self.config.backup_before_remove {
            self.backup_package(unused_pkg)?;
        }

        match unused_pkg.source {
            PackageSource::Pkg => {
                let output = Command::new("pkg")
                    .arg("uninstall")
                    .arg(&unused_pkg.name)
                    .output()?;

                if output.status.success() {
                    self.log_event(&PackageEvent {
                        timestamp: Utc::now().timestamp(),
                        package: unused_pkg.name.clone(),
                        action: "REMOVE".to_string(),
                        source: PackageSource::Pkg,
                        details: None,
                        pid: None,
                        user: None,
                    })?;

                    let mut cache = self.cache.write();
                    cache.remove(&unused_pkg.name);
                    drop(cache);
                    self.save_cache()?;

                    self.logger.info(&format!("Removed package: {}", unused_pkg.name))?;
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(anyhow!("Failed to remove package: {}", stderr))
                }
            }
            _ => {
                if unused_pkg.install_path.exists() {
                    if unused_pkg.install_path.is_dir() {
                        std::fs::remove_dir_all(&unused_pkg.install_path)?;
                    } else {
                        std::fs::remove_file(&unused_pkg.install_path)?;
                    }
                }

                self.log_event(&PackageEvent {
                    timestamp: Utc::now().timestamp(),
                    package: unused_pkg.name.clone(),
                    action: "REMOVE".to_string(),
                    source: unused_pkg.source.clone(),
                    details: None,
                    pid: None,
                    user: None,
                })?;

                let mut cache = self.cache.write();
                cache.remove(&unused_pkg.name);
                drop(cache);
                self.save_cache()?;

                self.logger.info(&format!("Removed package: {}", unused_pkg.name))?;
                Ok(())
            }
        }
    }

    fn backup_package(&self, pkg: &UnusedPackage) -> Result<()> {
        let backup_dir = self.config.cache_dir.join("backups");
        std::fs::create_dir_all(&backup_dir)?;

        let timestamp = Utc::now().timestamp();
        let backup_path = backup_dir.join(format!("{}_{}", pkg.name, timestamp));

        if pkg.install_path.exists() {
            if pkg.install_path.is_dir() {
                let output = Command::new("cp")
                    .arg("-r")
                    .arg(&pkg.install_path)
                    .arg(&backup_path)
                    .output()?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    self.logger.warning(&format!("Backup failed for {}: {}", pkg.name, stderr))?;
                } else {
                    self.logger.info(&format!("Backed up {} to {}", pkg.name, backup_path.display()))?;
                }
            }
        }

        Ok(())
    }

    pub fn remove_package_from_cache(&self, package: &str) -> Result<()> {
        let mut cache = self.cache.write();
        cache.remove(package);
        drop(cache);
        self.save_cache()?;
        self.logger.info(&format!("Removed {} from cache", package))?;
        Ok(())
    }

    pub fn print_dependency_tree(&self, package: &str, max_depth: usize) -> Result<()> {
        let deps = self.get_dependencies(package)?;
        println!("Dependency tree for '{}' (max depth: {}):", package, max_depth);
        self.print_tree(package, &deps, 0, max_depth, &mut HashSet::new())?;
        Ok(())
    }

    fn print_tree(&self, _package: &str, deps: &[String], indent: usize, max_depth: usize, visited: &mut HashSet<String>) -> Result<()> {
        if indent >= max_depth {
            println!("{}... (depth limit reached)", "  ".repeat(indent));
            return Ok(());
        }

        let prefix = "  ".repeat(indent);
        for (i, dep) in deps.iter().enumerate() {
            let is_last = i == deps.len() - 1;
            let connector = if is_last { "└── " } else { "├── " };

            let status = if visited.contains(dep) {
                " [cycle]"
            } else {
                ""
            };

            println!("{}{}{}{}", prefix, connector, dep, status);

            if !visited.contains(dep) {
                visited.insert(dep.clone());
                if let Ok(sub_deps) = self.get_dependencies(dep) {
                    if !sub_deps.is_empty() {
                        let new_indent = if is_last { indent + 1 } else { indent + 1 };
                        self.print_tree(dep, &sub_deps, new_indent, max_depth, visited)?;
                    }
                }
                visited.remove(dep);
            }
        }

        Ok(())
    }

    pub fn install_package(&self, package: &Package) -> Result<()> {
        match package.source {
            PackageSource::Pkg => {
                let output = Command::new("pkg")
                    .arg("install")
                    .arg(&package.name)
                    .output()?;

                if output.status.success() {
                    self.log_event(&PackageEvent {
                        timestamp: Utc::now().timestamp(),
                        package: package.name.clone(),
                        action: "INSTALL".to_string(),
                        source: PackageSource::Pkg,
                        details: None,
                        pid: None,
                        user: None,
                    })?;

                    let mut cache = self.cache.write();
                    cache.insert(package.name.clone(), package.clone());
                    drop(cache);
                    self.save_cache()?;

                    self.logger.info(&format!("Installed package: {}", package.name))?;
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(anyhow!("Failed to install package: {}", stderr))
                }
            }
            _ => {
                Err(anyhow!("Cannot automatically install {} packages", package.source))
            }
        }
    }

    pub fn get_stats(&self) -> Result<Stats> {
        let packages = self.get_installed_packages_all()?;
        let used = self.get_used_packages()?;
        let total_size: u64 = packages.iter().filter_map(|p| p.size).sum();

        let mut by_source: HashMap<String, (usize, u64)> = HashMap::new();
        for pkg in &packages {
            let entry = by_source.entry(pkg.source.to_string()).or_insert((0, 0));
            entry.0 += 1;
            if let Some(size) = pkg.size {
                entry.1 += size;
            }
        }

        let mut by_source_vec: Vec<_> = by_source.into_iter().map(|(k, v)| (k, v.0, v.1)).collect();
        by_source_vec.sort_by(|a, b| b.1.cmp(&a.1));

        let mut largest = packages.clone();
        largest.sort_by(|a, b| b.size.unwrap_or(0).cmp(&a.size.unwrap_or(0)));
        let largest_packages = largest.into_iter().take(10).collect();

        let mut oldest = packages.clone();
        oldest.sort_by(|a, b| a.installed_date.unwrap_or(0).cmp(&b.installed_date.unwrap_or(0)));
        let oldest_packages = oldest.into_iter().take(10).collect();

        let mut newest = packages.clone();
        newest.sort_by(|a, b| b.installed_date.unwrap_or(0).cmp(&a.installed_date.unwrap_or(0)));
        let newest_packages = newest.into_iter().take(10).collect();

        let avg_size = if !packages.is_empty() {
            total_size / packages.len() as u64
        } else {
            0
        };

        let mut log_entries = 0;
        let mut install_events = 0;
        let mut remove_events = 0;

        if self.config.log_file.exists() {
            let file = File::open(&self.config.log_file)?;
            let reader = BufReader::new(file);

            for line in reader.lines() {
                if let Ok(line) = line {
                    log_entries += 1;
                    if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                        if event.action == "INSTALL" {
                            install_events += 1;
                        } else if event.action == "REMOVE" {
                            remove_events += 1;
                        }
                    }
                }
            }
        }

        Ok(Stats {
            total_packages: packages.len(),
            used_packages: used.len(),
            total_size,
            by_source: by_source_vec,
            largest_packages,
            oldest_packages,
            newest_packages,
            average_package_size: avg_size,
            total_log_entries: log_entries,
            total_install_events: install_events,
            total_remove_events: remove_events,
        })
    }

    pub fn get_tracker(&self) -> Tracker {
        Tracker {
            config: self.config.clone(),
            cache: self.cache.clone(),
            cache_manager: self.cache_manager.clone(),
            logger: self.logger.clone(),
        }
    }
}