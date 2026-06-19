mod analyzer;
mod cache;
mod cmd;
mod config;
mod logger;
mod models;
mod scanner;
mod tracker;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::process;

use crate::{analyzer::Analyzer, config::Config, tracker::Tracker};

#[derive(Parser)]
#[command(name = "pkgtrace")]
#[command(about = "Advanced package tracker for Termux with dependency resolution")]
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    List {
        #[arg(short, long)]
        sizes: bool,
        #[arg(short = 'S', long)]
        source: Option<String>,
        #[arg(short, long)]
        min_size: Option<u64>,
        #[arg(short, long)]
        used: bool,
    },
    Unused {
        #[arg(default_value_t = 30)]
        days: u32,
        #[arg(short, long)]
        explain: bool,
        #[arg(short, long)]
        deps: bool,
        #[arg(short, long)]
        min_size: Option<u64>,
        #[arg(short, long)]
        remove: bool,
        #[arg(short, long)]
        dry_run: bool,
    },
    Deps {
        package: String,
        #[arg(short, long)]
        reverse: bool,
        #[arg(short, long)]
        tree: bool,
        #[arg(short, long)]
        depth: Option<usize>,
    },
    Info {
        package: String,
        #[arg(short, long)]
        verbose: bool,
    },
    Scan {
        #[arg(short, long)]
        force: bool,
        #[arg(short, long)]
        background: bool,
    },
    Clean {
        #[arg(default_value_t = 30)]
        days: u32,
        #[arg(short, long)]
        yes: bool,
        #[arg(short, long)]
        min_size: Option<u64>,
        #[arg(short, long)]
        dry_run: bool,
    },
    Export {
        #[arg(short, long, default_value = "json")]
        format: String,
        #[arg(short, long)]
        output: Option<String>,
        #[arg(short, long)]
        include_deps: bool,
    },
    Import {
        file: String,
        #[arg(short, long)]
        dry_run: bool,
        #[arg(short, long)]
        force: bool,
    },
    Analyze {
        #[arg(default_value_t = 30)]
        days: u32,
        #[arg(short, long)]
        output: Option<String>,
    },
    Graph {
        package: String,
        #[arg(short, long)]
        format: Option<String>,
        #[arg(short, long)]
        output: Option<String>,
    },
    SafeRemove {
        #[arg(default_value_t = 30)]
        days: u32,
        #[arg(short, long)]
        yes: bool,
        #[arg(short, long)]
        dry_run: bool,
    },
    Stats,
    Monitor {
        #[arg(short, long)]
        daemon: bool,
        #[arg(short, long)]
        interval: Option<u64>,
    },
    Verify {
        #[arg(short, long)]
        fix: bool,
    },
    Search {
        query: String,
        #[arg(short, long)]
        source: Option<String>,
    },
    Autoremove {
        #[arg(short, long)]
        yes: bool,
        #[arg(short, long)]
        dry_run: bool,
    },
    Compare {
        #[arg(short, long)]
        file: String,
        #[arg(short, long)]
        output: Option<String>,
    },
    CacheStats,
    RebuildCache,
    RebuildFileMap,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load_or_create()?;

    let tracker = Tracker::new(config)?;
    let analyzer = Analyzer::new(tracker.clone());

    let result = match &cli.command {
        Commands::List {
            sizes,
            source,
            min_size,
            used,
        } => cmd::cmd_list(&tracker, *sizes, source.as_deref(), *min_size, *used),
        Commands::Unused {
            days,
            explain,
            deps,
            min_size,
            remove,
            dry_run,
        } => cmd::cmd_unused(
            &tracker, *days, *explain, *deps, *min_size, *remove, *dry_run,
        ),
        Commands::Deps {
            package,
            reverse,
            tree,
            depth,
        } => cmd::cmd_deps(&tracker, package, *reverse, *tree, *depth),
        Commands::Info { package, verbose } => cmd::cmd_info(&tracker, package, *verbose),
        Commands::Scan { force, background } => cmd::cmd_scan(&tracker, *force, *background),
        Commands::Clean {
            days,
            yes,
            min_size,
            dry_run,
        } => cmd::cmd_clean(&tracker, *days, *yes, *min_size, *dry_run),
        Commands::Export {
            format,
            output,
            include_deps,
        } => cmd::cmd_export(&tracker, format, output.as_deref(), *include_deps),
        Commands::Import {
            file,
            dry_run,
            force,
        } => cmd::cmd_import(&tracker, file, *dry_run, *force),
        Commands::Analyze { days, output } => cmd::cmd_analyze(&analyzer, *days, output.as_deref()),
        Commands::Graph {
            package,
            format,
            output,
        } => cmd::cmd_graph(&analyzer, package, format.as_deref(), output.as_deref()),
        Commands::SafeRemove { days, yes, dry_run } => {
            cmd::cmd_safe_remove(&analyzer, *days, *yes, *dry_run)
        }
        Commands::Stats => cmd::cmd_stats(&tracker),
        Commands::Monitor { daemon, interval } => cmd::cmd_monitor(&tracker, *daemon, *interval),
        Commands::Verify { fix } => cmd::cmd_verify(&tracker, *fix),
        Commands::Search { query, source } => cmd::cmd_search(&tracker, query, source.as_deref()),
        Commands::Autoremove { yes, dry_run } => cmd::cmd_autoremove(&tracker, *yes, *dry_run),
        Commands::Compare { file, output } => cmd::cmd_compare(&tracker, file, output.as_deref()),
        Commands::CacheStats => cmd::cmd_cache_stats(&tracker),
        Commands::RebuildCache => cmd::cmd_rebuild_cache(&tracker),
        Commands::RebuildFileMap => cmd::cmd_rebuild_file_map(&tracker),
    };

    if let Err(e) = result {
        eprintln!("{} {}", "Error:".bold(), e);
        process::exit(1);
    }

    Ok(())
}
