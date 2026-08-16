use nodewipe_core::{annotate_workspace_roots, find_stale_projects, load_ignore_patterns, scan, ArtifactKind, ScanOptions};
use std::path::PathBuf;

pub fn run(root: &PathBuf, stale_days: u64, json: bool) -> anyhow::Result<()> {
    let opts = ScanOptions { root: root.clone(), ignore_patterns: load_ignore_patterns(root), ..Default::default() };
    eprintln!("Scanning {}...", root.display());
    let mut entries = scan(&opts)?;
    annotate_workspace_roots(&mut entries, &opts.root);

    if json {
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "root": root.display().to_string(),
            "total_bytes": entries.iter().map(|e| e.size_bytes).sum::<u64>(),
            "by_ecosystem": build_summary(&entries),
            "stale_projects": find_stale_projects(&entries, stale_days),
        }))?);
        return Ok(());
    }

    let total: u64 = entries.iter().map(|e| e.size_bytes).sum();
    println!("\n nodewipe doctor — {}\n", root.display());
    println!(" {:<20} {:>10}  {:>6}  {}", "Ecosystem", "Size", "Count", "Description");
    println!(" {}", "─".repeat(76));

    let mut by_kind = build_kind_list(&entries);
    by_kind.sort_by(|a, b| b.1.cmp(&a.1));
    for (kind, size, count) in &by_kind {
        println!(" {:<20} {:>10}  {:>6}  {}",
            kind.label(), human_size(*size), count,
            kind.description().chars().take(44).collect::<String>());
    }
    println!(" {}", "─".repeat(76));
    println!(" {:<20} {:>10}  {:>6}", "TOTAL", human_size(total), entries.len());

    let stale = find_stale_projects(&entries, stale_days);
    println!();
    if stale.is_empty() {
        println!(" ✓  No stale projects (source changed within {}d).", stale_days);
    } else {
        println!(" ⚠  {} stale project(s) — source unchanged for {}+ days:\n", stale.len(), stale_days);
        println!(" {:<50} {:>10}  {:>8}", "Project", "Size", "Days idle");
        println!(" {}", "─".repeat(72));
        for p in &stale {
            println!(" {:<50} {:>10}  {:>8}",
                truncate(&p.root.display().to_string(), 50),
                human_size(p.total_artifact_bytes),
                p.days_since_source_change);
        }
        println!("\n Run `nodewipe scan --older-than {}d --root <path>` to clean up.", stale_days);
    }
    println!();
    Ok(())
}

fn build_kind_list(entries: &[nodewipe_core::Entry]) -> Vec<(ArtifactKind, u64, usize)> {
    use std::collections::HashMap;
    let mut map: HashMap<ArtifactKind, (u64, usize)> = HashMap::new();
    for e in entries { let v = map.entry(e.kind).or_insert((0,0)); v.0 += e.size_bytes; v.1 += 1; }
    map.into_iter().map(|(k,(s,c))| (k,s,c)).collect()
}

fn build_summary(entries: &[nodewipe_core::Entry]) -> Vec<serde_json::Value> {
    build_kind_list(entries).into_iter().map(|(kind, size, count)| {
        serde_json::json!({"kind": kind.slug(), "label": kind.label(), "total_bytes": size, "count": count})
    }).collect()
}

fn human_size(bytes: u64) -> String {
    const U: [&str;5] = ["B","KB","MB","GB","TB"];
    let mut s = bytes as f64; let mut u = 0;
    while s >= 1024.0 && u < 4 { s /= 1024.0; u += 1; }
    format!("{s:.2} {}", U[u])
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max { s.to_string() } else { format!("…{}", &s[s.len().saturating_sub(max-1)..]) }
}
