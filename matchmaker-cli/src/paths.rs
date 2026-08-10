use std::{
    ffi::OsStr,
    fs,
    io,
    path::{Path, PathBuf},
};

use cba::expr_as_path_fn;

use crate::clap::LIBRARY_FULL;

fn config_dir_impl() -> Option<PathBuf> {
    if let Some(env_val) = std::env::var_os("MATCHMAKER_CONFIG_DIR") {
        let env_path = PathBuf::from(env_val);
        if env_path.exists() {
            return Some(env_path);
        }
    }

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
        .unwrap_or(std::path::Path::new(""))
        .join("presets")
);

#[cfg(not(debug_assertions))]
expr_as_path_fn!(
    default_config_path,
    config_dir_impl().unwrap_or_default().join("config.toml")
);

/// Return all installed `.toml` presets as sorted absolute paths.
///
/// Files named `base.toml` are configuration parents rather than selectable
/// presets and are intentionally omitted.
pub fn preset_paths() -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_preset_paths(presets_path(), &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_preset_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_preset_paths(&path, paths)?;
        } else if file_type.is_file()
            && path.extension() == Some(OsStr::new("toml"))
            && path.file_name() != Some(OsStr::new("base.toml"))
        {
            paths.push(path.canonicalize()?);
        }
    }

    Ok(())
}
