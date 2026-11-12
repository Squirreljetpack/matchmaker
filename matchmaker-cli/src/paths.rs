use std::{
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use cba::{birs, expr_as_path_fn};

use crate::clap::LIBRARY_FULL;

expr_as_path_fn!(state_dir, birs::state_dir(LIBRARY_FULL).unwrap_or_default());
expr_as_path_fn!(
    last_key_path,
    birs::state_dir(LIBRARY_FULL)
        .unwrap_or_default()
        .join("last_key")
);

#[cfg(debug_assertions)]
expr_as_path_fn!(
    default_config_path,
    birs::config_dir(LIBRARY_FULL)
        .unwrap_or_default()
        .join("dev.toml")
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
    birs::config_dir(LIBRARY_FULL)
        .unwrap_or_default()
        .join("config.toml")
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
            if path.file_name() == Some(OsStr::new("themes")) {
                let main_toml = path.join("main.toml");
                if main_toml.is_file() {
                    paths.push(main_toml.canonicalize()?);
                }
            } else {
                collect_preset_paths(&path, paths)?;
            }
        } else if file_type.is_file()
            && path.extension() == Some(OsStr::new("toml"))
            && path.file_name() != Some(OsStr::new("base.toml"))
        {
            paths.push(path.canonicalize()?);
        }
    }

    Ok(())
}
