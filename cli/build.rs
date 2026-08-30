/// Build script for nodewipe-cli.
///
/// Generates shell completions (bash, zsh, fish, elvish, powershell) and a
/// man page from the Clap definition at compile time. Outputs go to
/// `assets/completions/` and `assets/man/` inside the crate directory, which
/// the AUR PKGBUILD and install.sh can then install to the right system paths.
use clap::CommandFactory;
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
use std::io::Error;

// Pull in the shared CLI definition without importing the full crate.
include!("src/cli_def.rs");

fn main() -> Result<(), Error> {
    // Tell Cargo to re-run this script only when the CLI definition changes.
    println!("cargo:rerun-if-changed=src/cli_def.rs");

    let out_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set"),
    );

    // ── Shell completions ─────────────────────────────────────────────────────
    let completions_dir = out_dir.join("assets").join("completions");
    std::fs::create_dir_all(&completions_dir)?;

    let mut cmd = Cli::command();

    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::Elvish, Shell::PowerShell] {
        generate_to(shell, &mut cmd, "nodewipe", &completions_dir)?;
    }

    // ── Man page ──────────────────────────────────────────────────────────────
    let man_dir = out_dir.join("assets").join("man");
    std::fs::create_dir_all(&man_dir)?;

    let cmd = Cli::command();
    let man = Man::new(cmd);
    let mut buf = Vec::new();
    man.render(&mut buf)?;
    std::fs::write(man_dir.join("nodewipe.1"), buf)?;

    Ok(())
}
