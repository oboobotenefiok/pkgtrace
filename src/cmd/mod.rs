use std::{
    collections::HashSet,
    io::{self, Write},
};

use anyhow::Result;
use chrono::{Local, TimeZone, Utc};
use colored::Colorize;

use crate::{
    analyzer::Analyzer,
    models::{DependencyNode, Package, PackageSource, PackageStatus},
    tracker::Tracker,
    utils,
};

pub fn cmd_list(
    tracker: &Tracker,
    show_sizes: bool,
    source: Option<&str>,
    min_size: Option<u64>,
    used_only: bool,
) -> Result<()> {
    let packages = tracker.get_installed_packages_all()?;
    let used_packages = if used_only {
        tracker.get_used_packages()?
    } else {
        HashSet::new()
    };

    let mut filtered: Vec<_> = packages
        .into_iter()
        .filter(|pkg| {
            if let Some(src) = source {
                if pkg.source.to_string().to_lowercase() != src.to_lowercase() {
                    return false;
                }
            }
            if let Some(min_mb) = min_size {
                if let Some(size) = pkg.size {
                    if size < min_mb * 1024 * 1024 {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            if used_only && !used_packages.contains(&pkg.name) {
                return false;
            }
            true
        })
        .collect();

    filtered.sort_by(|a, b| a.name.cmp(&b.name));

    if filtered.is_empty() {
        println!("No packages found matching criteria");
        return Ok(());
    }

    let total_size: u64 = filtered.iter().filter_map(|p| p.size).sum();
    println!(
        "Found {} packages (total size: {})",
        filtered.len(),
        utils::format_size(total_size)
    );
    println!("{}", "─".repeat(80));

    for pkg in filtered {
        let source_color = match pkg.source {
            PackageSource::Pkg => "green",
            PackageSource::Cargo => "cyan",
            PackageSource::Pip => "yellow",
            PackageSource::Npm => "red",
            PackageSource::Gem => "magenta",
            PackageSource::Manual => "white",
            PackageSource::Unknown => "white",
        };

        let source_str = pkg.source.to_string().color(source_color);
        let version_str = pkg
            .version
            .map(|v| format!("v{}", v))
            .unwrap_or_else(|| "unknown".to_string());

        let used_marker = if used_packages.contains(&pkg.name) {
            " [used]".green()
        } else {
            "".normal()
        };

        let size_str = if show_sizes {
            pkg.size
                .map(|s| format!(" ({})", utils::format_size(s)))
                .unwrap_or_else(|| " (size unknown)".to_string())
        } else {
            String::new()
        };

        println!(
            "  {} {} {}{}{}",
            pkg.name.green().bold(),
            version_str.dimmed(),
            source_str,
            size_str.dimmed(),
            used_marker
        );
    }

    Ok(())
}

pub fn cmd_unused(
    tracker: &Tracker,
    days: u32,
    explain: bool,
    deps: bool,
    min_size: Option<u64>,
    remove: bool,
    dry_run: bool,
) -> Result<()> {
    let unused = if deps {
        tracker.find_unused_with_deps(days)?
    } else {
        tracker.find_unused(days)?
    };

    let unused: Vec<_> = unused
        .into_iter()
        .filter(|pkg| {
            if let Some(min_mb) = min_size {
                if let Some(size) = pkg.size {
                    size >= min_mb * 1024 * 1024
                } else {
                    false
                }
            } else {
                true
            }
        })
        .collect();

    if unused.is_empty() {
        println!("No unused packages found (threshold: {} days)", days);
        return Ok(());
    }

    let total_size: u64 = unused.iter().filter_map(|p| p.size).sum();

    println!("Unused Packages Found");
    println!(
        "Threshold: {} days | Found: {} packages | Total size: {}",
        days,
        unused.len(),
        utils::format_size(total_size)
    );
    println!("{}", "─".repeat(80));

    for pkg in &unused {
        let last_used_str = match pkg.last_used {
            Some(ts) => {
                let date = chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                format!("last used: {} ({} days ago)", date, pkg.days_unused)
            }
            None => "NEVER USED".red().to_string(),
        };

        let size_str = pkg
            .size
            .map(utils::format_size)
            .unwrap_or_else(|| "unknown".to_string());

        let status_str = match pkg.status {
            PackageStatus::Dependency => " [dependency]".yellow(),
            PackageStatus::Protected => " [protected]".blue(),
            _ => "".normal(),
        };

        println!(
            "  {} ({}) - {} - {}{}",
            pkg.name.cyan().bold(),
            pkg.source.to_string().dimmed(),
            last_used_str,
            size_str.dimmed(),
            status_str
        );

        if explain {
            match tracker.explain_protection(&pkg.name) {
                Ok(info) => println!("    -> {}", info.dimmed()),
                Err(e) => println!("    -> Error: {}", e),
            }
        }
    }

    println!("{}", "─".repeat(80));

    if remove {
        if dry_run {
            println!(
                "\nDRY RUN - would remove {} packages ({} total)",
                unused.len(),
                utils::format_size(total_size)
            );
            return Ok(());
        }

        println!("\nThis will REMOVE the following packages:");
        for pkg in &unused {
            println!("  - {}", pkg.name);
        }

        println!("\nTotal size to free: {}", utils::format_size(total_size));

        if !confirm_action("Proceed with removal?")? {
            println!("Aborted");
            return Ok(());
        }

        let mut removed = 0;
        let mut failed = 0;

        for pkg in &unused {
            match tracker.remove_package(pkg) {
                Ok(_) => {
                    println!("Removed {}", pkg.name);
                    removed += 1;
                }
                Err(e) => {
                    println!("Failed to remove {}: {}", pkg.name, e);
                    failed += 1;
                }
            }
        }

        println!("\nRemoved: {}, Failed: {}", removed, failed);
    } else {
        println!(
            "\nTo remove these packages: pkgtrace clean --days {} --min-size {}",
            days,
            min_size.unwrap_or(0)
        );
        println!(
            "Or use: pkgtrace unused --remove --days {} --dry-run first",
            days
        );
    }

    Ok(())
}

pub fn cmd_deps(
    tracker: &Tracker,
    package: &str,
    reverse: bool,
    tree: bool,
    depth: Option<usize>,
) -> Result<()> {
    if reverse {
        let dependents = tracker.get_reverse_dependencies(package)?;
        if dependents.is_empty() {
            println!("No packages depend on '{}'", package);
        } else {
            println!(
                "Packages that depend on '{}' ({}):",
                package,
                dependents.len()
            );
            for dep in dependents {
                println!("  - {}", dep);
            }
        }
    } else if tree {
        tracker.print_dependency_tree(package, depth.unwrap_or(10))?;
    } else {
        let deps = tracker.get_dependencies(package)?;
        if deps.is_empty() {
            println!("'{}' has no dependencies", package);
        } else {
            println!("Dependencies for '{}' ({}):", package, deps.len());
            for dep in deps {
                println!("  - {}", dep);
            }
        }
    }
    Ok(())
}

pub fn cmd_info(tracker: &Tracker, package: &str, verbose: bool) -> Result<()> {
    let info = tracker.get_package_info(package)?;

    println!("Package Information");
    println!("{}", "─".repeat(40));
    println!("Name:     {}", info.name.green());
    println!(
        "Version:  {}",
        info.version.unwrap_or_else(|| "unknown".to_string())
    );
    println!("Source:   {}", info.source.to_string());
    println!("Location: {}", info.install_path.display());
    if let Some(size) = info.size {
        println!("Size:     {}", utils::format_size(size));
    }
    if let Some(installed) = info.installed_date {
        println!("Installed: {}", installed);
    }
    if let Some(last_used) = info.last_used_date {
        println!("Last used: {}", last_used);
    }
    if let Some(count) = info.usage_count {
        println!("Usage count: {}", count);
    }
    if let Some(deps) = info.dependencies {
        if !deps.is_empty() {
            println!("\nDependencies ({}):", deps.len());
            for dep in deps {
                println!("  - {}", dep);
            }
        }
    }
    if let Some(reverse_deps) = info.reverse_dependencies {
        if !reverse_deps.is_empty() && verbose {
            println!("\nReverse Dependencies ({}):", reverse_deps.len());
            for dep in reverse_deps {
                println!("  - {}", dep);
            }
        }
    }
    if verbose {
        if let Some(checksum) = info.checksum {
            println!("Checksum: {}", checksum);
        }
    }

    Ok(())
}

pub fn cmd_scan(tracker: &Tracker, force: bool, background: bool) -> Result<()> {
    if background {
        println!("Starting background scan...");
        let tracker_clone = tracker.clone();
        std::thread::spawn(move || {
            let _ = tracker_clone.scan_all_packages(true);
        });
        println!("Scan started in background");
        return Ok(());
    }

    println!("Scanning packages...");
    let packages = tracker.scan_all_packages(force)?;
    println!("Found {} packages", packages.len());

    let mut by_source = std::collections::HashMap::new();
    for pkg in &packages {
        *by_source.entry(pkg.source.to_string()).or_insert(0) += 1;
    }

    println!("\nBreakdown by source:");
    let mut sources: Vec<_> = by_source.into_iter().collect();
    sources.sort_by(|a, b| b.1.cmp(&a.1));
    for (source, count) in sources {
        println!("  {}: {}", source, count);
    }

    Ok(())
}

pub fn cmd_clean(
    tracker: &Tracker,
    days: u32,
    yes: bool,
    min_size: Option<u64>,
    dry_run: bool,
) -> Result<()> {
    let unused = tracker.find_unused_with_deps(days)?;

    let unused: Vec<_> = unused
        .into_iter()
        .filter(|pkg| {
            if let Some(min_mb) = min_size {
                if let Some(size) = pkg.size {
                    size >= min_mb * 1024 * 1024
                } else {
                    false
                }
            } else {
                true
            }
        })
        .collect();

    if unused.is_empty() {
        println!("No packages to clean");
        return Ok(());
    }

    let total_size: u64 = unused.iter().filter_map(|p| p.size).sum();
    println!("Found {} packages to clean:", unused.len());
    for pkg in &unused {
        println!(
            "  - {} ({} days unused, {})",
            pkg.name,
            pkg.days_unused,
            pkg.size
                .map(utils::format_size)
                .unwrap_or_else(|| "unknown".to_string())
        );
    }
    println!("Total size to free: {}", utils::format_size(total_size));

    if dry_run {
        println!("\nDRY RUN - would remove {} packages", unused.len());
        return Ok(());
    }

    if !yes && !confirm_action("Remove these packages?")? {
        println!("Aborted");
        return Ok(());
    }

    let mut removed = 0;
    let mut failed = 0;

    for pkg in &unused {
        match tracker.remove_package(pkg) {
            Ok(_) => {
                println!("Removed {}", pkg.name);
                removed += 1;
            }
            Err(e) => {
                println!("Failed to remove {}: {}", pkg.name, e);
                failed += 1;
            }
        }
    }

    println!("\nRemoved: {}, Failed: {}", removed, failed);
    Ok(())
}

pub fn cmd_export(
    tracker: &Tracker,
    format: &str,
    output: Option<&str>,
    include_deps: bool,
) -> Result<()> {
    let packages = tracker.get_installed_packages_all()?;

    let export_data = if include_deps {
        let mut data = Vec::new();
        for pkg in &packages {
            if let Ok(deps) = tracker.get_dependencies(&pkg.name) {
                let mut pkg_with_deps = pkg.clone();
                pkg_with_deps.dependencies = Some(deps);
                data.push(pkg_with_deps);
            } else {
                data.push(pkg.clone());
            }
        }
        data
    } else {
        packages
    };

    let output_str = match format.to_lowercase().as_str() {
        "json" => serde_json::to_string_pretty(&export_data)?,
        "csv" => utils::packages_to_csv(&export_data)?,
        "markdown" => utils::packages_to_markdown(&export_data)?,
        "yaml" => {
            let yaml = serde_yaml::to_string(&export_data)?;
            yaml
        }
        _ => return Err(anyhow::anyhow!("Unsupported format: {}", format)),
    };

    if let Some(path) = output {
        std::fs::write(path, output_str)?;
        println!("Exported to {}", path);
    } else {
        println!("{}", output_str);
    }

    Ok(())
}

pub fn cmd_import(tracker: &Tracker, file: &str, dry_run: bool, force: bool) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    let packages: Vec<Package> = serde_json::from_str(&content)?;

    println!("Found {} packages to import", packages.len());

    if dry_run {
        println!("DRY RUN - would install:");
        for pkg in &packages {
            println!("  - {} ({})", pkg.name, pkg.source);
        }
        return Ok(());
    }

    let mut installed = 0;
    let mut failed = 0;

    for pkg in &packages {
        if !force {
            if tracker.get_package_info(&pkg.name).is_ok() {
                println!("Skipping {} (already installed)", pkg.name);
                continue;
            }
        }
        match tracker.install_package(pkg) {
            Ok(_) => {
                println!("Installed {}", pkg.name);
                installed += 1;
            }
            Err(e) => {
                println!("Failed to install {}: {}", pkg.name, e);
                failed += 1;
            }
        }
    }

    println!("\nInstalled: {}, Failed: {}", installed, failed);
    Ok(())
}

pub fn cmd_analyze(analyzer: &Analyzer, days: u32, output: Option<&str>) -> Result<()> {
    let report = analyzer.analyze(days)?;

    let report_str = format!(
        "Package Analysis Report\n\
        {}\n\
        Total packages:   {}\n\
        Used packages:    {}\n\
        Unused packages:  {}\n\
        Total size:       {}\n\
        Potential savings: {}\n\
        \n\
        Packages by source:\n\
        {}\n\
        \n\
        Recommendations:\n\
        {}\n",
        "=".repeat(50),
        report.total_packages,
        report.used_packages,
        report.unused_packages,
        utils::format_size(report.total_size),
        utils::format_size(report.potential_savings),
        report
            .packages_by_source
            .iter()
            .map(|(k, v)| format!("  {}: {}", k, v))
            .collect::<Vec<_>>()
            .join("\n"),
        report
            .recommendations
            .iter()
            .map(|r| format!("  - {}", r))
            .collect::<Vec<_>>()
            .join("\n")
    );

    if let Some(path) = output {
        std::fs::write(path, report_str)?;
        println!("Report saved to {}", path);
    } else {
        println!("{}", report_str);
    }

    if !report.large_unused.is_empty() {
        println!("\nLarge unused packages (>10MB):");
        for pkg in &report.large_unused {
            println!(
                "  - {} ({})",
                pkg.name,
                pkg.size
                    .map(utils::format_size)
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
    }

    Ok(())
}

pub fn cmd_graph(
    analyzer: &Analyzer,
    package: &str,
    format: Option<&str>,
    output: Option<&str>,
) -> Result<()> {
    let graph = analyzer.get_dependency_graph(package)?;

    if let Some(fmt) = format {
        match fmt.to_lowercase().as_str() {
            "dot" => {
                let dot = utils::graph_to_dot(&graph)?;
                if let Some(path) = output {
                    std::fs::write(path, dot)?;
                    println!("Graph saved to {}", path);
                } else {
                    println!("{}", dot);
                }
            }
            "json" => {
                let json = serde_json::to_string_pretty(&graph)?;
                if let Some(path) = output {
                    std::fs::write(path, json)?;
                    println!("Graph saved to {}", path);
                } else {
                    println!("{}", json);
                }
            }
            _ => {
                println!("Unsupported format: {}", fmt);
                println!("Supported formats: dot, json");
            }
        }
        return Ok(());
    }

    println!("Dependency graph for '{}':", package);
    println!("{}", "─".repeat(40));

    let mut depth_map: std::collections::HashMap<usize, Vec<&DependencyNode>> =
        std::collections::HashMap::new();
    for node in &graph.dependencies {
        depth_map.entry(node.depth).or_default().push(node);
    }

    for depth in 0..=graph.depth {
        if let Some(nodes) = depth_map.get(&depth) {
            let indent = "  ".repeat(depth);
            for node in nodes {
                println!(
                    "{}{} └── {}",
                    indent,
                    if depth > 0 { "│" } else { "" },
                    node.name
                );
            }
        }
    }

    if !graph.cycles.is_empty() {
        println!("\nCycles detected:");
        for cycle in &graph.cycles {
            println!("  {}", cycle.join(" -> "));
        }
    }

    if !graph.dependents.is_empty() {
        println!("\nReverse dependencies ({}):", graph.dependents.len());
        for dep in &graph.dependents {
            println!("  - {}", dep);
        }
    }

    Ok(())
}

pub fn cmd_safe_remove(analyzer: &Analyzer, days: u32, yes: bool, dry_run: bool) -> Result<()> {
    let safe = analyzer.get_safe_to_remove(days)?;

    if safe.is_empty() {
        println!("No safe packages to remove");
        return Ok(());
    }

    let total_size: u64 = safe.iter().filter_map(|p| p.size).sum();
    println!("Found {} safe packages to remove:", safe.len());
    println!("Total size to free: {}", utils::format_size(total_size));

    for pkg in &safe {
        println!(
            "  - {} ({} days unused, {})",
            pkg.name,
            pkg.days_unused,
            pkg.size
                .map(utils::format_size)
                .unwrap_or_else(|| "unknown".to_string())
        );
    }

    if dry_run {
        println!("\nDRY RUN - would remove {} packages", safe.len());
        return Ok(());
    }

    if !yes && !confirm_action("Remove these packages?")? {
        println!("Aborted");
        return Ok(());
    }

    let tracker = analyzer.get_tracker();
    let mut removed = 0;
    let mut failed = 0;

    for pkg in &safe {
        match tracker.remove_package(pkg) {
            Ok(_) => {
                println!("Removed {}", pkg.name);
                removed += 1;
            }
            Err(e) => {
                println!("Failed to remove {}: {}", pkg.name, e);
                failed += 1;
            }
        }
    }

    println!("\nRemoved: {}, Failed: {}", removed, failed);
    Ok(())
}

pub fn cmd_stats(tracker: &Tracker) -> Result<()> {
    let stats = tracker.get_stats()?;

    println!("Package Statistics");
    println!("{}", "=".repeat(50));
    println!("Total packages:      {}", stats.total_packages);
    println!("Used packages:       {}", stats.used_packages);
    println!(
        "Unused packages:     {}",
        stats.total_packages - stats.used_packages
    );
    println!(
        "Total size:          {}",
        utils::format_size(stats.total_size)
    );
    println!(
        "Average size:        {}",
        utils::format_size(stats.average_package_size)
    );
    println!("\nBy source:");

    for (source, count, size) in &stats.by_source {
        println!(
            "  {}: {} packages ({} total)",
            source,
            count,
            utils::format_size(*size)
        );
    }

    if !stats.largest_packages.is_empty() {
        println!("\nLargest packages:");
        for pkg in &stats.largest_packages {
            println!(
                "  - {} ({})",
                pkg.name,
                pkg.size
                    .map(utils::format_size)
                    .unwrap_or_else(|| "unknown".to_string())
            );
        }
    }

    let (file_map_entries, file_map_built) = tracker.get_file_map_stats();
    println!("\nFile to Package Mapping:");
    println!("  Total entries: {}", file_map_entries);
    if file_map_built > 0 {
        let dt = Local.from_utc_datetime(&Utc.timestamp(file_map_built, 0).naive_utc());
        println!("  Built at: {}", dt.format("%Y-%m-%d %H:%M:%S"));
    }

    Ok(())
}

pub fn cmd_monitor(tracker: &Tracker, daemon: bool, interval: Option<u64>) -> Result<()> {
    if daemon {
        println!("Starting monitor daemon...");
        let interval = interval.unwrap_or(3600);
        let tracker_clone = tracker.clone();
        std::thread::spawn(move || loop {
            let _ = tracker_clone.scan_all_packages(false);
            std::thread::sleep(std::time::Duration::from_secs(interval));
        });
        println!("Monitor daemon started (interval: {}s)", interval);
        println!("Press Ctrl+C to stop");
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    } else {
        println!("Running one-time monitor scan...");
        let packages = tracker.scan_all_packages(false)?;
        let used = tracker.get_used_packages()?;

        println!("Found {} packages, {} used", packages.len(), used.len());

        let unused: Vec<_> = packages
            .into_iter()
            .filter(|p| !used.contains(&p.name))
            .collect();

        if !unused.is_empty() {
            println!("\nUnused packages:");
            for pkg in unused {
                println!("  - {}", pkg.name);
            }
        }
    }

    Ok(())
}

pub fn cmd_verify(tracker: &Tracker, fix: bool) -> Result<()> {
    println!("Verifying package integrity...");
    let packages = tracker.get_installed_packages_all()?;

    let mut issues = Vec::new();
    for pkg in &packages {
        if !pkg.install_path.exists() {
            issues.push(format!(
                "Package '{}' missing from {}",
                pkg.name,
                pkg.install_path.display()
            ));
        }
    }

    if issues.is_empty() {
        println!("All packages verified");
        return Ok(());
    }

    println!("Found {} issues:", issues.len());
    for issue in &issues {
        println!("  - {}", issue);
    }

    if fix {
        println!("Fixing issues...");
        for pkg in &packages {
            if !pkg.install_path.exists() {
                match tracker.remove_package_from_cache(&pkg.name) {
                    Ok(_) => println!("Removed {} from cache", pkg.name),
                    Err(e) => println!("Failed to remove {} from cache: {}", pkg.name, e),
                }
            }
        }
        println!("Fix complete");
    }

    Ok(())
}

pub fn cmd_search(tracker: &Tracker, query: &str, source: Option<&str>) -> Result<()> {
    let packages = tracker.get_installed_packages_all()?;
    let query_lower = query.to_lowercase();

    let matches: Vec<_> = packages
        .into_iter()
        .filter(|pkg| {
            if let Some(src) = source {
                if pkg.source.to_string().to_lowercase() != src.to_lowercase() {
                    return false;
                }
            }
            pkg.name.to_lowercase().contains(&query_lower)
        })
        .collect();

    if matches.is_empty() {
        println!("No packages found matching '{}'", query);
        return Ok(());
    }

    println!("Found {} packages matching '{}':", matches.len(), query);
    for pkg in matches {
        println!(
            "  {} ({}) {}",
            pkg.name.green(),
            pkg.source.to_string().dimmed(),
            pkg.version
                .map(|v| format!("v{}", v))
                .unwrap_or_else(|| "".to_string())
                .dimmed()
        );
    }

    Ok(())
}

pub fn cmd_autoremove(tracker: &Tracker, yes: bool, dry_run: bool) -> Result<()> {
    let _all_packages = tracker.get_installed_packages_all()?;
    let used_packages = tracker.get_used_packages()?;
    let used_vec: Vec<String> = used_packages.into_iter().collect();

    let dependency_set = tracker.get_all_dependencies(&used_vec)?;

    let unused = tracker.find_unused(30)?;

    let to_remove: Vec<_> = unused
        .into_iter()
        .filter(|pkg| {
            if dependency_set.contains(&pkg.name) {
                return false;
            }
            if Tracker::get_core_packages().contains(&pkg.name.as_str()) {
                return false;
            }
            true
        })
        .collect();

    if to_remove.is_empty() {
        println!("No packages to autoremove");
        return Ok(());
    }

    let total_size: u64 = to_remove.iter().filter_map(|p| p.size).sum();
    println!("Found {} packages to autoremove:", to_remove.len());
    println!("Total size to free: {}", utils::format_size(total_size));

    for pkg in &to_remove {
        println!("  - {} (unused, not a dependency)", pkg.name);
    }

    if dry_run {
        println!("\nDRY RUN - would remove {} packages", to_remove.len());
        return Ok(());
    }

    if !yes && !confirm_action("Remove these packages?")? {
        println!("Aborted");
        return Ok(());
    }

    let mut removed = 0;
    let mut failed = 0;

    for pkg in &to_remove {
        match tracker.remove_package(pkg) {
            Ok(_) => {
                println!("Removed {}", pkg.name);
                removed += 1;
            }
            Err(e) => {
                println!("Failed to remove {}: {}", pkg.name, e);
                failed += 1;
            }
        }
    }

    println!("\nRemoved: {}, Failed: {}", removed, failed);
    Ok(())
}

pub fn cmd_compare(tracker: &Tracker, file: &str, output: Option<&str>) -> Result<()> {
    let content = std::fs::read_to_string(file)?;
    let other_packages: Vec<Package> = serde_json::from_str(&content)?;

    let analyzer = Analyzer::new(tracker.clone());
    let comparison = analyzer.compare(&other_packages)?;

    let mut result = String::new();
    result.push_str(&format!("Comparison with {}\n", file));
    result.push_str(&format!("{}\n", "=".repeat(50)));
    result.push_str(&format!("Common packages: {}\n", comparison.common.len()));
    result.push_str(&format!(
        "Only in current: {}\n",
        comparison.only_current.len()
    ));
    result.push_str(&format!("Only in other: {}\n", comparison.only_other.len()));
    result.push_str(&format!(
        "Version differences: {}\n\n",
        comparison.version_differences.len()
    ));

    if !comparison.only_current.is_empty() {
        result.push_str("Packages only in current:\n");
        for pkg in &comparison.only_current {
            result.push_str(&format!("  - {} ({})\n", pkg.name, pkg.source));
        }
        result.push_str("\n");
    }

    if !comparison.only_other.is_empty() {
        result.push_str("Packages only in other:\n");
        for pkg in &comparison.only_other {
            result.push_str(&format!("  - {} ({})\n", pkg.name, pkg.source));
        }
        result.push_str("\n");
    }

    if !comparison.version_differences.is_empty() {
        result.push_str("Version differences:\n");
        for diff in &comparison.version_differences {
            result.push_str(&format!(
                "  {}: {} vs {}\n",
                diff.package,
                diff.current_version.as_deref().unwrap_or("none"),
                diff.other_version.as_deref().unwrap_or("none")
            ));
        }
    }

    if let Some(path) = output {
        std::fs::write(path, result)?;
        println!("Comparison saved to {}", path);
    } else {
        println!("{}", result);
    }

    Ok(())
}

pub fn cmd_cache_stats(tracker: &Tracker) -> Result<()> {
    let analyzer = Analyzer::new(tracker.clone());
    let stats = analyzer.get_cache_stats()?;
    println!("{}", stats);
    Ok(())
}

pub fn cmd_rebuild_cache(tracker: &Tracker) -> Result<()> {
    println!("Rebuilding caches...");
    let analyzer = Analyzer::new(tracker.clone());
    analyzer.rebuild_caches()?;
    println!("Caches rebuilt successfully");
    Ok(())
}

pub fn cmd_rebuild_file_map(tracker: &Tracker) -> Result<()> {
    println!("Rebuilding file to package mapping...");
    tracker.build_file_map_cache()?;
    println!("File to package mapping rebuilt successfully");
    Ok(())
}

pub fn confirm_action(prompt: &str) -> Result<bool> {
    print!("{} [y/N]: ", prompt);
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;

    let response = input.trim().to_lowercase();
    Ok(response == "y" || response == "yes")
}

