use crate::model::ArtifactKind;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub timestamp: u64,
    pub path: String,
    pub kind: ArtifactKind,
    pub freed_bytes: u64,
    pub mode: String,
}

fn history_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("nodewipe").join("history.json"))
}

pub fn load() -> Vec<HistoryEntry> {
    let Some(path) = history_path() else { return Vec::new() };
    let Ok(content) = std::fs::read_to_string(&path) else { return Vec::new() };
    serde_json::from_str(&content).unwrap_or_default()
}

pub fn append(path: &str, kind: ArtifactKind, freed_bytes: u64, mode: &str) {
    let Some(history_path) = history_path() else { return };
    if let Some(parent) = history_path.parent() { let _ = std::fs::create_dir_all(parent); }
    let mut entries = load();
    entries.push(HistoryEntry {
        timestamp: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        path: path.to_string(), kind, freed_bytes, mode: mode.to_string(),
    });
    if entries.len() > 10_000 { entries.drain(0..entries.len() - 10_000); }
    if let Ok(json) = serde_json::to_string_pretty(&entries) { let _ = std::fs::write(&history_path, json); }
}

#[derive(Debug, Serialize)]
pub struct EcosystemStats {
    pub kind: ArtifactKind,
    pub label: String,
    pub count: usize,
    pub total_freed_bytes: u64,
}

pub fn stats_by_ecosystem(entries: &[HistoryEntry]) -> Vec<EcosystemStats> {
    use std::collections::HashMap;
    let mut map: HashMap<String, (ArtifactKind, usize, u64)> = HashMap::new();
    for e in entries {
        let v = map.entry(e.kind.slug().to_string()).or_insert((e.kind, 0, 0));
        v.1 += 1; v.2 += e.freed_bytes;
    }
    let mut stats: Vec<EcosystemStats> = map.into_values().map(|(kind, count, total_freed_bytes)| {
        EcosystemStats { label: kind.label().to_string(), kind, count, total_freed_bytes }
    }).collect();
    stats.sort_by(|a, b| b.total_freed_bytes.cmp(&a.total_freed_bytes));
    stats
}
