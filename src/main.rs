// 🦀 The first thing this repo is to do is to make someone understand Rust.

// 👀 This repo goes ahead to solve the problem of bloat on Termux.

// 🦀 If you're learning Rust here, please follow the comments that begin with a crab symbol (🦀).

// 👀 If you're contributing, follow EITHER the ones WITHOUT the crab(...), OR the ones with the spy eye (👀). The spy eye is the symbol 'mascot' for this project as it depicts monitoring your Termux app directories for any changes.

// 🦀, 👀 Any comment with 🤔 could just be me thinking out loud.

// 🤔 Interestingly, the main file is the longest file right now, haha. The reason is quite obvious. It should basically contain only main but I overdid it.

// 🤔 Alright, I finally shortened it.

mod tracker;
mod analyzer;
mod scanner;
mod config;
mod models;
mod utils;
mod logger;
mod cache;
mod cmd; // Rust will look for mod.rs
// 🤔 There should be better shorthand for all these plenty mods and uses. I could create a prelude but no need for now.
use clap::{Parser, Subcommand};
use colored::Colorize;
use anyhow::Result;
use std::process;

use crate::{
    config::Config,
    tracker::Tracker,
    analyzer::Analyzer,
};

#[derive(Parser)]
// 👀  There's shorthand for using one command and nesting them. We re-write this soon.
#[command(name = "pkgtrace")]
#[command(about = "Advanced package tracker for Termux")] // I need a long about here.
#[command(version = "0.1.0")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}
// 👀 We need to add documentation comments for the help on clap.
#[derive(Subcommand)]
enum Commands {
    List {
        #[arg(short, long)]
        sizes: bool,
        #[arg(short= 'S', long)] // 👀 I changed to capital S due to RUNTIME ERROR from clap builder in relation to the small s used in the source field below. NEVER DELETE THIS COMMENT and never change the capital S to small s except it no longer works.
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
    // I may change the default from json to something easily figure-out-able by just anyone.
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
}
// 👀 In most of my projects, when the main returns a result and all functions are called from main, they mostly also return a result, lol. I need to find a workaround, maybe just for this one to be unique or let's see.
fn main() -> Result<()> {
    let cli = Cli::parse();
  // 🦀  We load the records first if they exist or create them.
    let config = Config::load_or_create()?;
    
    // 🦀 This is where the whole tracker decision starts. We pass it to cmd multiple times, then analyzer. You'll understand why I'm really pointing out the tracker as you proceed.
    let tracker = Tracker::new(config)?;
    let analyzer = Analyzer::new(tracker.clone());
    // 🤔 I wonder why we always reference the cli. I mean, where else do I ever use it.
 
    let result = match &cli.command {
        Commands::List { sizes, source, min_size, used } => {
            cmd::cmd_list(&tracker, *sizes, source.as_deref(), *min_size, *used)
        }
        Commands::Unused { days, explain, deps, min_size, remove, dry_run } => {
            cmd::cmd_unused(&tracker, *days, *explain, *deps, *min_size, *remove, *dry_run)
        }
        Commands::Deps { package, reverse, tree, depth } => {
            cmd::cmd_deps(&tracker, package, *reverse, *tree, *depth)
        }
        Commands::Info { package, verbose } => {
            cmd::cmd_info(&tracker, package, *verbose)
        }
        Commands::Scan { force, background } => {
            cmd::cmd_scan(&tracker, *force, *background)
        }
        Commands::Clean { days, yes, min_size, dry_run } => {
            cmd::cmd_clean(&tracker, *days, *yes, *min_size, *dry_run)
        }
        Commands::Export { format, output, include_deps } => {
            cmd::cmd_export(&tracker, format, output.as_deref(), *include_deps)
        }
        Commands::Import { file, dry_run, force } => {
            cmd::cmd_import(&tracker, file, *dry_run, *force)
        }
        Commands::Analyze { days, output } => {
            cmd::cmd_analyze(&analyzer, *days, output.as_deref())
        }
        Commands::Graph { package, format, output } => {
            cmd::cmd_graph(&analyzer, package, format.as_deref(), output.as_deref())
        }
        Commands::SafeRemove { days, yes, dry_run } => {
            cmd::cmd_safe_remove(&analyzer, *days, *yes, *dry_run)
        }
        Commands::Stats => cmd::cmd_stats(&tracker),
        Commands::Monitor { daemon, interval } => {
            cmd::cmd_monitor(&tracker, *daemon, *interval)
        }
        Commands::Verify { fix } => cmd::cmd_verify(&tracker, *fix),
        Commands::Search { query, source } => {
            cmd::cmd_search(&tracker, query, source.as_deref())
        }
        Commands::Autoremove { yes, dry_run } => {
            cmd::cmd_autoremove(&tracker, *yes, *dry_run)
        }
        Commands::Compare { file, output } => {
            cmd::cmd_compare(&tracker, file, output.as_deref())
        }
        Commands::CacheStats => cmd::cmd_cache_stats(&tracker),
        Commands::RebuildCache => cmd::cmd_rebuild_cache(&tracker),
    };
    // 🦀 If the match above returns an error, we print the error to the developer and exit main, otherwise we do nothing and of course the other side of the code will run, leading to Ok() to satisfy the main return type contract.
    if let Err(e) = result {
        eprintln!("{} {}", "Error:".bold(), e);
        process::exit(1);
    }
    
    Ok(()) // 🦀 Here's the success satisfaction.
}
// 🤔, 👀 Actually, the main function should end here but problem is, I'm handling all the command output from the match here...

// 🤔,👀 I need to create a different mod and move them there soon. It's a lot in here already.

// 👀 Finally moved them to cmd folder and edited the call in the match arms. Also added the relevant imports there.