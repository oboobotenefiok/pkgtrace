use anyhow::{anyhow, Result};
use rayon::prelude::*;
use serde_json;
use std::{
    collections::HashSet, os::unix::fs::PermissionsExt, path::PathBuf, process::Command, sync::Arc,
    time::SystemTime,
};
use walkdir::WalkDir;
use which::which;

use crate::{
    config::Config,
    models::{Package, PackageSource},
    utils,
};

pub struct PackageScanner;

impl PackageScanner {
    pub fn scan_all(config: Arc<Config>) -> Result<HashSet<Package>> {
        use indicatif::{ProgressBar, ProgressStyle};

        let mut all_packages = HashSet::new();
        let scan_dirs = config.get_scan_dirs();
        let exclude_patterns = config.get_exclude_patterns();

        let pb = ProgressBar::new(6);
        pb.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} sources",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        pb.set_message("Scanning package sources");

        let sources = vec![
            (
                "pkg",
                Self::scan_pkg_if_needed(
                    Self::get_pkg_db_modified(),
                    Self::is_cache_fresh(&config),
                ),
            ),
            ("cargo", Self::scan_cargo()),
            ("pip", Self::scan_pip()),
            ("npm", Self::scan_npm()),
            ("gem", Self::scan_gem()),
            ("manual", Self::scan_manual(scan_dirs, exclude_patterns)),
        ];

        let results: Vec<_> = sources
            .into_par_iter()
            .map(|(name, result)| {
                pb.inc(1);
                pb.set_message(format!("Scanning {}", name));
                (name, result)
            })
            .collect();

        for (name, result) in results {
            if let Ok(packages) = result {
                let count = packages.len();
                all_packages.extend(packages);
                pb.println(format!("✓ {}: {} packages", name, count));
            }
        }

        pb.finish_with_message("Scan complete");

        let pb2 = ProgressBar::new(2);
        pb2.set_style(
            ProgressStyle::default_bar()
                .template(
                    "{spinner:.green} [{elapsed_precise}] {bar:40.cyan/blue} {pos}/{len} steps",
                )
                .unwrap()
                .progress_chars("#>-"),
        );

        pb2.set_message("Merging duplicates");
        Self::merge_duplicates(&mut all_packages);
        pb2.inc(1);

        pb2.set_message("Enriching package info");
        Self::enrich_package_info(&mut all_packages)?;
        pb2.inc(1);

        pb2.finish_with_message("Ready");

        Ok(all_packages)
    }

    fn get_pkg_db_modified() -> Option<SystemTime> {
        let db_paths = [
            "/data/data/com.termux/files/usr/lib/apt/lists/",
            "/data/data/com.termux/files/usr/var/lib/dpkg/status",
            "/data/data/com.termux/files/usr/var/lib/dpkg/available",
        ];

        let mut latest = None;
        for path in db_paths {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    if latest.is_none() || modified > latest.unwrap() {
                        latest = Some(modified);
                    }
                }
            }
        }
        latest
    }

    fn is_cache_fresh(config: &Config) -> bool {
        let cache_file = config.db_file.join("packages.db.json");
        if let Ok(metadata) = std::fs::metadata(&cache_file) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = modified.elapsed() {
                    return elapsed.as_secs() < 3600;
                }
            }
        }
        false
    }

    fn scan_pkg_if_needed(
        db_modified: Option<SystemTime>,
        cache_fresh: bool,
    ) -> Result<HashSet<Package>> {
        if cache_fresh {
            if let Some(db_time) = db_modified {
                if let Ok(cache_meta) = std::fs::metadata("packages.db.json") {
                    if let Ok(cache_time) = cache_meta.modified() {
                        if cache_time > db_time {
                            return Ok(HashSet::new());
                        }
                    }
                }
            }
        }

        Self::scan_pkg()
    }

    fn scan_pkg() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        let output = Command::new("pkg").arg("list-installed").output();

        if let Err(e) = output {
            return Err(anyhow!("Failed to run pkg list-installed: {}", e));
        }

        let output = output.unwrap();
        if !output.status.success() {
            return Err(anyhow!("pkg list-installed failed"));
        }

        let stdout = String::from_utf8(output.stdout)?;
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].to_string();
                let version = parts[1].to_string();

                let install_path =
                    PathBuf::from(format!("/data/data/com.termux/files/usr/bin/{}", name));

                packages.insert(Package {
                    name,
                    version: Some(version),
                    source: PackageSource::Pkg,
                    install_path,
                    size: None,
                    dependencies: None,
                    installed_date: None,
                    last_used: None,
                    usage_count: None,
                    checksum: None,
                });
            }
        }

        let dpkg_output = Command::new("dpkg").arg("-l").output()?;

        if dpkg_output.status.success() {
            let stdout = String::from_utf8(dpkg_output.stdout)?;
            for line in stdout.lines() {
                if line.starts_with("ii") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let name = parts[1].to_string();

                        let exists = packages.iter().any(|p| p.name == name);
                        if !exists {
                            let version = parts.get(2).map(|v| v.to_string());
                            let install_path = PathBuf::from(format!(
                                "/data/data/com.termux/files/usr/bin/{}",
                                name
                            ));

                            packages.insert(Package {
                                name,
                                version,
                                source: PackageSource::Pkg,
                                install_path,
                                size: None,
                                dependencies: None,
                                installed_date: None,
                                last_used: None,
                                usage_count: None,
                                checksum: None,
                            });
                        }
                    }
                }
            }
        }

        Ok(packages)
    }

    fn scan_cargo() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        if which("cargo").is_err() {
            return Ok(packages);
        }

        let home =
            home::home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"));
        let cargo_bin = home.join(".cargo/bin");

        if cargo_bin.exists() {
            for entry in WalkDir::new(&cargo_bin)
                .max_depth(1)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(metadata) = path.metadata() {
                        if metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0) {
                            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                                let size = Some(metadata.len());
                                packages.insert(Package {
                                    name: name.to_string(),
                                    version: None,
                                    source: PackageSource::Cargo,
                                    install_path: path.to_path_buf(),
                                    size,
                                    dependencies: None,
                                    installed_date: None,
                                    last_used: None,
                                    usage_count: None,
                                    checksum: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        let packages_vec: Vec<Package> = packages.iter().cloned().collect();
        for pkg in packages_vec {
            if let Ok(output) = Command::new(&pkg.install_path).arg("--version").output() {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(cap) = regex::Regex::new(r"v?(\d+\.\d+\.\d+)")?.captures(&stdout) {
                        if let Some(ver) = cap.get(1) {
                            let mut new_pkg = pkg.clone();
                            new_pkg.version = Some(ver.as_str().to_string());
                            packages.remove(&pkg);
                            packages.insert(new_pkg);
                        }
                    }
                }
            }
        }

        Ok(packages)
    }

    fn scan_pip() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        let pip_cmd = if which("pip3").is_ok() { "pip3" } else { "pip" };

        if which(pip_cmd).is_err() {
            return Ok(packages);
        }

        let output = Command::new(pip_cmd)
            .arg("list")
            .arg("--format=json")
            .output()?;

        if !output.status.success() {
            return Ok(packages);
        }

        let stdout = String::from_utf8(output.stdout)?;
        if let Ok(json) = serde_json::from_str::<Vec<PipPackage>>(&stdout) {
            let site_packages =
                PathBuf::from("/data/data/com.termux/files/usr/lib/python3.11/site-packages");

            for pkg in json {
                let install_path = Self::get_pip_location(pip_cmd, &pkg.name, &site_packages);
                let size = utils::get_path_size(&install_path);

                packages.insert(Package {
                    name: pkg.name,
                    version: Some(pkg.version),
                    source: PackageSource::Pip,
                    install_path,
                    size,
                    dependencies: None,
                    installed_date: None,
                    last_used: None,
                    usage_count: None,
                    checksum: None,
                });
            }
        }

        Ok(packages)
    }

    fn get_pip_location(pip_cmd: &str, package: &str, default_path: &PathBuf) -> PathBuf {
        if let Ok(output) = Command::new(pip_cmd).arg("show").arg(package).output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.starts_with("Location:") {
                    let loc = line.trim_start_matches("Location:").trim();
                    let path = PathBuf::from(loc);
                    if path.exists() {
                        return path.join(package);
                    }
                }
            }
        }
        default_path.join(package)
    }

    fn scan_npm() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        if which("npm").is_err() {
            return Ok(packages);
        }

        let output = Command::new("npm")
            .arg("list")
            .arg("--global")
            .arg("--depth=0")
            .arg("--json")
            .output()?;

        if !output.status.success() {
            return Ok(packages);
        }

        let stdout = String::from_utf8(output.stdout)?;
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&stdout) {
            if let Some(deps) = json.get("dependencies").and_then(|d| d.as_object()) {
                let node_modules =
                    PathBuf::from("/data/data/com.termux/files/usr/lib/node_modules");

                for (name, info) in deps {
                    let version = info
                        .get("version")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string());
                    let install_path = node_modules.join(name);
                    let size = utils::get_path_size(&install_path);

                    packages.insert(Package {
                        name: name.clone(),
                        version,
                        source: PackageSource::Npm,
                        install_path,
                        size,
                        dependencies: None,
                        installed_date: None,
                        last_used: None,
                        usage_count: None,
                        checksum: None,
                    });
                }
            }
        }

        Ok(packages)
    }

    fn scan_gem() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        if which("gem").is_err() {
            return Ok(packages);
        }

        let output = Command::new("gem")
            .arg("list")
            .arg("--local")
            .arg("--format=json")
            .output()?;

        if !output.status.success() {
            return Ok(packages);
        }

        let stdout = String::from_utf8(output.stdout)?;
        if let Ok(json) = serde_json::from_str::<Vec<GemPackage>>(&stdout) {
            let gem_home = std::env::var("GEM_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("/data/data/com.termux/files/usr/lib/ruby/gems"));

            for pkg in json {
                let install_path = gem_home.join(format!("gems/{}-{}", pkg.name, pkg.version));
                let size = utils::get_path_size(&install_path);

                packages.insert(Package {
                    name: pkg.name,
                    version: Some(pkg.version),
                    source: PackageSource::Gem,
                    install_path,
                    size,
                    dependencies: None,
                    installed_date: None,
                    last_used: None,
                    usage_count: None,
                    checksum: None,
                });
            }
        }

        Ok(packages)
    }

    fn scan_manual(
        scan_dirs: Vec<PathBuf>,
        exclude_patterns: Vec<String>,
    ) -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        let source_dirs = vec![
            home::home_dir()
                .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"))
                .join("src"),
            home::home_dir()
                .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"))
                .join("build"),
            home::home_dir()
                .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"))
                .join("projects"),
        ];

        for path in scan_dirs {
            if path.exists() {
                for entry in WalkDir::new(&path)
                    .max_depth(2)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let entry_path = entry.path();
                    if entry_path.is_file() {
                        if let Ok(metadata) = entry_path.metadata() {
                            if metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0) {
                                if let Some(name) = entry_path.file_name().and_then(|n| n.to_str())
                                {
                                    let is_excluded = exclude_patterns.iter().any(|pattern| {
                                        if let Ok(re) = regex::Regex::new(pattern) {
                                            re.is_match(name)
                                        } else {
                                            false
                                        }
                                    });

                                    if !is_excluded {
                                        let is_tracked =
                                            packages.iter().any(|p: &Package| p.name == name);
                                        if !is_tracked {
                                            let size = Some(metadata.len());
                                            packages.insert(Package {
                                                name: name.to_string(),
                                                version: None,
                                                source: PackageSource::Manual,
                                                install_path: entry_path.to_path_buf(),
                                                size,
                                                dependencies: None,
                                                installed_date: None,
                                                last_used: None,
                                                usage_count: None,
                                                checksum: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        for dir in source_dirs {
            if dir.exists() {
                for entry in WalkDir::new(&dir)
                    .max_depth(2)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    let entry_path = entry.path();
                    if let Some(filename) = entry_path.file_name().and_then(|n| n.to_str()) {
                        if filename == "Makefile"
                            || filename == "configure"
                            || filename.ends_with(".sh")
                        {
                            if let Some(parent) = entry_path.parent() {
                                if let Some(dir_name) = parent.file_name().and_then(|n| n.to_str())
                                {
                                    let bin_path = parent.join(dir_name);
                                    if bin_path.exists() {
                                        if let Ok(metadata) = bin_path.metadata() {
                                            if metadata.is_file()
                                                && (metadata.permissions().mode() & 0o111 != 0)
                                            {
                                                let is_tracked = packages
                                                    .iter()
                                                    .any(|p: &Package| p.name == dir_name);
                                                if !is_tracked {
                                                    let size = utils::get_path_size(parent);
                                                    packages.insert(Package {
                                                        name: dir_name.to_string(),
                                                        version: None,
                                                        source: PackageSource::Manual,
                                                        install_path: bin_path,
                                                        size,
                                                        dependencies: None,
                                                        installed_date: None,
                                                        last_used: None,
                                                        usage_count: None,
                                                        checksum: None,
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(packages)
    }

    fn merge_duplicates(packages: &mut HashSet<Package>) {
        let mut to_remove = Vec::new();
        let mut to_keep = Vec::new();

        let mut names: std::collections::HashMap<String, Vec<Package>> =
            std::collections::HashMap::new();
        for pkg in packages.iter() {
            names.entry(pkg.name.clone()).or_default().push(pkg.clone());
        }

        for (_name, pkgs) in names {
            if pkgs.len() > 1 {
                if let Some(pkg_version) = pkgs.iter().find(|p| p.source == PackageSource::Pkg) {
                    for pkg in &pkgs {
                        if pkg.source != PackageSource::Pkg {
                            to_remove.push(pkg.name.clone());
                        }
                    }
                    to_keep.push(pkg_version.clone());
                } else {
                    let mut sorted = pkgs.clone();
                    sorted.sort_by(|a, b| {
                        b.installed_date
                            .unwrap_or(0)
                            .cmp(&a.installed_date.unwrap_or(0))
                    });
                    to_keep.push(sorted[0].clone());
                    for pkg in &sorted[1..] {
                        to_remove.push(pkg.name.clone());
                    }
                }
            } else if let Some(pkg) = pkgs.first() {
                to_keep.push(pkg.clone());
            }
        }

        for name in &to_remove {
            packages.retain(|p| p.name != *name);
        }

        for pkg in to_keep {
            packages.insert(pkg);
        }
    }

    fn enrich_package_info(packages: &mut HashSet<Package>) -> Result<()> {
        let packages_vec: Vec<Package> = packages.iter().cloned().collect();
        for pkg in packages_vec {
            let mut updated = false;
            let mut new_pkg = pkg.clone();

            if new_pkg.size.is_none() {
                new_pkg.size = utils::get_path_size(&new_pkg.install_path);
                updated = true;
            }

            if new_pkg.installed_date.is_none() {
                if let Ok(metadata) = std::fs::metadata(&new_pkg.install_path) {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(duration) = modified.elapsed() {
                            let timestamp =
                                chrono::Utc::now().timestamp() - duration.as_secs() as i64;
                            new_pkg.installed_date = Some(timestamp);
                            updated = true;
                        }
                    }
                }
            }

            if new_pkg.checksum.is_none() && new_pkg.install_path.is_file() {
                if let Ok(checksum) = utils::compute_file_checksum(&new_pkg.install_path) {
                    new_pkg.checksum = Some(checksum);
                    updated = true;
                }
            }

            if updated {
                packages.remove(&pkg);
                packages.insert(new_pkg);
            }
        }

        Ok(())
    }
}

#[derive(serde::Deserialize)]
struct PipPackage {
    name: String,
    version: String,
}

#[derive(serde::Deserialize)]
struct GemPackage {
    name: String,
    version: String,
}

