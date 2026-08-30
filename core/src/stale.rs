/// Stale project detection.
///
/// A project is considered "likely abandoned" when its artifact directory
/// (`node_modules`, `target`, `venv`, …) exists but the *source files* in
/// the same project root haven't been touched in `threshold` days.
///
/// We look at source files only — not the artifact dir itself, because those
/// timestamps are often bumped by the toolchain, not by the developer.
use crate::model::Entry;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

// ── public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct StaleProject {
    /// The project root (parent of the artifact directory).
    pub root: PathBuf,
    /// The artifact entry that flagged this project.
    pub artifact: Entry,
    /// How many days since *any* source file was modified.
    pub stale_days: u64,
    /// Bytes that could be freed by deleting the artifact.
    pub reclaimable_bytes: u64,
}

// ── main entry point ──────────────────────────────────────────────────────────

/// Given a flat list of scanned entries, return those whose project root looks
/// abandoned (no source file touched in `threshold`).
///
/// An entry without a determinable project root, or without a parent
/// directory we can walk, is skipped rather than falsely flagged.
pub fn find_stale_projects(entries: &[Entry], threshold: Duration) -> Vec<StaleProject> {
    let now = SystemTime::now();
    let mut stale = Vec::new();

    for entry in entries {
        let root = match entry.path.parent() {
            Some(p) => p.to_path_buf(),
            None => continue,
        };

        let youngest_source = youngest_source_file_mtime(&root, &entry.path);

        let age = match youngest_source {
            Some(mtime) => match now.duration_since(mtime) {
                Ok(d) => d,
                Err(_) => continue, // clock skew — skip
            },
            None => continue, // couldn't read any source files — skip
        };

        if age >= threshold {
            stale.push(StaleProject {
                root: root.clone(),
                artifact: entry.clone(),
                stale_days: age.as_secs() / 86400,
                reclaimable_bytes: entry.size_bytes,
            });
        }
    }

    // Most-stale first.
    stale.sort_by(|a, b| b.stale_days.cmp(&a.stale_days));
    stale
}

// ── internals ─────────────────────────────────────────────────────────────────

/// Returns the most-recently-modified source file timestamp under `root`,
/// excluding the artifact directory `artifact_path` itself.
///
/// "Source files" = anything that isn't a known artifact directory.
/// We cap the walk at depth 3 to stay fast on large projects; config files
/// and source files are almost always at the root or one level deep.
fn youngest_source_file_mtime(root: &Path, artifact_path: &Path) -> Option<SystemTime> {
    use std::fs;

    let mut youngest: Option<SystemTime> = None;

    // We do a shallow manual walk (not walkdir) to avoid pulling in the full
    // subtree and to be able to skip artifact directories cleanly.
    fn walk(
        dir: &Path,
        artifact: &Path,
        depth: usize,
        youngest: &mut Option<SystemTime>,
    ) {
        if depth == 0 {
            return;
        }
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();

            // Never recurse into the artifact directory we're evaluating.
            if path == artifact {
                continue;
            }

            // Skip other well-known artifact dirs to avoid their timestamps
            // masquerading as source activity.
            if is_known_artifact_dir(&path) {
                continue;
            }

            let meta = match entry.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };

            // Skip version control internals like `.git` — their mtimes would
            // dominate the signal. Do NOT skip hidden files (like `.env`) or
            // other hidden source dirs (like `.github` or `.vscode`).
            let is_hidden = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false);

            if is_hidden && meta.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == ".git" || name == ".svn" || name == ".hg" {
                    continue;
                }
            }

            if meta.is_dir() {
                walk(&path, artifact, depth - 1, youngest);
            } else if meta.is_file() {
                if let Ok(mtime) = meta.modified() {
                    *youngest = Some(match youngest {
                        Some(prev) if mtime > *prev => mtime,
                        Some(prev) => *prev,
                        None => mtime,
                    });
                }
            }
        }
    }

    walk(root, artifact_path, 3, &mut youngest);
    youngest
}

/// Quick check: is this directory one of our known artifact kinds?
/// Used to skip them during source-file walks so their mtimes don't pollute
/// the "last touched by the developer" signal.
fn is_known_artifact_dir(path: &Path) -> bool {
    let name = match path.file_name().and_then(|n| n.to_str()) {
        Some(n) => n,
        None => return false,
    };
    matches!(
        name,
        "node_modules"
            | "venv"
            | ".venv"
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | ".ruff_cache"
            | "target"
            | "build"
            | ".next"
            | ".turbo"
            | "dist"
            | "vendor"
            | "bin"
            | "obj"
            | "go-build"
            | "mod"
    )
}
