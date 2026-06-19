use anyhow::{anyhow, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};
use walkdir::WalkDir;

use crate::models::{DependencyGraph, Package};

pub fn format_size(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    format!("{:.1} {}", size, UNITS[unit_index])
}

pub fn packages_to_csv(packages: &[Package]) -> Result<String> {
    let mut csv = String::new();
    csv.push_str("Name,Version,Source,Location,Size,InstallDate,LastUsed\n");

    for pkg in packages {
        let size = pkg
            .size
            .map(format_size)
            .unwrap_or_else(|| "unknown".to_string());
        let install_date = pkg
            .installed_date
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let last_used = pkg
            .last_used
            .map(|ts| {
                chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            })
            .unwrap_or_else(|| "never".to_string());

        csv.push_str(&format!(
            "{},{},{},{},{},{},{}\n",
            pkg.name,
            pkg.version.clone().unwrap_or_else(|| "unknown".to_string()),
            pkg.source,
            pkg.install_path.display(),
            size,
            install_date,
            last_used
        ));
    }

    Ok(csv)
}

pub fn packages_to_markdown(packages: &[Package]) -> Result<String> {
    let mut md = String::new();
    md.push_str("# Installed Packages\n\n");
    md.push_str("| Name | Version | Source | Location | Size |\n");
    md.push_str("|------|---------|--------|----------|------|\n");

    for pkg in packages {
        let size = pkg
            .size
            .map(format_size)
            .unwrap_or_else(|| "unknown".to_string());
        md.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            pkg.name,
            pkg.version.clone().unwrap_or_else(|| "unknown".to_string()),
            pkg.source,
            pkg.install_path.display(),
            size
        ));
    }

    Ok(md)
}

pub fn graph_to_dot(graph: &DependencyGraph) -> Result<String> {
    let mut dot = String::new();
    dot.push_str("digraph Dependencies {\n");
    dot.push_str("  rankdir=LR;\n");
    dot.push_str("  node [shape=box];\n\n");

    for node in &graph.dependencies {
        dot.push_str(&format!("  \"{}\" -> \"{}\";\n", node.parent, node.name));
    }

    if !graph.cycles.is_empty() {
        dot.push_str("\n  // Cycles detected\n");
        for (i, cycle) in graph.cycles.iter().enumerate() {
            dot.push_str(&format!("  subgraph cluster_cycle_{} {{\n", i));
            dot.push_str("    color=red;\n");
            dot.push_str("    label=\"Cycle\";\n");
            for node in cycle {
                dot.push_str(&format!("    \"{}\";\n", node));
            }
            dot.push_str("  }\n");
        }
    }

    dot.push_str("}\n");

    Ok(dot)
}

pub fn get_file_extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_string())
}

pub fn is_executable(path: &Path) -> bool {
    if let Ok(metadata) = path.metadata() {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    } else {
        false
    }
}

pub fn get_path_size(path: &Path) -> Option<u64> {
    if !path.exists() {
        return None;
    }

    let mut total_size = 0;
    if path.is_file() {
        if let Ok(metadata) = path.metadata() {
            total_size = metadata.len();
        }
    } else if path.is_dir() {
        for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
            if let Ok(metadata) = entry.metadata() {
                if metadata.is_file() {
                    total_size += metadata.len();
                }
            }
        }
    }

    Some(total_size)
}

pub fn safe_remove(path: &Path) -> Result<()> {
    if path.exists() {
        if path.is_dir() {
            std::fs::remove_dir_all(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

pub fn compute_file_checksum(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 8192];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let result = hasher.finalize();
    Ok(format!("{:x}", result))
}

pub fn parse_version(version_str: &str) -> Option<(u32, u32, u32)> {
    let parts: Vec<&str> = version_str.split('.').collect();
    if parts.len() >= 3 {
        if let (Ok(major), Ok(minor), Ok(patch)) = (
            parts[0].parse::<u32>(),
            parts[1].parse::<u32>(),
            parts[2].parse::<u32>(),
        ) {
            return Some((major, minor, patch));
        }
    }
    None
}

pub fn compare_versions(v1: &str, v2: &str) -> Option<std::cmp::Ordering> {
    if let (Some((m1, n1, p1)), Some((m2, n2, p2))) = (parse_version(v1), parse_version(v2)) {
        match m1.cmp(&m2) {
            std::cmp::Ordering::Equal => match n1.cmp(&n2) {
                std::cmp::Ordering::Equal => Some(p1.cmp(&p2)),
                other => Some(other),
            },
            other => Some(other),
        }
    } else {
        None
    }
}

pub fn is_path_safe(path: &Path) -> bool {
    let path_str = path.to_str().unwrap_or("");
    !path_str.contains("..") && !path_str.contains("//")
}

pub fn get_temp_dir() -> Result<PathBuf> {
    let temp_dir = std::env::temp_dir().join("pkgtrace");
    ensure_dir(&temp_dir)?;
    Ok(temp_dir)
}

pub fn rotate_log_file(log_path: &Path, max_size: u64) -> Result<()> {
    if !log_path.exists() {
        return Ok(());
    }

    let metadata = std::fs::metadata(log_path)?;
    if metadata.len() > max_size {
        let backup_path = log_path.with_extension("log.old");
        if backup_path.exists() {
            std::fs::remove_file(&backup_path)?;
        }
        std::fs::rename(log_path, backup_path)?;
    }

    Ok(())
}

pub fn get_package_size_summary(packages: &[Package]) -> String {
    let mut sizes: Vec<u64> = packages.iter().filter_map(|p| p.size).collect();
    sizes.sort_unstable();

    if sizes.is_empty() {
        return "No size data available".to_string();
    }

    let total: u64 = sizes.iter().sum();
    let count = sizes.len();
    let avg = total / count as u64;
    let min = sizes[0];
    let max = sizes[sizes.len() - 1];
    let median = if count % 2 == 0 {
        (sizes[count / 2 - 1] + sizes[count / 2]) / 2
    } else {
        sizes[count / 2]
    };

    format!(
        "Total: {}, Avg: {}, Median: {}, Min: {}, Max: {}",
        format_size(total),
        format_size(avg),
        format_size(median),
        format_size(min),
        format_size(max)
    )
}

pub fn get_package_dependency_summary(packages: &[Package]) -> String {
    let with_deps = packages.iter().filter(|p| p.dependencies.is_some()).count();

    let total_deps: usize = packages
        .iter()
        .filter_map(|p| p.dependencies.as_ref())
        .map(|d| d.len())
        .sum();

    format!(
        "{} packages have dependencies, {} total dependencies",
        with_deps, total_deps
    )
}

pub fn get_dpkg_db_mtime() -> Option<SystemTime> {
    let db_paths = [
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

pub fn build_file_package_map() -> Result<std::collections::HashMap<String, String>> {
    let mut map = std::collections::HashMap::new();

    let output = Command::new("dpkg").arg("-S").arg("/*").output()?;

    if !output.status.success() {
        return Err(anyhow!("dpkg -S failed with status: {}", output.status));
    }

    let stdout = String::from_utf8(output.stdout)?;
    for line in stdout.lines() {
        if let Some((package, paths)) = line.split_once(':') {
            let package = package.trim().to_string();
            for path in paths.split(',').map(|s| s.trim()) {
                if let Some(filename) = path.split('/').last() {
                    if !filename.is_empty() {
                        map.insert(filename.to_string(), package.clone());
                    }
                }
            }
        }
    }

    Ok(map)
}

pub fn resolve_filename_to_package(
    filename: &str,
    file_map: &std::collections::HashMap<String, String>,
) -> Option<String> {
    if let Some(pkg) = file_map.get(filename) {
        return Some(pkg.clone());
    }
    if let Some(stem) = filename.split('.').next() {
        if let Some(pkg) = file_map.get(stem) {
            return Some(pkg.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Package, PackageSource};
    use tempfile::tempdir;

    #[test]
    fn test_format_size() {
        assert_eq!(format_size(0), "0.0 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 1024), "1.0 TB");
    }

    #[test]
    fn test_parse_version() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
    }

    #[test]
    fn test_compare_versions() {
        assert_eq!(
            compare_versions("1.2.3", "1.2.3"),
            Some(std::cmp::Ordering::Equal)
        );
        assert_eq!(
            compare_versions("1.2.4", "1.2.3"),
            Some(std::cmp::Ordering::Greater)
        );
        assert_eq!(
            compare_versions("1.1.3", "1.2.3"),
            Some(std::cmp::Ordering::Less)
        );
        assert_eq!(
            compare_versions("1.2.3", "2.0.0"),
            Some(std::cmp::Ordering::Less)
        );
    }

    #[test]
    fn test_get_path_size() -> Result<()> {
        let dir = tempdir()?;
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, "hello world")?;

        let size = get_path_size(&file_path);
        assert_eq!(size, Some(11));

        let size = get_path_size(dir.path());
        assert!(size.is_some());
        assert!(size.unwrap() >= 11);

        Ok(())
    }

    #[test]
    fn test_is_path_safe() {
        assert!(is_path_safe(Path::new("/usr/bin")));
        assert!(!is_path_safe(Path::new("../etc/passwd")));
        assert!(!is_path_safe(Path::new("//etc/passwd")));
    }

    #[test]
    fn test_packages_to_csv() -> Result<()> {
        let pkg = Package {
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
        };

        let csv = packages_to_csv(&[pkg])?;
        assert!(csv.contains("test,1.0.0,pkg,/usr/bin/test,1.0 KB"));

        Ok(())
    }

    #[test]
    fn test_packages_to_markdown() -> Result<()> {
        let pkg = Package {
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
        };

        let md = packages_to_markdown(&[pkg])?;
        assert!(md.contains("| test | 1.0.0 | pkg | /usr/bin/test | 1.0 KB |"));

        Ok(())
    }

    #[test]
    fn test_resolve_filename_to_package() {
        let mut map = std::collections::HashMap::new();
        map.insert("nm".to_string(), "binutils".to_string());
        map.insert("libcrypto.so.3".to_string(), "openssl".to_string());

        assert_eq!(
            resolve_filename_to_package("nm", &map),
            Some("binutils".to_string())
        );
        assert_eq!(
            resolve_filename_to_package("libcrypto.so.3", &map),
            Some("openssl".to_string())
        );
        assert_eq!(resolve_filename_to_package("unknown", &map), None);
    }
}
