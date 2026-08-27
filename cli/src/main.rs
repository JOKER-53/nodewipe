mod cmd_doctor;
mod cmd_stats;
mod tui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use nodewipe_core::{
    annotate_workspace_roots, delete, find_stale_projects, group_by_workspace,
    history_append, load_config, load_ignore_patterns, restore, scan,
    ArtifactKind, DeleteMode, Entry, HistoryEntry, ScanOptions,
};
use nodewipe_core::history::{now_unix, format_date};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

const BANNER: &str = r#"
 _   _           _    __        ___            
| \ | | ___   __| | __\ \      / (_)_ __   ___ 
|  \| |/ _ \ / _` |/ _ \ \ /\ / /| | '_ \ / _ \
| |\  | (_) | (_| |  __/\ V  V / | | |_) |  __/
|_| \_|\___/ \__,_|\___| \_/\_/  |_| .__/ \___|
                                   |_|         
"#;

const TYPE_HELP: &str = "node_modules, venv, pycache, pytest_cache, mypy_cache, \
ruff_cache, rust_target, maven_target, gradle_build, next_cache, turbo_cache, dist";

#[derive(Parser)]
#[command(
    name = "nodewipe",
    version,
    about = "Find and reclaim disk space from stray dev-dependency and build-artifact directories",
    before_help = BANNER
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,

    #[arg(long, global = true, default_value = ".")]
    root: PathBuf,

    #[arg(long, global = true)]
    json: bool,

    #[arg(long, global = true, value_delimiter = ',')]
    exclude_types: Vec<String>,

    #[arg(long, global = true)]
    no_ignore_file: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Scan for disposable artifact directories (default if no subcommand given).
    Scan {
        /// Directory to scan, e.g. `nodewipe scan ~`
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,

        /// Only show entries at least this many megabytes in size.
        #[arg(long, default_value_t = 0)]
        min_mb: u64,

        /// Group output by monorepo/workspace root instead of a flat list.
        #[arg(long)]
        grouped: bool,

        /// Only show artifacts whose last modification is older than this,
        /// e.g. `30d`, `2w`, `6m`, `1y`.
        #[arg(long)]
        older_than: Option<String>,

        /// Primary sort order: `size` (largest first, default) or `age`
        /// (oldest-last-touched first, surfaces abandoned projects).
        #[arg(long, value_enum, default_value_t = SortKey::Size)]
        sort: SortKey,

        /// Flag projects as likely-abandoned when source files haven't been
        /// touched in this many days. Prints a stale-project summary below
        /// the normal scan output.
        #[arg(long)]
        stale_days: Option<u64>,
    },
    /// Delete one or more artifact directories.
    Delete {
        paths: Vec<PathBuf>,

        #[arg(long, value_enum)]
        mode: Option<DeleteModeArg>,

        #[arg(long)]
        yes: bool,

        #[arg(long)]
        dry_run: bool,
    },
    /// Restore a `.tar.gz` backup created by `delete --mode archive`.
    Restore {
        archive: PathBuf,
    },
    /// List every supported artifact type and its slug (for --exclude-types).
    Types,
    /// Launch the desktop GUI.
    Gui,
    /// Scan your system and show a breakdown of disk usage by ecosystem:
    /// how many Rust target/ dirs, Python venvs, etc., and which is oldest.
    Doctor {
        /// Directory to scan, e.g. `nodewipe doctor ~`
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },
    /// Show a dashboard of how much space you have freed over time.
    /// Reads ~/.local/share/nodewipe/history.json.
    Stats {
        /// How many past days to include in the bar chart.
        #[arg(long, default_value_t = 30)]
        days: u64,

        /// Directory context (unused by stats, accepted for consistency).
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum DeleteModeArg {
    Trash,
    Archive,
    Permanent,
}

impl From<DeleteModeArg> for DeleteMode {
    fn from(v: DeleteModeArg) -> Self {
        match v {
            DeleteModeArg::Trash => DeleteMode::Trash,
            DeleteModeArg::Archive => DeleteMode::Archive,
            DeleteModeArg::Permanent => DeleteMode::Permanent,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum SortKey {
    /// Sort by size, largest first (default).
    Size,
    /// Sort by last-modified date, oldest first.
    Age,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let config = load_config();
    let root = cli.path.clone().unwrap_or_else(|| cli.root.clone());

    let mut exclude_kinds = parse_exclude_types(&cli.exclude_types)?;
    if let Some(defaults) = &config.default_exclude_types {
        exclude_kinds.extend(parse_exclude_types(defaults)?);
    }
    exclude_kinds.sort_by_key(|k| k.slug());
    exclude_kinds.dedup_by_key(|k| k.slug());

    let ignore_patterns = if cli.no_ignore_file { Vec::new() } else { load_ignore_patterns(&root) };

    match cli.command {
        None => {
            if cli.json || !atty_stdout() {
                cmd_scan(&root, cli.json, 0, false, exclude_kinds, ignore_patterns, None, SortKey::Size, None)
            } else {
                tui::run(root)?;
                Ok(ExitCode::SUCCESS)
            }
        }
        Some(Command::Scan { path: sub_path, min_mb, grouped, older_than, sort, stale_days }) => {
            let scan_root = sub_path.unwrap_or(root);
            cmd_scan(&scan_root, cli.json, min_mb, grouped, exclude_kinds, ignore_patterns, older_than, sort, stale_days)
        }
        Some(Command::Delete { paths, mode, yes, dry_run }) => {
            let resolved_mode = resolve_delete_mode(mode, &config);
            cmd_delete(paths, resolved_mode, yes, dry_run, cli.json)
        }
        Some(Command::Restore { archive }) => cmd_restore(&archive, cli.json),
        Some(Command::Types) => cmd_types(),
        Some(Command::Gui) => cmd_gui(),
        Some(Command::Doctor { path: sub_path }) => {
            let doctor_root = sub_path.unwrap_or(root);
            cmd_doctor::run(doctor_root, cli.json, exclude_kinds, ignore_patterns)
        }
        Some(Command::Stats { days, path: _ }) => {
            cmd_stats::run(cli.json, days)
        }
    }
}

fn resolve_delete_mode(cli_mode: Option<DeleteModeArg>, config: &nodewipe_core::Config) -> DeleteMode {
    if let Some(m) = cli_mode {
        return m.into();
    }
    match config.default_delete_mode.as_deref() {
        Some("archive") => DeleteMode::Archive,
        Some("permanent") => DeleteMode::Permanent,
        Some("trash") | None => DeleteMode::Trash,
        Some(other) => {
            eprintln!("warning: unknown default_delete_mode '{other}' in config, using trash");
            DeleteMode::Trash
        }
    }
}

fn parse_exclude_types(raw: &[String]) -> Result<Vec<ArtifactKind>> {
    raw.iter()
        .map(|s| {
            ArtifactKind::from_slug(s.trim())
                .ok_or_else(|| anyhow::anyhow!("unknown artifact type '{s}'. Valid types: {TYPE_HELP}"))
        })
        .collect()
}

fn parse_duration(s: &str) -> Result<Duration> {
    let s = s.trim();
    let (num_part, unit) = match s.chars().last() {
        Some(c) if c.is_ascii_alphabetic() => (&s[..s.len() - 1], c.to_ascii_lowercase()),
        _ => (s, 'd'),
    };
    let n: u64 = num_part
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid duration '{s}', expected e.g. 30d, 2w, 6m, 1y"))?;

    let days = match unit {
        'd' => n,
        'w' => n * 7,
        'm' => n * 30,
        'y' => n * 365,
        _ => anyhow::bail!("invalid duration unit in '{s}', expected d/w/m/y"),
    };
    Ok(Duration::from_secs(days * 24 * 60 * 60))
}

fn atty_stdout() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

fn cmd_types() -> Result<ExitCode> {
    println!("Supported artifact types:\n");
    for kind in ArtifactKind::ALL {
        println!("  {:<16} {}", kind.slug(), kind.label());
        println!("  {:<16} {}", "", kind.description());
        println!();
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_scan(
    root: &PathBuf,
    json: bool,
    min_mb: u64,
    grouped: bool,
    exclude_kinds: Vec<ArtifactKind>,
    ignore_patterns: Vec<String>,
    older_than: Option<String>,
    sort: SortKey,
    stale_days: Option<u64>,
) -> Result<ExitCode> {
    let opts = ScanOptions {
        root: root.clone(),
        exclude_kinds,
        ignore_patterns,
        ..Default::default()
    };

    let mut entries: Vec<Entry> = scan(&opts)?;
    annotate_workspace_roots(&mut entries, &opts.root);

    let min_bytes = min_mb * 1024 * 1024;
    entries.retain(|e| e.size_bytes >= min_bytes);

    if let Some(older_than) = older_than {
        let threshold = parse_duration(&older_than)?;
        let now = SystemTime::now();
        entries.retain(|e| match e.last_modified {
            Some(t) => now.duration_since(t).map(|age| age >= threshold).unwrap_or(false),
            None => false,
        });
    }

    match sort {
        SortKey::Size => {
            entries.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
        }
        SortKey::Age => {
            entries.sort_by(|a, b| match (a.last_modified, b.last_modified) {
                (Some(ta), Some(tb)) => ta.cmp(&tb),
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, None) => std::cmp::Ordering::Equal,
            });
        }
    }

    let stale = if let Some(days) = stale_days {
        let threshold = Duration::from_secs(days * 86400);
        find_stale_projects(&entries, threshold)
    } else {
        Vec::new()
    };

    if grouped {
        let groups = group_by_workspace(entries);
        if json {
            println!("{}", serde_json::to_string_pretty(&groups.into_values().collect::<Vec<_>>())?);
        } else {
            for group in groups.values() {
                println!("\n{} ({})", group.root.display(), human_size(group.total_size_bytes));
                for e in &group.entries {
                    println!("  {}  {}  {}", human_size(e.size_bytes), kind_display(e), e.path.display());
                }
            }
        }
    } else if json {
        let out = serde_json::json!({
            "entries": entries,
            "stale_projects": stale,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        let total: u64 = entries.iter().map(|e| e.size_bytes).sum();
        let now = SystemTime::now();
        for e in &entries {
            let age_str = match e.last_modified {
                Some(mtime) => {
                    let days = now.duration_since(mtime)
                        .map(|d| d.as_secs() / 86400)
                        .unwrap_or(0);
                    format!("{:>4}d", days)
                }
                None => "   ?d".to_string(),
            };
            println!("{}  {}  {}  {}", human_size(e.size_bytes), age_str, kind_display(e), e.path.display());
        }
        println!("\n{} artifacts found, {} reclaimable", entries.len(), human_size(total));

        if !stale.is_empty() {
            println!(
                "\n ⚠  {} likely-abandoned project{} (source untouched for {}+ days):\n",
                stale.len(),
                if stale.len() == 1 { "" } else { "s" },
                stale_days.unwrap_or(90),
            );
            for s in &stale {
                println!("   {} days   {}   {}",
                    s.stale_days,
                    human_size(s.reclaimable_bytes),
                    s.root.display(),
                );
            }
            println!();
        }
    }

    Ok(ExitCode::SUCCESS)
}

fn cmd_delete(
    paths: Vec<PathBuf>,
    mode: DeleteMode,
    yes: bool,
    dry_run: bool,
    json: bool,
) -> Result<ExitCode> {
    if paths.is_empty() {
        eprintln!("no paths given");
        return Ok(ExitCode::FAILURE);
    }

    print_risk_warnings(&paths);

    if dry_run {
        for p in &paths {
            println!("would delete ({mode:?}): {}", p.display());
        }
        return Ok(ExitCode::SUCCESS);
    }

    if !yes {
        eprintln!("refusing to delete without --yes (or use --dry-run to preview)");
        return Ok(ExitCode::FAILURE);
    }

    let mut results = Vec::new();
    let mut had_error = false;

    for p in &paths {
        let size = dir_size_quick(p);
        let kind_slug = nodewipe_core::classify_path(p)
            .map(|k| k.slug().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        match delete(p, mode, size) {
            Ok(r) => {
                let ts = now_unix();
                let _ = history_append(HistoryEntry {
                    deleted_at: ts,
                    date: format_date(ts),
                    path: p.display().to_string(),
                    kind: kind_slug,
                    size_bytes: size,
                    mode: format!("{mode:?}").to_lowercase(),
                });
                results.push(r);
            }
            Err(e) => {
                had_error = true;
                eprintln!("failed to delete {}: {e:#}", p.display());
            }
        }
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        for r in &results {
            print!("deleted {} ({:?})", r.path.display(), r.mode);
            if let Some(a) = &r.archive_path {
                print!(" -> archived at {}", a.display());
            }
            println!();
        }
    }

    Ok(if had_error { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn cmd_restore(archive: &PathBuf, json: bool) -> Result<ExitCode> {
    match restore(archive) {
        Ok(restored_path) => {
            if json {
                println!("{}", serde_json::json!({ "restored": restored_path }));
            } else {
                println!("Restored: {}", restored_path.display());
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            eprintln!("failed to restore {}: {e:#}", archive.display());
            Ok(ExitCode::FAILURE)
        }
    }
}

fn print_risk_warnings(paths: &[PathBuf]) {
    for p in paths {
        if let Some(kind) = nodewipe_core::classify_path(p) {
            if let Some(note) = kind.risk_note() {
                eprintln!("⚠ WARNING [{}] {}\n  {}\n", kind.label(), p.display(), note);
            }
        }
    }
}

fn cmd_gui() -> Result<ExitCode> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf()));

    if let Some(dir) = &exe_dir {
        let gui_bin_name = if cfg!(windows) { "nodewipe-gui.exe" } else { "nodewipe-gui" };
        let candidates = [dir.join(gui_bin_name), dir.join("nodewipe-gui.AppImage")];

        for candidate in candidates {
            if candidate.exists() {
                println!("Launching nodewipe GUI...");
                std::process::Command::new(&candidate)
                    .spawn()
                    .with_context(|| format!("failed to launch {}", candidate.display()))?;
                return Ok(ExitCode::SUCCESS);
            }
        }
    }

    if cfg!(target_os = "macos") {
        let launched = std::process::Command::new("open").arg("-a").arg("nodewipe").status();
        if matches!(launched, Ok(status) if status.success()) {
            return Ok(ExitCode::SUCCESS);
        }
    }

    eprintln!("nodewipe GUI isn't installed alongside this CLI.");
    eprintln!();
    eprintln!("Install both with:");
    eprintln!("  curl -fsSL https://raw.githubusercontent.com/KADHIRAVANEG/nodewipe/main/scripts/install.sh | bash");
    eprintln!("  (choose option 2: CLI + GUI)");
    eprintln!();
    eprintln!("Or from source:");
    eprintln!("  cargo build --release --workspace   # builds nodewipe-gui alongside this CLI");
    Ok(ExitCode::FAILURE)
}

fn dir_size_quick(path: &PathBuf) -> u64 {
    walkdir_size(path)
}

fn walkdir_size(path: &PathBuf) -> u64 {
    use std::fs;
    fn walk(p: &std::path::Path, total: &mut u64) {
        if let Ok(entries) = fs::read_dir(p) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Ok(meta) = entry.metadata() {
                    if meta.is_dir() {
                        walk(&path, total);
                    } else {
                        *total += meta.len();
                    }
                }
            }
        }
    }
    let mut total = 0;
    walk(path, &mut total);
    total
}

fn kind_display(e: &Entry) -> String {
    match &e.package_manager {
        Some(pm) => format!("{:<14} ({pm:?})", e.kind.label()),
        None => e.kind.label().to_string(),
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.2} {}", UNITS[unit])
}
