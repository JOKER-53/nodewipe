/// `nodewipe doctor` analysis engine.
///
/// Produces a `DoctorReport` — a per-ecosystem breakdown of all discovered
/// artifacts, sorted by total size, with the oldest untouched artifact
/// highlighted per ecosystem. The CLI formats this into the human-readable
/// summary; the `--json` flag emits it raw.
use crate::model::{ArtifactKind, Entry};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::SystemTime;

// ── public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EcosystemReport {
    /// Artifact kind slug, e.g. "rust_target".
    pub kind: String,
    /// Human label, e.g. "Rust target/".
    pub label: String,
    /// Number of artifact directories found.
    pub count: usize,
    /// Total bytes across all found directories.
    pub total_bytes: u64,
    /// Age in days of the *oldest* (least-recently-touched) artifact.
    pub oldest_days: Option<u64>,
    /// Path of the oldest artifact (for display).
    pub oldest_path: Option<String>,
    /// Paths of the top N biggest artifacts, for context.
    pub biggest: Vec<BigArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigArtifact {
    pub path: String,
    pub size_bytes: u64,
    /// Age in days since last modification.
    pub age_days: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoctorReport {
    /// Grand total bytes across all ecosystems.
    pub total_bytes: u64,
    /// Total number of artifact dirs found.
    pub total_count: usize,
    /// Per-ecosystem breakdown, sorted by total_bytes descending.
    pub ecosystems: Vec<EcosystemReport>,
    /// Timestamp when this report was generated (Unix secs).
    pub generated_at: u64,
}

// ── main entry point ──────────────────────────────────────────────────────────

/// Build a full doctor report from a flat list of scan entries.
pub fn build_report(entries: &[Entry]) -> DoctorReport {
    let now = SystemTime::now();

    let generated_at = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Group entries by kind.
    let mut groups: HashMap<ArtifactKind, Vec<&Entry>> = HashMap::new();
    for e in entries {
        groups.entry(e.kind).or_default().push(e);
    }

    let mut ecosystems: Vec<EcosystemReport> = groups
        .iter()
        .map(|(kind, group_entries)| {
            let total_bytes: u64 = group_entries.iter().map(|e| e.size_bytes).sum();
            let count = group_entries.len();

            // Oldest = smallest last_modified timestamp (furthest in the past).
            let (oldest_days, oldest_path) = group_entries
                .iter()
                .filter_map(|e| {
                    e.last_modified.and_then(|mtime| {
                        now.duration_since(mtime)
                            .ok()
                            .map(|age| (age.as_secs() / 86400, e.path.display().to_string()))
                    })
                })
                .max_by_key(|(days, _)| *days)
                .map(|(days, path)| (Some(days), Some(path)))
                .unwrap_or((None, None));

            // Top 3 biggest for context.
            let mut sorted = group_entries.to_vec();
            sorted.sort_by_key(|b| std::cmp::Reverse(b.size_bytes));
            let biggest = sorted
                .iter()
                .take(3)
                .map(|e| {
                    let age_days = e.last_modified.and_then(|mtime| {
                        now.duration_since(mtime)
                            .ok()
                            .map(|age| age.as_secs() / 86400)
                    });
                    BigArtifact {
                        path: e.path.display().to_string(),
                        size_bytes: e.size_bytes,
                        age_days,
                    }
                })
                .collect();

            EcosystemReport {
                kind: kind.slug().to_string(),
                label: kind.label().to_string(),
                count,
                total_bytes,
                oldest_days,
                oldest_path,
                biggest,
            }
        })
        .collect();

    // Biggest ecosystem first.
    ecosystems.sort_by_key(|b| std::cmp::Reverse(b.total_bytes));

    let total_bytes = ecosystems.iter().map(|e| e.total_bytes).sum();
    let total_count = ecosystems.iter().map(|e| e.count).sum();

    DoctorReport {
        total_bytes,
        total_count,
        ecosystems,
        generated_at,
    }
}
