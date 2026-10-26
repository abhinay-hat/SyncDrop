//! SyncDrop CLI: plan, sync, and cleanup using the same config as the desktop app.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use clap::{Parser, Subcommand};
use syncdrop_core::cleanup;
use syncdrop_core::config::ConfigStore;
use syncdrop_core::engine::{self, Plan};
use syncdrop_core::profile::SyncProfile;

#[derive(Parser)]
#[command(name = "syncdrop-cli", about = "Plan and run SyncDrop backups from the terminal")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Print the sync plan for a profile (no writes).
    Plan {
        #[arg(long)]
        profile: String,
    },
    /// Run a sync. Without --yes, large plans print and exit 2 for confirmation.
    Sync {
        #[arg(long)]
        profile: String,
        /// Skip the size/file confirmation threshold.
        #[arg(long)]
        yes: bool,
    },
    /// Find (and optionally delete) dependency/cache folders on the destination.
    Cleanup {
        #[arg(long)]
        profile: String,
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let cli = Cli::parse();
    let store = ConfigStore::new().map_err(|e| e.to_string())?;
    match cli.command {
        Commands::Plan { profile } => {
            let (profile, dest) = resolve(&store, &profile)?;
            let plan = engine::dry_run(&profile, &dest).map_err(|e| e.to_string())?;
            print_plan(&plan);
            Ok(ExitCode::SUCCESS)
        }
        Commands::Sync { profile, yes } => {
            let (profile, dest) = resolve(&store, &profile)?;
            let plan = engine::dry_run(&profile, &dest).map_err(|e| e.to_string())?;
            print_plan(&plan);
            if !yes && plan.needs_confirmation(&profile) {
                eprintln!(
                    "plan exceeds confirmation threshold ({} bytes or {} files); re-run with --yes",
                    profile.confirm_bytes_threshold, profile.confirm_files_threshold
                );
                return Ok(ExitCode::from(2));
            }
            if plan.copies.is_empty() && plan.updates.is_empty() && plan.deletes.is_empty() {
                println!("nothing to do");
                return Ok(ExitCode::SUCCESS);
            }
            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            let cancel = AtomicBool::new(false);
            let record = engine::sync(
                &profile,
                &dest,
                |done, total, bytes_done, bytes_total, path| {
                    eprintln!(
                        "[{done}/{total}] {path} ({bytes_done}/{bytes_total} bytes)"
                    );
                },
                &cancel,
            )
            .map_err(|e| e.to_string())?;
            println!(
                "done: {} files, {} bytes in {:.1}s",
                record.file_count, record.total_bytes, record.duration_seconds
            );
            Ok(ExitCode::SUCCESS)
        }
        Commands::Cleanup { profile, dry_run } => {
            let (profile, dest) = resolve(&store, &profile)?;
            let plan = cleanup::plan_cleanup(&dest).map_err(|e| e.to_string())?;
            println!(
                "cleanup targets: {} ({} bytes)",
                plan.targets.len(),
                plan.total_bytes
            );
            for t in &plan.targets {
                println!("  {} ({} bytes)", t.relative_path, t.bytes);
            }
            if dry_run {
                println!("dry-run: nothing deleted");
                return Ok(ExitCode::SUCCESS);
            }
            if plan.targets.is_empty() {
                return Ok(ExitCode::SUCCESS);
            }
            cleanup::apply_cleanup(&dest, &plan).map_err(|e| e.to_string())?;
            println!("deleted {} folders under {}", plan.targets.len(), dest.display());
            let _ = profile;
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn resolve(store: &ConfigStore, name: &str) -> Result<(SyncProfile, PathBuf), String> {
    let profile = store
        .profiles()
        .iter()
        .find(|p| p.name.eq_ignore_ascii_case(name) || p.id.to_string() == name)
        .cloned()
        .ok_or_else(|| format!("profile not found: {name}"))?;
    if let Err(issues) = profile.validate() {
        return Err(issues
            .into_iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("; "));
    }
    let dest = PathBuf::from(profile.expanded_dest_path());
    Ok((profile, dest))
}

fn print_plan(plan: &Plan) {
    println!("{}", plan.summary_line());
    for rel in &plan.copies {
        println!("  copy   {rel}");
    }
    for rel in &plan.updates {
        println!("  update {rel}");
    }
    for rel in &plan.deletes {
        println!("  delete {rel}");
    }
}
