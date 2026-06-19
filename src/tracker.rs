use anyhow::{anyhow, Result};
use chrono::{DateTime, Local, TimeZone, Utc};
use parking_lot::RwLock;
use serde_json;
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::Command,
    sync::Arc,
};

use crate::{
    cache::CacheManager, config::Config, logger::Logger, models::*, scanner::PackageScanner, utils,
};

#[derive(Clone)]
pub struct Tracker {
    config: Arc<Config>,
    cache: Arc<RwLock<HashMap<String, Package>>>,
    pub cache_manager: CacheManager,
    logger: Logger,
    dependency_cache: Arc<RwLock<DependencyCache>>,
    usage_cache: Arc<RwLock<UsageCache>>,
    file_map_cache: Arc<RwLock<FileMapCache>>,
    last_rebuild: Arc<RwLock<i64>>,
}

impl Tracker {
    pub fn new(config: Config) -> Result<Self> {
        let config_arc = Arc::new(config);
        let cache_manager = CacheManager::new(config_arc.clone())?;
        let logger = Logger::new(config_arc.clone())?;

        let tracker = Self {
            config: config_arc.clone(),
            cache: Arc::new(RwLock::new(HashMap::new())),
            cache_manager: cache_manager.clone(),
            logger,
            dependency_cache: Arc::new(RwLock::new(DependencyCache::new())),
            usage_cache: Arc::new(RwLock::new(UsageCache::new())),
            file_map_cache: Arc::new(RwLock::new(FileMapCache::new())),
            last_rebuild: Arc::new(RwLock::new(0)),
        };

        tracker.load_cache()?;
        tracker.rebuild_caches()?;
        tracker.load_file_map_cache()?;

        Ok(tracker)
    }

    fn load_cache(&self) -> Result<()> {
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

    pub fn rebuild_caches(&self) -> Result<()> {
        let start = std::time::Instant::now();

        let usage_cache = self.build_usage_cache()?;
        *self.usage_cache.write() = usage_cache;

        let dep_cache = self.build_dependency_cache()?;
        *self.dependency_cache.write() = dep_cache;

        *self.last_rebuild.write() = Utc::now().timestamp();

        let duration = start.elapsed();
        self.logger
            .info(&format!("Caches rebuilt in {:?}", duration))?;

        Ok(())
    }

    fn build_usage_cache(&self) -> Result<UsageCache> {
        let mut cache = UsageCache::new();

        if !self.config.log_file.exists() {
            return Ok(cache);
        }

        let file = File::open(&self.config.log_file)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();
        let mut package_index: HashMap<String, Vec<usize>> = HashMap::new();
        let mut last_timestamp = 0;

        for line in reader.lines() {
            if let Ok(line) = line {
                if let Ok(event) = serde_json::from_str::<PackageEvent>(&line) {
                    let idx = events.len();
                    events.push(event.clone());
                    package_index
                        .entry(event.package.clone())
                        .or_default()
                        .push(idx);
                    if event.timestamp > last_timestamp {
                        last_timestamp = event.timestamp;
                    }
                }
            }
        }

        cache.events = events;
        cache.package_index = package_index;
        cache.loaded_at = Utc::now().timestamp();
        cache.last_event_timestamp = last_timestamp;
        cache.event_count = cache.events.len();

        Ok(cache)
    }

    fn build_dependency_cache(&self) -> Result<DependencyCache> {
        use indicatif::{ProgressBar, ProgressStyle};

        let packages = self.get_installed_packages_all()?;
        let mut cache = DependencyCache::new();

        if packages.is_empty() {
            return Ok(cache);
        }

        let pb = ProgressBar::new(packages.len() as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} packages ({eta})")
            .unwrap()
            .progress_chars("#>-"));

        pb.set_message("Fetching dependencies");

        let pkg_names: Vec<&str> = packages.iter().map(|p| p.name.as_str()).collect();
        let deps_map = self.get_dependencies_batch(&pkg_names)?;

        for (_i, pkg) in packages.iter().enumerate() {
            pb.inc(1);

            let deps = deps_map.get(&pkg.name).cloned().unwrap_or_default();
            cache.direct_deps.insert(pkg.name.clone(), deps.clone());

            for dep in deps {
                cache
                    .reverse_deps
                    .entry(dep)
                    .or_insert_with(Vec::new)
                    .push(pkg.name.clone());
            }
        }

        pb.finish_with_message("Dependencies cached");

        cache.built_at = Utc::now().timestamp();
        cache.package_count = packages.len();
        cache.max_depth = 0;

        Ok(cache)
    }

    fn get_dependencies_batch(&self, packages: &[&str]) -> Result<HashMap<String, Vec<String>>> {
        if packages.is_empty() {
            return Ok(HashMap::new());
        }

        let mut cmd = Command::new("pkg");
        cmd.arg("show");
        for pkg in packages {
            cmd.arg(pkg);
        }

        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                self.logger.warning(&format!(
                    "Batch pkg show failed, falling back to individual: {}",
                    e
                ))?;
                return self.get_dependencies_batch_fallback(packages);
            }
        };

        if !output.status.success() {
            self.logger
                .warning("Batch pkg show returned non-zero, falling back to individual")?;
            return self.get_dependencies_batch_fallback(packages);
        }

        let stdout = String::from_utf8(output.stdout)?;
        let mut results = HashMap::new();
        let mut current_pkg: Option<String> = None;
        let mut current_deps: Vec<String> = Vec::new();
        let mut in_deps = false;

        for line in stdout.lines() {
            if line.starts_with("Package:") {
                if let Some(name) = current_pkg.take() {
                    results.insert(name, current_deps.clone());
                    current_deps.clear();
                }
                let name = line.trim_start_matches("Package:").trim().to_string();
                current_pkg = Some(name);
                in_deps = false;
            } else if line.starts_with("Depends:") {
                in_deps = true;
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() > 1 {
                    let dep_list = parts[1].trim();
                    for dep in dep_list.split(',') {
                        let clean = dep
                            .trim()
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if !clean.is_empty() {
                            current_deps.push(clean);
                        }
                    }
                }
            } else if in_deps && line.starts_with(' ') {
                for dep in line.trim().split(',') {
                    let clean = dep
                        .trim()
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !clean.is_empty() {
                        current_deps.push(clean);
                    }
                }
            } else if in_deps && !line.starts_with(' ') && !line.is_empty() {
                in_deps = false;
            }
        }

        if let Some(name) = current_pkg {
            results.insert(name, current_deps);
        }

        Ok(results)
    }

    fn get_dependencies_batch_fallback(
        &self,
        packages: &[&str],
    ) -> Result<HashMap<String, Vec<String>>> {
        use indicatif::{ProgressBar, ProgressStyle};

        let pb = ProgressBar::new(packages.len() as u64);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.yellow} [{elapsed_precise}] [{bar:40.yellow/blue}] {pos}/{len} packages (fallback)")
            .unwrap()
            .progress_chars("#>-"));

        pb.set_message("Fetching dependencies (individual fallback)");

        let mut results = HashMap::new();
        for pkg in packages {
            pb.inc(1);
            if let Ok(deps) = self.get_dependencies_from_system_safe(pkg) {
                results.insert(pkg.to_string(), deps);
            }
        }

        pb.finish_with_message("Fallback complete");
        Ok(results)
    }

    fn get_dependencies_from_system_safe(&self, package: &str) -> Result<Vec<String>> {
        match self.get_dependencies_from_system(package) {
            Ok(deps) => Ok(deps),
            Err(e) => {
                let _ = self
                    .logger
                    .warning(&format!("Failed to get deps for {}: {}", package, e));
                Ok(Vec::new())
            }
        }
    }

    fn get_dependencies_from_system(&self, package: &str) -> Result<Vec<String>> {
        let output = Command::new("pkg").arg("show").arg(package).output();

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
                        let clean = dep
                            .trim()
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_string();
                        if !clean.is_empty() {
                            deps.push(clean);
                        }
                    }
                }
            } else if in_deps && line.starts_with(' ') {
                for dep in line.trim().split(',') {
                    let clean = dep
                        .trim()
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_string();
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

    fn load_file_map_cache(&self) -> Result<()> {
        let db_mtime = utils::get_dpkg_db_mtime();

        if let Ok(Some(cache)) = self.cache_manager.load_file_map_cache() {
            if let Some(mtime) = db_mtime {
                if let Ok(mtime_secs) = mtime.elapsed() {
                    let cache_age = Utc::now().timestamp() - cache.built_at;
                    if cache_age < mtime_secs.as_secs() as i64 {
                        let mut file_map = self.file_map_cache.write();
                        *file_map = cache;
                        return Ok(());
                    }
                }
            }

            let age = Utc::now().timestamp() - cache.built_at;
            if age < 86400 {
                let mut file_map = self.file_map_cache.write();
                *file_map = cache;
                return Ok(());
            }
        }

        self.build_file_map_cache()
    }

    pub fn build_file_map_cache(&self) -> Result<()> {
        self.logger.info("Building file to package mapping...")?;

        let mapping = utils::build_file_package_map()?;
        let total_entries = mapping.len();

        let cache = FileMapCache {
            mapping,
            built_at: Utc::now().timestamp(),
            total_entries,
        };

        self.cache_manager.save_file_map_cache(&cache)?;

        let mut file_map = self.file_map_cache.write();
        *file_map = cache;

        self.logger.info(&format!(
            "File to package map built: {} entries",
            total_entries
        ))?;
        Ok(())
    }

    fn resolve_filename_to_package(&self, filename: &str) -> Option<String> {
        let file_map = self.file_map_cache.read();
        file_map.get_package_for_file(filename)
    }

    pub fn scan_all_packages(&self, force: bool) -> Result<Vec<Package>> {
        if !force && self.cache_manager.is_fresh()? {
            return Ok(self.cache.read().values().cloned().collect());
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

        self.rebuild_caches()?;
        self.build_file_map_cache()?;

        self.logger.info(&format!(
            "Scan complete: {} packages found",
            packages_vec.len()
        ))?;
        Ok(packages_vec)
    }

    pub fn get_installed_packages_all(&self) -> Result<Vec<Package>> {
        let cache = self.cache.read();
        if !cache.is_empty() {
            return Ok(cache.values().cloned().collect());
        }
        drop(cache);

        self.scan_all_packages(false)
    }

    pub fn get_dependencies(&self, package: &str) -> Result<Vec<String>> {
        let pkg_name = if let Some(resolved) = self.resolve_filename_to_package(package) {
            resolved
        } else {
            package.to_string()
        };

        let dep_cache = self.dependency_cache.read();
        if let Some(deps) = dep_cache.get_deps(&pkg_name) {
            return Ok(deps.clone());
        }
        drop(dep_cache);

        let deps = self.get_dependencies_from_system_safe(&pkg_name)?;

        let mut dep_cache = self.dependency_cache.write();
        dep_cache.direct_deps.insert(pkg_name, deps.clone());
        drop(dep_cache);

        Ok(deps)
    }

    pub fn get_reverse_dependencies(&self, package: &str) -> Result<Vec<String>> {
        let dep_cache = self.dependency_cache.read();
        if let Some(reverse) = dep_cache.get_reverse_deps(package) {
            return Ok(reverse.clone());
        }
        drop(dep_cache);

        let all_packages = self.get_installed_packages_all()?;
        let mut reverse_deps = Vec::new();

        for pkg in all_packages {
            if let Ok(deps) = self.get_dependencies(&pkg.name) {
                if deps.contains(&package.to_string()) {
                    reverse_deps.push(pkg.name);
                }
            }
        }

        let mut dep_cache = self.dependency_cache.write();
        dep_cache
            .reverse_deps
            .insert(package.to_string(), reverse_deps.clone());
        drop(dep_cache);

        Ok(reverse_deps)
    }

    pub fn get_all_dependencies(&self, packages: &[String]) -> Result<HashSet<String>> {
        let mut all_deps = HashSet::new();
        let mut to_process: Vec<String> = packages.to_vec();
        let mut processed = HashSet::new();
        let core_packages = Self::get_core_packages();
        let _depth_limit = self.config.dependency_depth;

        while let Some(pkg) = to_process.pop() {
            if processed.contains(&pkg) {
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
                        to_process.push(dep);
                    }
                }
            }
        }

        Ok(all_deps)
    }

    pub fn get_used_packages(&self) -> Result<HashSet<String>> {
        let usage_cache = self.usage_cache.read();
        if !usage_cache.is_empty() {
            return Ok(usage_cache.get_used_packages());
        }
        drop(usage_cache);

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

    fn get_install_date_from_cache(&self, package: &str) -> Option<String> {
        let usage_cache = self.usage_cache.read();
        if let Some(timestamp) = usage_cache.get_install_time(package) {
            let dt = DateTime::<Local>::from(Utc.timestamp(timestamp, 0));
            Some(dt.format("%Y-%m-%d %H:%M:%S").to_string())
        } else {
            None
        }
    }

    fn get_last_used_date_from_cache(&self, package: &str) -> Option<String> {
        let usage_cache = self.usage_cache.read();
        if let Some(timestamp) = usage_cache.get_last_use(package) {
            let dt = DateTime::<Local>::from(Utc.timestamp(timestamp, 0));
            Some(dt.format("%Y-%m-%d %H:%M:%S").to_string())
        } else {
            None
        }
    }

    fn get_usage_count_from_cache(&self, package: &str) -> Option<u64> {
        let usage_cache = self.usage_cache.read();
        let count = usage_cache.get_usage_count(package);
        if count > 0 {
            Some(count as u64)
        } else {
            None
        }
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

        let installed_date = self.get_install_date_from_cache(package);
        let last_used_date = self.get_last_used_date_from_cache(package);
        let usage_count = self.get_usage_count_from_cache(package);

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

    pub fn find_unused(&self, days_threshold: u32) -> Result<Vec<UnusedPackage>> {
        let installed = self.get_installed_packages_all()?;
        let core_packages = Self::get_core_packages();
        let current_time = Utc::now().timestamp();
        let threshold_seconds = (days_threshold as i64) * 86400;

        let usage_cache = self.usage_cache.read();
        let used_packages = usage_cache.get_used_packages();
        let mut unused = Vec::new();

        for pkg in installed {
            if core_packages.contains(&pkg.name.as_str()) && self.config.protect_core {
                continue;
            }

            if used_packages.contains(&pkg.name) {
                continue;
            }

            let last_used = usage_cache.get_last_use(&pkg.name);
            let days_unused = if let Some(last) = last_used {
                let age = current_time - last;
                if age > threshold_seconds {
                    (age / 86400) as u32
                } else {
                    continue;
                }
            } else {
                if days_threshold == 0 {
                    0
                } else {
                    continue;
                }
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
        let unused = self.find_unused(days_threshold)?;
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
            return Ok(format!(
                "Package '{}' is directly used (you installed it)",
                package
            ));
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
                        return Ok(format!(
                            "Package '{}' is a dependency of '{}'",
                            package, used_pkg
                        ));
                    }
                }
            }
            return Ok(format!(
                "Package '{}' is a dependency of used packages",
                package
            ));
        }

        Ok(format!("Package '{}' is not protected", package))
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

                    self.rebuild_caches()?;
                    self.build_file_map_cache()?;

                    self.logger
                        .info(&format!("Removed package: {}", unused_pkg.name))?;
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

                self.rebuild_caches()?;
                self.build_file_map_cache()?;

                self.logger
                    .info(&format!("Removed package: {}", unused_pkg.name))?;
                Ok(())
            }
        }
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

                    self.rebuild_caches()?;
                    self.build_file_map_cache()?;

                    self.logger
                        .info(&format!("Installed package: {}", package.name))?;
                    Ok(())
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    Err(anyhow!("Failed to install package: {}", stderr))
                }
            }
            _ => Err(anyhow!(
                "Cannot automatically install {} packages",
                package.source
            )),
        }
    }

    pub fn remove_package_from_cache(&self, package: &str) -> Result<()> {
        let mut cache = self.cache.write();
        cache.remove(package);
        drop(cache);
        self.save_cache()?;
        self.rebuild_caches()?;
        self.build_file_map_cache()?;
        self.logger
            .info(&format!("Removed {} from cache", package))?;
        Ok(())
    }

    pub fn print_dependency_tree(&self, package: &str, max_depth: usize) -> Result<()> {
        let deps = self.get_dependencies(package)?;
        println!(
            "Dependency tree for '{}' (max depth: {}):",
            package, max_depth
        );
        self.print_tree(package, &deps, 0, max_depth, &mut HashSet::new())?;
        Ok(())
    }

    fn print_tree(
        &self,
        _package: &str,
        deps: &[String],
        indent: usize,
        max_depth: usize,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
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
                        self.print_tree_with_vertical(
                            dep,
                            &sub_deps,
                            indent + 1,
                            max_depth,
                            visited,
                            is_last,
                        )?;
                    }
                }
                visited.remove(dep);
            }
        }

        Ok(())
    }

    fn print_tree_with_vertical(
        &self,
        _package: &str,
        deps: &[String],
        indent: usize,
        max_depth: usize,
        visited: &mut HashSet<String>,
        parent_is_last: bool,
    ) -> Result<()> {
        if indent >= max_depth {
            println!("{}... (depth limit reached)", "  ".repeat(indent));
            return Ok(());
        }

        let prefix = if parent_is_last { "    " } else { "│   " };
        let full_prefix = "  ".repeat(indent) + prefix;

        for (i, dep) in deps.iter().enumerate() {
            let is_last = i == deps.len() - 1;
            let connector = if is_last { "└── " } else { "├── " };
            let status = if visited.contains(dep) {
                " [cycle]"
            } else {
                ""
            };

            println!("{}{}{}{}", full_prefix, connector, dep, status);

            if !visited.contains(dep) {
                visited.insert(dep.clone());
                if let Ok(sub_deps) = self.get_dependencies(dep) {
                    if !sub_deps.is_empty() {
                        self.print_tree_with_vertical(
                            dep,
                            &sub_deps,
                            indent + 1,
                            max_depth,
                            visited,
                            is_last,
                        )?;
                    }
                }
                visited.remove(dep);
            }
        }
        Ok(())
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
        oldest.sort_by(|a, b| {
            a.installed_date
                .unwrap_or(0)
                .cmp(&b.installed_date.unwrap_or(0))
        });
        let oldest_packages = oldest.into_iter().take(10).collect();

        let mut newest = packages.clone();
        newest.sort_by(|a, b| {
            b.installed_date
                .unwrap_or(0)
                .cmp(&a.installed_date.unwrap_or(0))
        });
        let newest_packages = newest.into_iter().take(10).collect();

        let avg_size = if !packages.is_empty() {
            total_size / packages.len() as u64
        } else {
            0
        };

        let usage_cache = self.usage_cache.read();
        let total_install_events = usage_cache
            .events
            .iter()
            .filter(|e| e.action == "INSTALL")
            .count();
        let total_remove_events = usage_cache
            .events
            .iter()
            .filter(|e| e.action == "REMOVE")
            .count();

        Ok(Stats {
            total_packages: packages.len(),
            used_packages: used.len(),
            total_size,
            by_source: by_source_vec,
            largest_packages,
            oldest_packages,
            newest_packages,
            average_package_size: avg_size,
            total_log_entries: usage_cache.event_count,
            total_install_events,
            total_remove_events,
        })
    }

    pub fn log_event(&self, event: &PackageEvent) -> Result<()> {
        self.logger.log_event(event)
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
                    self.logger
                        .warning(&format!("Backup failed for {}: {}", pkg.name, stderr))?;
                } else {
                    self.logger.info(&format!(
                        "Backed up {} to {}",
                        pkg.name,
                        backup_path.display()
                    ))?;
                }
            }
        }

        Ok(())
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

    pub fn get_tracker(&self) -> Tracker {
        Tracker {
            config: self.config.clone(),
            cache: self.cache.clone(),
            cache_manager: self.cache_manager.clone(),
            logger: self.logger.clone(),
            dependency_cache: self.dependency_cache.clone(),
            usage_cache: self.usage_cache.clone(),
            file_map_cache: self.file_map_cache.clone(),
            last_rebuild: self.last_rebuild.clone(),
        }
    }

    pub fn get_cache_manager(&self) -> CacheManager {
        self.cache_manager.clone()
    }

    pub fn get_file_map_stats(&self) -> (usize, i64) {
        let file_map = self.file_map_cache.read();
        (file_map.total_entries, file_map.built_at)
    }
}
