use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use clap::Parser;
use matchmaker::config::Config;

#[derive(Debug, Parser, Default, Clone)]
pub struct Cli {
    #[arg(long, value_name = "DIR", default_value_os = config_dir().as_os_str() )]
    pub config: PathBuf,
    #[arg(long)]
    pub dump_config: bool,
    #[arg(short = 'F')]
    pub fullscreen: bool,
    #[arg(long)]
    pub test_keys: bool,
}

// ---------- DEFAULTS ----------

fn config_dir_impl() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let config = home.join(".config").join(env!("CARGO_PKG_NAME"));
        if config.exists() {
            return Some(config);
        }
    };

    dirs::config_dir().map(|x| x.join(env!("CARGO_PKG_NAME")))
}

pub fn config_dir() -> &'static Path {
    static DEFAULT_PATH: LazyLock<PathBuf> =
        LazyLock::new(|| config_dir_impl().unwrap_or_default());
    &DEFAULT_PATH
}

pub fn state_dir() -> Option<PathBuf> {
    if let Some(ret) = dirs::state_dir() {
        Some(ret)
    } else {
        dirs::home_dir().map(|home| {
            home.join(".local")
                .join("state")
                .join(env!("CARGO_PKG_NAME"))
        })
    }
}

pub fn logs_dir() -> &'static Path {
    static LOGS_DIR: LazyLock<PathBuf> = LazyLock::new(|| state_dir().unwrap_or_default());

    &LOGS_DIR
}

// ----------------------- CONFIG
impl Cli {
    pub fn merge_config(&self, config: &mut Config) {
        if self.fullscreen {
            config.tui.layout = None;
        }
    }
}
