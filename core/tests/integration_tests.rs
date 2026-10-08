use nodewipe_core::history::{daily_totals, format_date, stats_by_ecosystem};
use nodewipe_core::workspace::{annotate_workspace_roots, group_by_workspace};
use nodewipe_core::{
    ArtifactKind, DeleteMode, HistoryEntry, ScanOptions, classify_path, delete,
    load_ignore_patterns, restore, scan,
};
use std::fs::{self, File};
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_classify_path_marker_rules() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // node_modules requires no marker
    let nm = root.join("node_modules");
    fs::create_dir(&nm).unwrap();
    assert_eq!(classify_path(&nm), Some(ArtifactKind::NodeModules));

    // target without Cargo.toml should be None
    let target = root.join("target");
    fs::create_dir(&target).unwrap();
    assert_eq!(classify_path(&target), None);

    // target with sibling Cargo.toml should be RustTarget
    File::create(root.join("Cargo.toml")).unwrap();
    assert_eq!(classify_path(&target), Some(ArtifactKind::RustTarget));

    // venv without pyvenv.cfg should be None
    let venv = root.join("venv");
    fs::create_dir(&venv).unwrap();
    assert_eq!(classify_path(&venv), None);

    // venv with pyvenv.cfg inside should be PythonVenv
    File::create(venv.join("pyvenv.cfg")).unwrap();
    assert_eq!(classify_path(&venv), Some(ArtifactKind::PythonVenv));

    // dist with pyproject.toml should be GenericDist
    let py_project = root.join("py_proj");
    fs::create_dir(&py_project).unwrap();
    File::create(py_project.join("pyproject.toml")).unwrap();
    let dist = py_project.join("dist");
    fs::create_dir(&dist).unwrap();
    assert_eq!(classify_path(&dist), Some(ArtifactKind::GenericDist));
}

#[test]
fn test_scan_and_ignore() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let proj1 = root.join("proj1");
    fs::create_dir_all(proj1.join("node_modules")).unwrap();
    File::create(proj1.join("node_modules").join("pkg.js")).unwrap();

    let proj2 = root.join("ignored_proj");
    fs::create_dir_all(proj2.join("node_modules")).unwrap();
    File::create(proj2.join("node_modules").join("pkg.js")).unwrap();

    // Create .nodewipeignore in root
    let mut ignore_file = File::create(root.join(".nodewipeignore")).unwrap();
    writeln!(ignore_file, "ignored_proj").unwrap();

    let ignore_patterns = load_ignore_patterns(root);
    let opts = ScanOptions {
        root: root.to_path_buf(),
        ignore_patterns,
        ..Default::default()
    };

    let entries = scan(&opts).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, ArtifactKind::NodeModules);
    assert!(entries[0].path.starts_with(&proj1));
}

#[test]
fn test_workspace_detection() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Cargo monorepo
    let cargo_root = root.join("cargo_mono");
    fs::create_dir_all(&cargo_root).unwrap();
    let mut cargo_toml = File::create(cargo_root.join("Cargo.toml")).unwrap();
    writeln!(cargo_toml, "[workspace]\nmembers = [\"app\"]").unwrap();

    let sub_app = cargo_root.join("app");
    fs::create_dir_all(sub_app.join("target")).unwrap();
    File::create(cargo_root.join("target").join("dummy")).ok();

    let mut entries = vec![nodewipe_core::Entry {
        path: sub_app.join("target"),
        size_bytes: 1024,
        kind: ArtifactKind::RustTarget,
        package_manager: None,
        last_modified: None,
        workspace_root: None,
    }];

    annotate_workspace_roots(&mut entries, root);
    assert_eq!(entries[0].workspace_root, Some(cargo_root.clone()));

    let groups = group_by_workspace(entries);
    assert!(groups.contains_key(&Some(cargo_root)));
}

#[test]
fn test_archive_and_restore_cycle() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let project = root.join("my_app");
    let nm = project.join("node_modules");
    fs::create_dir_all(&nm).unwrap();
    let test_file = nm.join("hello.txt");
    fs::write(&test_file, b"content").unwrap();

    // Delete with Archive mode
    let res = delete(&nm, DeleteMode::Archive, 7).unwrap();
    assert!(!nm.exists());
    let archive_path = res.archive_path.expect("archive_path should be set");
    assert!(archive_path.exists());

    // Restore from archive
    let restored = restore(&archive_path).unwrap();
    assert_eq!(restored, nm);
    assert!(nm.exists());
    assert_eq!(fs::read(nm.join("hello.txt")).unwrap(), b"content");
}

#[test]
fn test_history_stats_and_formatting() {
    assert_eq!(format_date(1700000000), "2023-11-14");

    let entries = vec![
        HistoryEntry {
            deleted_at: 1700000000,
            date: "2023-11-14".into(),
            path: "/path/to/target".into(),
            kind: "rust_target".into(),
            size_bytes: 500,
            mode: "trash".into(),
        },
        HistoryEntry {
            deleted_at: 1700001000,
            date: "2023-11-14".into(),
            path: "/path/to/venv".into(),
            kind: "venv".into(),
            size_bytes: 200,
            mode: "archive".into(),
        },
        HistoryEntry {
            deleted_at: 1700086400,
            date: "2023-11-15".into(),
            path: "/path/to/target2".into(),
            kind: "rust_target".into(),
            size_bytes: 300,
            mode: "permanent".into(),
        },
    ];

    let eco_stats = stats_by_ecosystem(&entries);
    assert_eq!(eco_stats.len(), 2);
    assert_eq!(eco_stats[0].kind, "rust_target");
    assert_eq!(eco_stats[0].total_bytes, 800);
    assert_eq!(eco_stats[0].count, 2);

    let days = daily_totals(&entries);
    assert_eq!(days.len(), 2);
    assert_eq!(days[0].date, "2023-11-14");
    assert_eq!(days[0].bytes_freed, 700);
}
