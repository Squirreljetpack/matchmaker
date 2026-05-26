use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use std::env;

// include!("build/completions_mock.rs");

// -----------------------------------------------------------------------------
// Include
// -----------------------------------------------------------------------------
include!("src/clap.rs");

fn generate_completions() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = manifest_dir.join("assets").join("completions");
    std::fs::create_dir_all(&out_dir).unwrap();
    
    let mut cmd = Cli::command();
    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        generate_to(shell, &mut cmd, BINARY_SHORT, &out_dir).unwrap();
    }
}

fn main() {
    println!("cargo:rerun-if-changed=src/clap.rs");
    println!("cargo:rerun-if-changed=build.rs");

    if env::var_os("SKIP_COMPLETIONS").is_none() {
        generate_completions();
    }
}
