use serde::{Deserialize, Serialize};
use std::{
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub log_file: PathBuf,
    pub db_file: PathBuf,
    pub cache_dir: PathBuf,
    pub scan_dirs: Vec<PathBuf>,
    pub exclude_patterns: Vec<String>,
    pub auto_scan: bool,
    pub scan_interval: u64,
    pub max_log_size: u64,
    pub log_level: String,
    pub dependency_depth: usize,
    pub protect_core: bool,
    pub backup_before_remove: bool,
    pub parallel_scans: usize,
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