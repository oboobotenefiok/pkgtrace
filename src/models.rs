use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt,
    path::PathBuf,
};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum PackageSource {
    Pkg,
    Cargo,
    Pip,
    Npm,
    Gem,
    Manual,
    Unknown,
}

impl fmt::Display for PackageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageSource::Pkg => write!(f, "pkg"),
            PackageSource::Cargo => write!(f, "cargo"),
            PackageSource::Pip => write!(f, "pip"),
            PackageSource::Npm => write!(f, "npm"),
            PackageSource::Gem => write!(f, "gem"),
            PackageSource::Manual => write!(f, "manual"),
            PackageSource::Unknown => write!(f, "unknown"),
        }
    }
}

impl PackageSource {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "pkg" => PackageSource::Pkg,
            "cargo" => PackageSource::Cargo,
            "pip" => PackageSource::Pip,
            "npm" => PackageSource::Npm,
            "gem" => PackageSource::Gem,
            "manual" => PackageSource::Manual,
            _ => PackageSource::Unknown,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Package {
    pub name: String,
    pub version: Option<String>,
    pub source: PackageSource,
    pub install_path: PathBuf,
    pub size: Option<u64>,
    #[serde(default)]
    pub dependencies: Option<Vec<String>>,
    #[serde(default)]
    pub installed_date: Option<i64>,
    #[serde(default)]
    pub last_used: Option<i64>,
    #[serde(default)]
    pub usage_count: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
}

impl PartialEq for Package {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.source == other.source
    }
}

impl Eq for Package {}

impl std::hash::Hash for Package {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.source.hash(state);
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PackageEvent {
    pub timestamp: i64,
    pub package: String,
    pub action: String,
    pub source: PackageSource,
    #[serde(default)]
    pub details: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UnusedPackage {
    pub name: String,
    pub source: PackageSource,
    pub last_used: Option<i64>,
    pub days_unused: u32,
    pub size: Option<u64>,
    pub status: PackageStatus,
    pub install_path: PathBuf,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PackageStatus {
    Unused,
    NeverLogged,
    Protected,
    Dependency,
    SystemCritical,
    Safe,
}

impl fmt::Display for PackageStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageStatus::Unused => write!(f, "unused"),
            PackageStatus::NeverLogged => write!(f, "never logged"),
            PackageStatus::Protected => write!(f, "protected"),
            PackageStatus::Dependency => write!(f, "dependency"),
            PackageStatus::SystemCritical => write!(f, "system critical"),
            PackageStatus::Safe => write!(f, "safe"),
        }
    }
}

#[derive(Debug)]
pub struct PackageInfo {
    pub name: String,
    pub version: Option<String>,
    pub source: PackageSource,
    pub install_path: PathBuf,
    pub size: Option<u64>,
    pub installed_date: Option<String>,
    pub last_used_date: Option<String>,
    pub dependencies: Option<Vec<String>>,
    pub reverse_dependencies: Option<Vec<String>>,
    pub usage_count: Option<u64>,
    pub checksum: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DependencyCache {
    pub direct_deps: HashMap<String, Vec<String>>,
    pub reverse_deps: HashMap<String, Vec<String>>,
    pub built_at: i64,
    pub package_count: usize,
    pub max_depth: usize,
}

impl DependencyCache {
    pub fn new() -> Self {
        Self {
            direct_deps: HashMap::new(),
            reverse_deps: HashMap::new(),
            built_at: 0,
            package_count: 0,
            max_depth: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.direct_deps.is_empty()
    }

    pub fn get_deps(&self, package: &str) -> Option<&Vec<String>> {
        self.direct_deps.get(package)
    }

    pub fn get_reverse_deps(&self, package: &str) -> Option<&Vec<String>> {
        self.reverse_deps.get(package)
    }

    pub fn has_package(&self, package: &str) -> bool {
        self.direct_deps.contains_key(package)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UsageCache {
    pub events: Vec<PackageEvent>,
    pub package_index: HashMap<String, Vec<usize>>,
    pub loaded_at: i64,
    pub last_event_timestamp: i64,
    pub event_count: usize,
}

impl UsageCache {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
            package_index: HashMap::new(),
            loaded_at: 0,
            last_event_timestamp: 0,
            event_count: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn get_events_for_package(&self, package: &str) -> Vec<&PackageEvent> {
        if let Some(indices) = self.package_index.get(package) {
            indices
                .iter()
                .filter_map(|&idx| self.events.get(idx))
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_last_use(&self, package: &str) -> Option<i64> {
        if let Some(indices) = self.package_index.get(package) {
            indices
                .iter()
                .filter_map(|&idx| self.events.get(idx))
                .filter(|e| e.action == "USE" || e.action == "INSTALL")
                .max_by_key(|e| e.timestamp)
                .map(|e| e.timestamp)
        } else {
            None
        }
    }

    pub fn get_usage_count(&self, package: &str) -> usize {
        if let Some(indices) = self.package_index.get(package) {
            indices
                .iter()
                .filter_map(|&idx| self.events.get(idx))
                .filter(|e| e.action == "USE" || e.action == "INSTALL")
                .count()
        } else {
            0
        }
    }

    pub fn get_used_packages(&self) -> std::collections::HashSet<String> {
        let mut used = std::collections::HashSet::new();
        for event in &self.events {
            if event.action == "INSTALL" || event.action == "USE" {
                used.insert(event.package.clone());
            }
        }
        used
    }

    pub fn get_install_time(&self, package: &str) -> Option<i64> {
        if let Some(indices) = self.package_index.get(package) {
            indices
                .iter()
                .filter_map(|&idx| self.events.get(idx))
                .find(|e| e.action == "INSTALL")
                .map(|e| e.timestamp)
        } else {
            None
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FileMapCache {
    pub mapping: HashMap<String, String>,
    pub built_at: i64,
    pub total_entries: usize,
}

impl FileMapCache {
    pub fn new() -> Self {
        Self {
            mapping: HashMap::new(),
            built_at: 0,
            total_entries: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.mapping.is_empty()
    }

    pub fn get_package_for_file(&self, filename: &str) -> Option<String> {
        if let Some(pkg) = self.mapping.get(filename) {
            return Some(pkg.clone());
        }
        if let Some(stem) = filename.split('.').next() {
            if let Some(pkg) = self.mapping.get(stem) {
                return Some(pkg.clone());
            }
        }
        None
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CacheEntry {
    pub package: Package,
    pub last_updated: i64,
    pub scan_duration: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportFormat {
    pub version: String,
    pub timestamp: i64,
    pub packages: Vec<Package>,
    pub metadata: ExportMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportMetadata {
    pub source: String,
    pub host: String,
    pub total_packages: usize,
    pub total_size: u64,
}

#[derive(Debug)]
pub struct AnalysisResult {
    pub safe_to_remove: Vec<UnusedPackage>,
    pub protected: Vec<UnusedPackage>,
    pub system_critical: Vec<String>,
    pub total_savings: u64,
    pub risk_level: RiskLevel,
}

#[derive(Debug)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskLevel::Low => write!(f, "LOW"),
            RiskLevel::Medium => write!(f, "MEDIUM"),
            RiskLevel::High => write!(f, "HIGH"),
            RiskLevel::Critical => write!(f, "CRITICAL"),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DependencyGraph {
    pub root: String,
    pub dependencies: Vec<DependencyNode>,
    pub dependents: Vec<String>,
    pub depth: usize,
    pub cycles: Vec<Vec<String>>,
    pub total_nodes: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DependencyNode {
    pub name: String,
    pub depth: usize,
    pub parent: String,
    pub version: Option<String>,
    pub source: PackageSource,
}

#[derive(Debug)]
pub struct Comparison {
    pub only_current: Vec<Package>,
    pub only_other: Vec<Package>,
    pub common: Vec<String>,
    pub version_differences: Vec<VersionDiff>,
}

#[derive(Debug)]
pub struct VersionDiff {
    pub package: String,
    pub current_version: Option<String>,
    pub other_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub package: String,
    pub action: LogAction,
    pub source: PackageSource,
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub enum LogAction {
    Install,
    Remove,
    Update,
    Use,
    Scan,
    Error,
    Warning,
    Info,
}

impl fmt::Display for LogAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LogAction::Install => write!(f, "INSTALL"),
            LogAction::Remove => write!(f, "REMOVE"),
            LogAction::Update => write!(f, "UPDATE"),
            LogAction::Use => write!(f, "USE"),
            LogAction::Scan => write!(f, "SCAN"),
            LogAction::Error => write!(f, "ERROR"),
            LogAction::Warning => write!(f, "WARNING"),
            LogAction::Info => write!(f, "INFO"),
        }
    }
}

#[derive(Debug)]
pub struct Stats {
    pub total_packages: usize,
    pub used_packages: usize,
    pub total_size: u64,
    pub by_source: Vec<(String, usize, u64)>,
    pub largest_packages: Vec<Package>,
    pub oldest_packages: Vec<Package>,
    pub newest_packages: Vec<Package>,
    pub average_package_size: u64,
    pub total_log_entries: usize,
    pub total_install_events: usize,
    pub total_remove_events: usize,
}

#[derive(Debug)]
pub struct VerificationResult {
    pub verified: Vec<String>,
    pub missing: Vec<String>,
    pub corrupted: Vec<String>,
    pub size_mismatch: Vec<(String, u64, u64)>,
    pub total_issues: usize,
    pub fixable: usize,
}
