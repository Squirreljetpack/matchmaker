use std::path::PathBuf;

use cli_boilerplate_automation::expr_as_path_fn;

use crate::clap::BINARY_FULL;

fn config_dir_impl() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let config = home.join(".config").join(BINARY_FULL);
        if config.exists() {
            return Some(config);
        }
    };

    dirs::config_dir().map(|x| x.join(BINARY_FULL))
}

pub fn state_dir() -> Option<PathBuf> {
    if let Some(ret) = dirs::state_dir() {
        Some(ret)
    } else {
        dirs::home_dir().map(|home| home.join(".local").join("state"))
    }
}

expr_as_path_fn!(logs_dir, state_dir().unwrap_or_default());
#[cfg(debug_assertions)]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("dev.toml")
);
#[cfg(not(debug_assertions))]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("config.toml")
);
