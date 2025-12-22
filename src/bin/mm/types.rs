use std::{
    path::{Path, PathBuf},
    sync::LazyLock,
};

use clap::Parser;
use matchmaker::config::Config;
use anyhow::Result;

#[derive(Debug, Parser, Default, Clone)]
pub struct Cli {
    #[arg(long, value_name = "DIR", default_value_os = config_dir().as_os_str() )]
    pub config: PathBuf,
    #[arg(long, value_name = "CONFIG_STR")]
    pub config_string: Option<String>,
    #[arg(long)]
    pub dump_config: bool,
    #[arg(short = 'F')]
    pub fullscreen: bool,
    #[arg(long)]
    pub test_keys: bool,
}

// ---------- DEFAULTS ----------

pub static BINARY_FULL: &str = "matchmaker";
pub static BINARY_SHORT: &str = "mm";

fn config_dir_impl() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let config = home.join(".config").join(BINARY_FULL);
        if config.exists() {
            return Some(config);
        }
    };

    dirs::config_dir().map(|x| x.join(BINARY_FULL))
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
            .join(BINARY_FULL)
        })
    }
}

pub fn logs_dir() -> &'static Path {
    static LOGS_DIR: LazyLock<PathBuf> = LazyLock::new(|| state_dir().unwrap_or_default());
    &LOGS_DIR
}

// ----------------------- CONFIG
impl Cli {
    pub fn merge_config(&self, config: &mut Config) -> Result<()> {
        if let Some(s) = &self.config_string {
            *config = toml::from_str(s)?;
        };
        if self.fullscreen {
            config.tui.layout = None;
        }
        Ok(())
    }
}
