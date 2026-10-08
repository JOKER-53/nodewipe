pub mod config;
pub mod deleter;
pub mod doctor;
pub mod history;
pub mod ignorefile;
pub mod model;
pub mod scanner;
pub mod stale;
pub mod workspace;

pub use config::{Config, load_config};
pub use deleter::{DeleteMode, DeleteResult, delete, restore};
pub use history::{
    EcosystemStats, HistoryEntry, append as history_append, load as history_load,
    stats_by_ecosystem,
};
pub use ignore::gitignore::Gitignore;
pub use ignorefile::load_patterns as load_ignore_patterns;
pub use model::{ArtifactKind, Entry, PackageManager, ScanOptions, WorkspaceGroup};
pub use scanner::{classify_path, scan};
pub use stale::{StaleProject, find_stale_projects};
pub use workspace::{annotate_workspace_roots, group_by_workspace};
