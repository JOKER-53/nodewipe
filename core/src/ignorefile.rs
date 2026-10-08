use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::Path;

pub fn load_patterns(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);

    if let Some(home) = dirs::home_dir() {
        let global_ignore = home.join(".nodewipeignore");
        if global_ignore.exists() {
            let _ = builder.add(global_ignore);
        }
    }

    let local_ignore = root.join(".nodewipeignore");
    if local_ignore.exists() {
        let _ = builder.add(local_ignore);
    }

    builder.build().unwrap_or_else(|_| Gitignore::empty())
}

pub fn is_ignored(path: &Path, ignore: &Gitignore) -> bool {
    if ignore.is_empty() {
        return false;
    }

    // matched_path_or_any_parents tells us if the path or its ancestors are ignored
    let is_dir = true; // nodewipe scanner only passes directories to this function
    matches!(
        ignore.matched_path_or_any_parents(path, is_dir),
        ignore::Match::Ignore(_)
    )
}
