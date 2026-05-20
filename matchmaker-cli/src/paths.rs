use std::path::PathBuf;

use cba::expr_as_path_fn;

use crate::clap::LIBRARY_FULL;

fn config_dir_impl() -> Option<PathBuf> {
    if let Some(home) = dirs::home_dir() {
        let config = home.join(".config").join(LIBRARY_FULL);
        if config.exists() {
            return Some(config);
        }
    };

    dirs::config_dir().map(|x| x.join(LIBRARY_FULL))
}

pub fn state_dir_impl() -> Option<PathBuf> {
    dirs::state_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join(".local").join("state")))
        .map(|x| x.join(LIBRARY_FULL))
}

expr_as_path_fn!(state_dir, state_dir_impl().unwrap_or_default());
expr_as_path_fn!(
    last_key_path,
    state_dir_impl().unwrap_or_default().join("last_key")
);

#[cfg(debug_assertions)]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("dev.toml")
);

expr_as_path_fn!(
    presets_path,
    default_config_path()
        .parent()
        .unwrap_or(&std::path::Path::new(""))
        .join("presets")
);

#[cfg(not(debug_assertions))]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("config.toml")
);
