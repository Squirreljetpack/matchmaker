use std::{
    ffi::OsString, path::{Path, PathBuf}, sync::LazyLock
};

use clap::Parser;
use anyhow::Result;

use crate::config::Config;

#[derive(Debug, Parser, Default, Clone)]
pub struct Cli {
    #[arg(long, value_name = "PATH_OR_STRING")]
    pub config: Option<OsString>,
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

pub fn default_config_path() -> &'static Path {
    #[cfg(debug_assertions)]
    {
        static DEFAULT_PATH: LazyLock<PathBuf> =
        LazyLock::new(|| config_dir_impl().unwrap_or_default().join("dev.toml"));
        &DEFAULT_PATH
    }
    #[cfg(not(debug_assertions))]
    {
        static DEFAULT_PATH: LazyLock<PathBuf> =
        LazyLock::new(|| config_dir_impl().unwrap_or_default().join("config.toml"));
        &DEFAULT_PATH
    }
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
    /// merge cli opts (not including config_path) into config
    pub fn merge_config(&self, config: &mut Config) -> Result<()> {
        if self.fullscreen {
            config.tui.layout = None;
        }
        Ok(())
    }
}
