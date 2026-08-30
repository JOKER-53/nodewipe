pub mod config;
pub mod deleter;
pub mod history;
pub mod ignorefile;
pub mod model;
pub mod scanner;
pub mod doctor;
pub mod stale;
pub mod workspace;

pub use config::{load_config, Config};
pub use deleter::{delete, restore, DeleteMode, DeleteResult};
pub use history::{append as history_append, load as history_load, stats_by_ecosystem, EcosystemStats, HistoryEntry};
pub use ignorefile::load_patterns as load_ignore_patterns;
pub use ignore::gitignore::Gitignore;
pub use model::{ArtifactKind, Entry, PackageManager, ScanOptions, WorkspaceGroup};
pub use scanner::{classify_path, scan};
pub use stale::{find_stale_projects, StaleProject};
pub use workspace::{annotate_workspace_roots, group_by_workspace};
