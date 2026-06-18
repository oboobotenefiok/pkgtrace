use anyhow::{Result, anyhow};
use rayon::prelude::*;
use serde_json;
use std::{
    collections::HashSet,
    path::PathBuf,
    process::Command,
    sync::Arc,
    os::unix::fs::PermissionsExt,
};
use walkdir::WalkDir;
use which::which;
use crate::{
    models::*,
    config::Config,
    utils,
};

pub struct PackageScanner; // This is a unit struct. Remember? There's nothing inside. We only use it to confirm something happened - worst explanation

impl PackageScanner {
// The config clone arrives here from the tracker.
// This is where the real thing happens.
    pub fn scan_all(config: Arc<Config>) -> Result<HashSet<Package>> {
    // We create an empty in-memory DataBase with a(n) hashmap. Mutable because we will constantly update it in the loop below(a for loop of course!).
        let mut all_packages = HashSet::new();
//Remember that config is an Arc<Config> type and config is from the Config fig in the config file - worst explanation! So, we apply a Config impl method: get_scan_dirs
        let scan_dirs = config.get_scan_dirs();
        // We then fetch the excluded patterns so we don't hurt the developers by uninstalling their termux root, lol.
        let exclude_patterns = config.get_exclude_patterns();

        let results: Vec<Result<HashSet<Package>>> = vec![
            Self::scan_pkg(),
            Self::scan_cargo(),
            Self::scan_pip(),
            Self::scan_npm(),
            Self::scan_gem(),
            Self::scan_manual(scan_dirs, exclude_patterns),
        ]
        .into_par_iter()
        .collect();
// We iterate over each result and return a result Ok containing the package itself to the hashmap. Remembering that the return type of this function is Result<HashSet<Package>> will boost your understanding right here right now.
        for result in results {
       // This assigns each result to a new variable packages, and pushes it to the all_packages hasmap at the start of this function.
            if let Ok(packages) = result {
                all_packages.extend(packages);
            }
        }

        Self::merge_duplicates(&mut all_packages);
        Self::enrich_package_info(&mut all_packages)?;

        Ok(all_packages)
    }

    fn merge_duplicates(packages: &mut HashSet<Package>) {
    // We create new HashSets to store instances of what to keep and what not to.
        let mut to_remove = Vec::new();
        let mut to_keep = Vec::new();

// We create an empty key value pair for the set
        let mut names: std::collections::HashMap<String, Vec<Package>> = std::collections::HashMap::new();
        for pkg in packages.iter() {
            names.entry(pkg.name.clone()).or_default().push(pkg.clone());
        }

        for (name, pkgs) in names {
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
                        b.installed_date.unwrap_or(0).cmp(&a.installed_date.unwrap_or(0))
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
        for mut pkg in packages_vec {
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
                            let timestamp = chrono::Utc::now().timestamp() - duration.as_secs() as i64;
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

    fn scan_pkg() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        let output = Command::new("pkg")
            .arg("list-installed")
            .output();

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

                let install_path = PathBuf::from(format!(
                    "/data/data/com.termux/files/usr/{}",
                    name
                ));

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

        Ok(packages)
    }

    fn scan_cargo() -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        if which("cargo").is_err() {
            return Ok(packages);
        }

        let home = home::home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home"));
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
        for mut pkg in packages_vec {
            if let Ok(output) = Command::new(&pkg.install_path)
                .arg("--version")
                .output()
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if let Some(cap) = regex::Regex::new(r"v?(\d+\.\d+\.\d+)")?.captures(&stdout) {
                        if let Some(ver) = cap.get(1) {
                            pkg.version = Some(ver.as_str().to_string());
                            packages.remove(&pkg);
                            packages.insert(pkg);
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
            let site_packages = PathBuf::from("/data/data/com.termux/files/usr/lib/python3.11/site-packages");

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
        if let Ok(output) = Command::new(pip_cmd)
            .arg("show")
            .arg(package)
            .output()
        {
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
                let node_modules = PathBuf::from("/data/data/com.termux/files/usr/lib/node_modules");

                for (name, info) in deps {
                    let version = info.get("version").and_then(|v| v.as_str()).map(|v| v.to_string());
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

    fn scan_manual(scan_dirs: Vec<PathBuf>, exclude_patterns: Vec<String>) -> Result<HashSet<Package>> {
        let mut packages = HashSet::new();

        let source_dirs = vec![
            home::home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home")).join("src"),
            home::home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home")).join("build"),
            home::home_dir().unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home")).join("projects"),
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
                                if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                                    let is_excluded = exclude_patterns.iter().any(|pattern| {
                                        if let Ok(re) = regex::Regex::new(pattern) {
                                            re.is_match(name)
                                        } else {
                                            false
                                        }
                                    });

                                    if !is_excluded {
                                  let is_tracked = packages.iter().any(|p: &Package| p.name == name);
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
                        if filename == "Makefile" || filename == "configure" || filename.ends_with(".sh") {
                            if let Some(parent) = entry_path.parent() {
                                if let Some(dir_name) = parent.file_name().and_then(|n| n.to_str()) {
                                    let bin_path = parent.join(dir_name);
                                    if bin_path.exists() {
                                        if let Ok(metadata) = bin_path.metadata() {
                                            if metadata.is_file() && (metadata.permissions().mode() & 0o111 != 0) {
                                                let is_tracked = packages.iter().any(|p: &Package| p.name == dir_name);
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