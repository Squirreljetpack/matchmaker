use std::{
    ffi::{OsStr, OsString},
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

/// Every OS prefix a shipped file may carry.
const ALL_OS_PREFIXES: [&str; 5] = ["win.", "windows.", "macos.", "linux.", "unix."];

/// The prefixes that apply to the running platform, most specific first.
///
/// `win.` and `windows.` are one platform under two spellings: the shipped
/// presets use `win.` while `-o FOLDER` has always probed `<consts::OS>.main.toml`.
/// On the unixes the OS prefix wins over the family prefix, so a `linux.`/`macos.`
/// file takes precedence over its `unix.` sibling.
pub(crate) fn os_prefixes() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &["win.", "windows."]
    } else if cfg!(target_os = "macos") {
        &["macos.", "unix."]
    } else if cfg!(target_os = "linux") {
        &["linux.", "unix."]
    } else {
        &["unix."]
    }
}

/// Split an OS prefix (`win.`, `windows.`, `macos.`, `linux.`, `unix.`) off
/// `name`, returning the prefix and the remainder.
pub fn split_os_prefix(name: &str) -> Option<(&'static str, &str)> {
    ALL_OS_PREFIXES
        .iter()
        .find_map(|prefix| name.strip_prefix(prefix).map(|rest| (*prefix, rest)))
}

/// A file name as a platform with `prefixes` knows it: `Some(name)` for an
/// unprefixed name or one carrying one of those prefixes (prefix removed),
/// `None` when it is prefixed for another platform.
pub fn platform_name_with<'a>(name: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    match split_os_prefix(name) {
        Some((prefix, rest)) => prefixes.contains(&prefix).then_some(rest),
        None => Some(name),
    }
}

/// [`platform_name_with`] for the running platform.
pub fn platform_name(name: &str) -> Option<&str> {
    platform_name_with(name, os_prefixes())
}

/// The prefixed sibling of plain `name` among `siblings`, if one exists, for the
/// running platform.
///
/// A platform-specific file wins over the plain file of the same name, which is
/// either the fallback for platforms without a variant or another platform's
/// own dialect.
pub fn shadowing_variant<'a, I: IntoIterator<Item = &'a str>>(
    name: &str,
    siblings: I,
) -> Option<&'a str> {
    shadowing_variant_with(name, siblings, os_prefixes())
}

/// [`shadowing_variant`] for a platform with `prefixes`.
pub fn shadowing_variant_with<'a, I: IntoIterator<Item = &'a str>>(
    name: &str,
    siblings: I,
    prefixes: &[&str],
) -> Option<&'a str> {
    if split_os_prefix(name).is_some() {
        return None;
    }
    siblings.into_iter().find(|sibling| {
        prefixes.iter().any(|prefix| {
            sibling
                .strip_prefix(prefix)
                .is_some_and(|rest| rest == name)
        })
    })
}

/// `std::fs::canonicalize` without the Windows verbatim (`\\?\`) prefix.
///
/// Preset paths are handed to shells and PowerShell cmdlets which either reject
/// verbatim paths outright (`cmd.exe` as a working directory, `Join-Path` as an
/// anchor) or inherit the spelling into every path they print.
pub fn canonicalize(path: impl AsRef<Path>) -> io::Result<PathBuf> {
    let path = fs::canonicalize(path)?;
    Ok(strip_verbatim(path))
}

#[cfg(windows)]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    // `canonicalize` only produces the two verbatim shapes `\\?\C:\…` and
    // `\\?\UNC\server\share`; anything else is left alone.
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = text.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(not(windows))]
fn strip_verbatim(path: PathBuf) -> PathBuf {
    path
}

/// Resolve `name` inside `dir` for this platform: the first existing candidate
/// among this platform's prefixed spellings, then `name` itself.
///
/// The plain name is the fallback rather than the first choice so an OS-specific
/// sibling always shadows it, and a preset that ships without prefixed variants
/// keeps working unchanged.
pub fn resolve_platform_path(dir: &Path, name: impl AsRef<OsStr>) -> PathBuf {
    let name = name.as_ref();
    for prefix in os_prefixes() {
        let mut candidate = OsString::from(prefix);
        candidate.push(name);
        let path = dir.join(candidate);
        if path.is_file() {
            return path;
        }
    }
    dir.join(name)
}

/// [`resolve_platform_path`] for a whole path, applying the prefix to its file
/// name only.
pub fn resolve_platform_file(path: &Path) -> PathBuf {
    resolve_platform_path(
        path.parent().unwrap_or(Path::new("")),
        path.file_name().unwrap_or_default(),
    )
}

/// Return all installed `.toml` presets as sorted absolute paths.
///
/// Files prefixed for another platform are omitted, and `base.toml` (prefixed or
/// not) is a configuration parent rather than a selectable preset. The returned
/// paths keep their on-disk names.
pub fn preset_paths() -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    collect_preset_paths(presets_path(), &mut paths)?;
    paths.sort();
    Ok(paths)
}

fn collect_preset_paths(dir: &Path, paths: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if let Some(name) = entry.file_name().to_str() {
            entries.push((name.to_string(), entry.path(), file_type));
        }
    }

    let names: Vec<&str> = entries.iter().map(|(name, ..)| name.as_str()).collect();

    for (name, path, file_type) in &entries {
        let Some(local) = platform_name(name) else {
            continue;
        };

        if file_type.is_dir() {
            if name == "themes" {
                let main_toml = resolve_platform_path(path, "main.toml");
                if main_toml.is_file() {
                    paths.push(canonicalize(main_toml)?);
                }
            } else {
                collect_preset_paths(path, paths)?;
            }
        } else if file_type.is_file() && local.ends_with(".toml") && local != "base.toml" {
            // The plain file is the fallback for platforms without a variant:
            // skip it when this platform has one (`ps/main.toml` beside
            // `ps/win.main.toml`), so the list holds one entry per preset.
            if shadowing_variant(name, names.iter().copied()).is_some() {
                continue;
            }
            paths.push(canonicalize(path)?);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOWS: [&str; 2] = ["win.", "windows."];
    const LINUX: [&str; 2] = ["linux.", "unix."];

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn platform_name_keeps_this_platform_and_hides_the_rest() {
        for prefix in os_prefixes() {
            let name = format!("{prefix}x.toml");
            assert_eq!(platform_name(&name), Some("x.toml"), "{name}");
        }
        assert_eq!(platform_name("x.toml"), Some("x.toml"));
        // `windows.` is not a prefix on any unix, and `win.` is not one on Windows.
        let foreign = if cfg!(target_os = "windows") {
            "linux."
        } else {
            "win."
        };
        assert_eq!(platform_name(&format!("{foreign}x.toml")), None);
    }

    #[test]
    fn platform_name_accepts_any_prefix_list() {
        assert_eq!(platform_name_with("win.x.toml", &WINDOWS), Some("x.toml"));
        assert_eq!(
            platform_name_with("windows.x.toml", &WINDOWS),
            Some("x.toml")
        );
        assert_eq!(platform_name_with("win.x.toml", &LINUX), None);
        assert_eq!(platform_name_with("unix.x.toml", &LINUX), Some("x.toml"));
        assert_eq!(platform_name_with("x.toml", &WINDOWS), Some("x.toml"));
        // The prefix is stripped once, and a bare prefix leaves a non-preset.
        assert_eq!(
            platform_name_with("win.win.x.toml", &WINDOWS),
            Some("win.x.toml")
        );
        assert_eq!(platform_name_with("unix.toml", &LINUX), Some("toml"));
    }

    #[test]
    fn a_variant_shadows_its_plain_sibling() {
        let names = ["x.toml", "win.x.toml", "y.toml", "unix.y.toml"];
        assert_eq!(
            shadowing_variant_with("x.toml", names, &WINDOWS),
            Some("win.x.toml")
        );
        assert_eq!(
            shadowing_variant_with("y.toml", names, &LINUX),
            Some("unix.y.toml")
        );
        assert_eq!(shadowing_variant_with("x.toml", names, &LINUX), None);
        assert_eq!(shadowing_variant_with("y.toml", names, &WINDOWS), None);
        // A prefixed file is never shadowed, and neither is an absent sibling.
        assert_eq!(shadowing_variant_with("win.x.toml", names, &WINDOWS), None);
        assert_eq!(shadowing_variant_with("z.toml", names, &WINDOWS), None);
    }

    #[test]
    fn resolve_prefers_the_platform_variant_over_the_plain_file() {
        let dir = scratch("mm_resolve_platform_path");
        fs::write(dir.join("x.toml"), "").unwrap();
        assert_eq!(resolve_platform_path(&dir, "x.toml"), dir.join("x.toml"));

        let variant = format!("{}x.toml", os_prefixes()[0]);
        fs::write(dir.join(&variant), "").unwrap();
        assert_eq!(resolve_platform_path(&dir, "x.toml"), dir.join(&variant));

        // A nested name resolves inside its own directory.
        let sub = dir.join("sub");
        fs::create_dir_all(&sub).unwrap();
        let nested = sub.join(&variant);
        fs::write(&nested, "").unwrap();
        assert_eq!(resolve_platform_file(&sub.join("x.toml")), nested);
    }

    #[test]
    fn canonicalize_returns_a_plain_absolute_path() {
        let dir = scratch("mm_canonicalize");
        let file = dir.join("x.toml");
        fs::write(&file, "").unwrap();
        let resolved = canonicalize(dir.join("./x.toml")).unwrap();
        assert_eq!(resolved, canonicalize(&file).unwrap());
        assert!(resolved.is_absolute());
        let text = resolved.to_string_lossy();
        assert!(!text.starts_with(r"\\?"), "{text}");
    }
}
