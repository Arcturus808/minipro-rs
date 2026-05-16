//! Build script: generate shell completion files for bash, zsh, fish, and
//! PowerShell into `$OUT_DIR/completions/`.
//!
//! Packaging scripts can copy these files to the appropriate system locations:
//!   bash  →  /usr/share/bash-completion/completions/minipro
//!   zsh   →  /usr/share/zsh/site-functions/_minipro
//!   fish  →  /usr/share/fish/vendor_completions.d/minipro.fish
//!   ps    →  (PowerShell profile directory)

use std::{env, fs, path::PathBuf};

use clap::{ArgAction, CommandFactory, Parser};
use clap_complete::{generate_to, shells};

include!("src/cli.rs");

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let comp_dir = out_dir.join("completions");
    fs::create_dir_all(&comp_dir).expect("cannot create completions dir");

    let mut cmd = Cli::command();

    for shell in [
        shells::Shell::Bash,
        shells::Shell::Zsh,
        shells::Shell::Fish,
        shells::Shell::PowerShell,
    ] {
        generate_to(shell, &mut cmd, "minipro", &comp_dir)
            .unwrap_or_else(|e| panic!("failed to generate {shell:?} completions: {e}"));
    }

    println!("cargo:rerun-if-changed=src/cli.rs");
    println!("cargo:rerun-if-changed=build.rs");
}
