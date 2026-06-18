use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

use anyhow::{Result, anyhow};
use chrono::{DateTime, Local, Utc, TimeZone};

use crate::{
    models::*,
    tracker::Tracker,
    utils,
};


#[derive(Clone)]
pub struct Analyzer {
    tracker: Tracker,
}

impl Analyzer {
    pub fn new(tracker: Tracker) -> Self {
        Self { tracker }
    }

    pub fn get_tracker(&self) -> Tracker {
        self.tracker.clone()
    }

    // ============ UNUSED PACKAGE ANALYSIS ============

    pub fn find_unused(&self, days_threshold: u32) -> Result<Vec<UnusedPackage>> {
        self.tracker.find_unused(days_threshold)
    }

    pub fn find_unused_with_deps(&self, days_threshold: u32) -> Result<Vec<UnusedPackage>> {
        self.tracker.find_unused_with_deps(days_threshold)
    }

    // ============ COMPREHENSIVE ANALYSIS ============

    pub fn analyze(&self, days_threshold: u32) -> Result<AnalysisReport> {
        let unused = self.find_unused_with_deps(days_threshold)?;
        let all_packages = self.tracker.get_installed_packages_all()?;
        let used_packages = self.tracker.get_used_packages()?;
        
        let mut report = AnalysisReport {
            total_packages: all_packages.len(),
            used_packages: used_packages.len(),
            unused_packages: unused.len(),
            total_size: 0,
            potential_savings: 0,
            packages_by_source: HashMap::new(),
            large_unused: Vec::new(),
            recommendations: Vec::new(),
        };
        
        // Calculate totals
        for pkg in &all_packages {
            if let Some(size) = pkg.size {
                report.total_size += size;
            }
        }
        
        for pkg in &unused {
            if let Some(size) = pkg.size {
                report.potential_savings += size;
            }
        }
        
        // Group by source
        for pkg in &all_packages {
            *report.packages_by_source
                .entry(pkg.source.to_string())
                .or_insert(0) += 1;
        }
        
        // Find large unused packages (>10MB)
        for pkg in &unused {
            if let Some(size) = pkg.size {
                if size > 10 * 1024 * 1024 {
                    report.large_unused.push(pkg.clone());
                }
            }
        }
        
        // Generate recommendations
        if !unused.is_empty() {
            report.recommendations.push(format!(
                "Remove {} unused packages to free up {}",
                unused.len(),
                utils::format_size(report.potential_savings)
            ));
            
            if !report.large_unused.is_empty() {
                report.recommendations.push(format!(
                    "Start with large packages: {}",
                    report.large_unused
                        .iter()
                        .map(|p| p.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
        }
        
        Ok(report)
    }

    // ============ DEPENDENCY GRAPH ============

    pub fn get_dependency_graph(&self, package: &str) -> Result<DependencyGraph> {
        let _deps = self.tracker.get_dependencies(package)?;
        let reverse_deps = self.tracker.get_reverse_dependencies(package)?;
        
        let mut graph = DependencyGraph {
            root: package.to_string(),
            dependencies: Vec::new(),
            dependents: reverse_deps,
            depth: 0,
            cycles: Vec::new(),
            total_nodes: 0,
        };
        
        // Build dependency tree using cached data
        self.build_dependency_tree(package, &mut graph, 0, &mut HashSet::new())?;
        
        // Find cycles
        self.find_cycles(package, &mut graph)?;
        
        graph.total_nodes = graph.dependencies.len() + 1;
        
        Ok(graph)
    }

    fn build_dependency_tree(
        &self,
        package: &str,
        graph: &mut DependencyGraph,
        depth: usize,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        if visited.contains(package) {
            return Ok(());
        }
        visited.insert(package.to_string());
        
        if depth > graph.depth {
            graph.depth = depth;
        }
        
        // Use cached dependencies
        let deps = self.tracker.get_dependencies(package)?;
        
        for dep in deps {
            let version = self.get_package_version(&dep);
            let source = self.get_package_source(&dep);
            
            graph.dependencies.push(DependencyNode {
                name: dep.clone(),
                depth: depth + 1,
                parent: package.to_string(),
                version,
                source,
            });
            
            if depth < 10 {
                self.build_dependency_tree(&dep, graph, depth + 1, visited)?;
            }
        }
        
        Ok(())
    }

    fn get_package_version(&self, package: &str) -> Option<String> {
        if let Ok(info) = self.tracker.get_package_info(package) {
            info.version
        } else {
            None
        }
    }

    fn get_package_source(&self, package: &str) -> PackageSource {
        if let Ok(info) = self.tracker.get_package_info(package) {
            info.source
        } else {
            PackageSource::Unknown
        }
    }

    // ============ CYCLE DETECTION ============

    fn find_cycles(&self, package: &str, graph: &mut DependencyGraph) -> Result<()> {
        let mut visited = HashSet::new();
        let mut path = Vec::new();
        self.find_cycles_recursive(package, &mut visited, &mut path, graph)?;
        Ok(())
    }

    fn find_cycles_recursive(
        &self,
        package: &str,
        visited: &mut HashSet<String>,
        path: &mut Vec<String>,
        graph: &mut DependencyGraph,
    ) -> Result<()> {
        if path.contains(&package.to_string()) {
            let cycle_start = path.iter().position(|p| p == package).unwrap_or(0);
            let cycle: Vec<String> = path[cycle_start..].to_vec();
            if !graph.cycles.contains(&cycle) {
                graph.cycles.push(cycle);
            }
            return Ok(());
        }
        
        if visited.contains(package) {
            return Ok(());
        }
        
        visited.insert(package.to_string());
        path.push(package.to_string());
        
        let deps = self.tracker.get_dependencies(package)?;
        for dep in deps {
            self.find_cycles_recursive(&dep, visited, path, graph)?;
        }
        
        path.pop();
        Ok(())
    }

    // ============ SUMMARY ============

    pub fn summary(&self) -> Result<String> {
        let all_packages = self.tracker.get_installed_packages_all()?;
        let used = self.tracker.get_used_packages()?;
        let total_size: u64 = all_packages.iter().filter_map(|p| p.size).sum();
        
        let mut summary = String::new();
        summary.push_str("Package Summary\n");
        summary.push_str(&format!("{}\n", "=".repeat(40)));
        summary.push_str(&format!("Total packages:   {}\n", all_packages.len()));
        summary.push_str(&format!("Used packages:    {}\n", used.len()));
        summary.push_str(&format!("Total size:       {}\n", utils::format_size(total_size)));
        summary.push_str("\n");
        
        let mut by_source: HashMap<String, (usize, u64)> = HashMap::new();
        for pkg in &all_packages {
            let entry = by_source.entry(pkg.source.to_string()).or_insert((0, 0));
            entry.0 += 1;
            if let Some(size) = pkg.size {
                entry.1 += size;
            }
        }
        
        summary.push_str("By source:\n");
        let mut sources: Vec<_> = by_source.into_iter().collect();
        sources.sort_by(|a, b| b.1.0.cmp(&a.1.0));
        for (source, (count, size)) in sources {
            summary.push_str(&format!(
                "  {}: {} packages ({} total)\n", 
                source, count, utils::format_size(size)
            ));
        }
        
        Ok(summary)
    }

    // ============ SAFE REMOVAL ============

    pub fn get_safe_to_remove(&self, days_threshold: u32) -> Result<Vec<UnusedPackage>> {
        let all_unused = self.find_unused_with_deps(days_threshold)?;
        
        let safe: Vec<_> = all_unused
            .into_iter()
            .filter(|pkg| {
                // Don't remove dependencies
                if pkg.status == PackageStatus::Dependency {
                    return false;
                }
                
                // Don't remove system critical packages
                if self.is_system_critical(&pkg.name) {
                    return false;
                }
                
                true
            })
            .collect();
        
        Ok(safe)
    }

    fn is_system_critical(&self, package: &str) -> bool {
        let critical = [
            "bash", "coreutils", "findutils", "grep", "sed", "awk",
            "tar", "gzip", "xz-utils", "termux-tools", "termux-exec",
            "termux-keyring", "termux-am", "termux-api",
            "apk-tools", "apt", "dpkg", "busybox",
            "ca-certificates", "openssl", "libc++", "libandroid-support",
        ];
        critical.contains(&package)
    }

    // ============ COMPARISON ============

    pub fn compare(&self, other_list: &[Package]) -> Result<Comparison> {
        let current = self.tracker.get_installed_packages_all()?;
        let current_names: HashSet<String> = current.iter().map(|p| p.name.clone()).collect();
        let other_names: HashSet<String> = other_list.iter().map(|p| p.name.clone()).collect();
        
        let only_current: Vec<_> = current
            .clone()
            .into_iter()
            .filter(|p| !other_names.contains(&p.name))
            .collect();
        
        let only_other: Vec<_> = other_list
            .iter()
            .filter(|p| !current_names.contains(&p.name))
            .cloned()
            .collect();
        
        let common: Vec<_> = current_names
            .intersection(&other_names)
            .cloned()
            .collect();
        
        let mut version_diffs = Vec::new();
        for name in &common {
            let current_pkg = current.iter().find(|p| &p.name == name);
            let other_pkg = other_list.iter().find(|p| &p.name == name);
            if let (Some(curr), Some(other)) = (current_pkg, other_pkg) {
                if curr.version != other.version {
                    version_diffs.push(VersionDiff {
                        package: name.clone(),
                        current_version: curr.version.clone(),
                        other_version: other.version.clone(),
                    });
                }
            }
        }
        
        Ok(Comparison {
            only_current,
            only_other,
            common,
            version_differences: version_diffs,
        })
    }

    // ============ RISK ANALYSIS ============

    pub fn analyze_risk(&self, days_threshold: u32) -> Result<AnalysisResult> {
        let unused = self.find_unused_with_deps(days_threshold)?;
        let mut safe = Vec::new();
        let mut protected = Vec::new();
        let mut system_critical = Vec::new();
        let mut total_savings = 0;
        
        for pkg in unused {
            if pkg.status == PackageStatus::SystemCritical {
                system_critical.push(pkg.name);
            } else if pkg.status == PackageStatus::Protected || pkg.status == PackageStatus::Dependency {
                protected.push(pkg);
            } else {
                if let Some(size) = pkg.size {
                    total_savings += size;
                }
                safe.push(pkg);
            }
        }
        
        let risk_level = if !system_critical.is_empty() {
            RiskLevel::Critical
        } else if !protected.is_empty() {
            RiskLevel::High
        } else if !safe.is_empty() && total_savings > 100 * 1024 * 1024 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };
        
        Ok(AnalysisResult {
            safe_to_remove: safe,
            protected,
            system_critical,
            total_savings,
            risk_level,
        })
    }

    // ============ DEPENDENCY CHAIN ============

    pub fn get_dependency_chain(&self, from: &str, to: &str) -> Result<Vec<String>> {
        let mut visited = HashSet::new();
        let mut path = Vec::new();
        let mut found = Vec::new();
        
        self.find_path(from, to, &mut visited, &mut path, &mut found)?;
        
        if found.is_empty() {
            Err(anyhow!("No path found from {} to {}", from, to))
        } else {
            Ok(found)
        }
    }

    fn find_path(
        &self,
        current: &str,
        target: &str,
        visited: &mut HashSet<String>,
        path: &mut Vec<String>,
        found: &mut Vec<String>,
    ) -> Result<()> {
        if visited.contains(current) || !found.is_empty() {
            return Ok(());
        }
        
        visited.insert(current.to_string());
        path.push(current.to_string());
        
        if current == target {
            found.extend(path.clone());
            return Ok(());
        }
        
        let deps = self.tracker.get_dependencies(current)?;
        for dep in deps {
            self.find_path(&dep, target, visited, path, found)?;
            if !found.is_empty() {
                break;
            }
        }
        
        path.pop();
        Ok(())
    }

    // ============ CACHE STATS ============

    pub fn get_cache_stats(&self) -> Result<String> {
        let tracker = self.tracker.clone();
        let cache_manager = tracker.cache_manager.clone();
        
        let pkg_stats = cache_manager.get_cache_stats()?;
        let dep_stats = cache_manager.get_dependency_cache_stats()?;
        let usage_stats = cache_manager.get_usage_cache_stats()?;
        
        let mut output = String::new();
        output.push_str("Cache Statistics\n");
        output.push_str(&format!("{}\n", "=".repeat(40)));
        output.push_str(&format!("Package cache: {}\n", pkg_stats.summary()));
        
        if let Some(stats) = dep_stats {
            output.push_str(&format!("Dependency cache: {}\n", stats.summary()));
        } else {
            output.push_str("Dependency cache: Not built\n");
        }
        
        if let Some(stats) = usage_stats {
            output.push_str(&format!("Usage cache: {}\n", stats.summary()));
        } else {
            output.push_str("Usage cache: Not built\n");
        }
        
        Ok(output)
    }

    pub fn rebuild_caches(&self) -> Result<()> {
        self.tracker.rebuild_caches()
    }
}

#[derive(Debug)]
pub struct AnalysisReport {
    pub total_packages: usize,
    pub used_packages: usize,
    pub unused_packages: usize,
    pub total_size: u64,
    pub potential_savings: u64,
    pub packages_by_source: HashMap<String, usize>,
    pub large_unused: Vec<UnusedPackage>,
    pub recommendations: Vec<String>,
}