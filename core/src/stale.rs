use crate::model::Entry;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const PROJECT_MARKERS: &[&str] = &[
    "package.json", "Cargo.toml", "go.mod", "pom.xml",
    "build.gradle", "build.gradle.kts", "composer.json",
    "pyproject.toml", "setup.py", "requirements.txt",
];

#[derive(Debug, Serialize)]
pub struct StaleProject {
    pub root: PathBuf,
    pub days_since_source_change: u64,
    pub artifacts: Vec<PathBuf>,
    pub total_artifact_bytes: u64,
}

fn is_project_root(dir: &Path) -> bool {
    if PROJECT_MARKERS.iter().any(|m| dir.join(m).exists()) { return true; }
    std::fs::read_dir(dir).ok().map(|e| e.flatten()
        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("csproj")))
        .unwrap_or(false)
}

fn latest_source_mtime(dir: &Path, artifact_paths: &[PathBuf]) -> Option<SystemTime> {
    let skip: std::collections::HashSet<&Path> = artifact_paths.iter().map(|p| p.as_path()).collect();
    let mut latest: Option<SystemTime> = None;
    let mut it = walkdir::WalkDir::new(dir).follow_links(false).into_iter();
    while let Some(Ok(e)) = it.next() {
        if skip.contains(e.path()) { it.skip_current_dir(); continue; }
        if e.file_type().is_dir() && e.file_name().to_str().map(|n| n.starts_with('.')).unwrap_or(false) {
            it.skip_current_dir(); continue;
        }
        if e.file_type().is_file() {
            if let Ok(mtime) = e.metadata().and_then(|m| m.modified()) {
                latest = Some(latest.map_or(mtime, |c: SystemTime| c.max(mtime)));
            }
        }
    }
    latest
}

pub fn find_stale_projects(entries: &[Entry], stale_days: u64) -> Vec<StaleProject> {
    use std::collections::HashMap;
    let threshold = Duration::from_secs(stale_days * 24 * 60 * 60);
    let now = SystemTime::now();
    let mut by_root: HashMap<PathBuf, Vec<&Entry>> = HashMap::new();
    for e in entries { by_root.entry(e.path.parent().unwrap_or(&e.path).to_path_buf()).or_default().push(e); }
    let mut stale = Vec::new();
    for (root, artifacts) in &by_root {
        if !is_project_root(root) { continue; }
        let artifact_paths: Vec<PathBuf> = artifacts.iter().map(|e| e.path.clone()).collect();
        let Some(mtime) = latest_source_mtime(root, &artifact_paths) else { continue };
        let age = now.duration_since(mtime).unwrap_or(Duration::ZERO);
        if age < threshold { continue; }
        stale.push(StaleProject {
            root: root.clone(),
            days_since_source_change: age.as_secs() / 86_400,
            artifacts: artifact_paths,
            total_artifact_bytes: artifacts.iter().map(|e| e.size_bytes).sum(),
        });
    }
    stale.sort_by(|a, b| b.total_artifact_bytes.cmp(&a.total_artifact_bytes));
    stale
}
