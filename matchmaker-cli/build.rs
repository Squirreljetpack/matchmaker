use clap::CommandFactory;
use clap_complete::{Shell, generate_to};
use std::env;

// include!("build/completions_mock.rs");

// -----------------------------------------------------------------------------
// Include
// -----------------------------------------------------------------------------
include!("src/clap.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/cli/types.rs");
    println!("cargo:rerun-if-changed=build.rs");

    let out_dir = {
        let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let out_dir = manifest_dir.join("assets").join("completions");
        std::fs::create_dir_all(&out_dir).unwrap();
        out_dir
    };

    let mut cmd = Cli::command();

    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish, Shell::PowerShell] {
        generate_to(shell, &mut cmd, BINARY_SHORT, &out_dir).unwrap();
    }
}
